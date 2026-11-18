//! Acceptance criteria of spec node CORE.STO.CAS (AC-1 .. AC-7) and its requirements.
//!
//! Expected addresses come from `fixtures/core/sto/cas/make_fixtures.py` (Python hashlib),
//! never from this crate. Content is the SplitMix64 byte stream described there.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::CValue;
use fid_store::cas::{CHUNK_SIZE, MAX_CHUNKS, MAX_MANIFEST_BYTES};
use fid_store::{Address, BlobStore, CasError, ChunkManifest, ObjectRef, PutOpts};
use fid_types::Digest;
use sha2::{Digest as _, Sha256};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/core/sto/cas");
const MIB: u64 = 1024 * 1024;
const GAMMA: u64 = 0x9E37_79B9_7F4A_7C15;

/// A unique directory under the system temp dir, removed on drop.
struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> TempDir {
        static N: AtomicU32 = AtomicU32::new(0);
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().subsec_nanos();
        let name = format!("fid-cas-{tag}-{}-{nanos}-{}", std::process::id(), N.fetch_add(1, Ordering::SeqCst));
        let p = std::path::absolute(std::env::temp_dir().join(name)).unwrap();
        std::fs::create_dir_all(&p).unwrap();
        TempDir(p)
    }

    fn store(&self) -> BlobStore {
        BlobStore::open(&self.0.join("blobs")).unwrap()
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

// ---------------------------------------------------------------- SplitMix64 content

fn mix(mut z: u64) -> u64 {
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// The SplitMix64 byte stream of `seed`, cut to `size` bytes, as a reader.
struct Stream {
    seed: u64,
    pos: u64,
    size: u64,
}

impl Stream {
    fn new(seed: u64, size: u64) -> Stream {
        Stream { seed, pos: 0, size }
    }
}

/// Byte `pos` of the stream of `seed` (word k uses state seed + (k + 1) * gamma).
fn word_at(seed: u64, k: u64) -> [u8; 8] {
    mix(seed.wrapping_add(k.wrapping_add(1).wrapping_mul(GAMMA))).to_le_bytes()
}

impl Read for Stream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let n = buf.len().min((self.size - self.pos) as usize);
        let mut i = 0;
        while i < n {
            let p = self.pos + i as u64;
            let w = word_at(self.seed, p / 8);
            let off = (p % 8) as usize;
            let take = (8 - off).min(n - i);
            buf[i..i + take].copy_from_slice(&w[off..off + take]);
            i += take;
        }
        self.pos += n as u64;
        Ok(n)
    }
}

fn bytes(seed: u64, size: u64) -> Vec<u8> {
    let mut v = Vec::new();
    Stream::new(seed, size).read_to_end(&mut v).unwrap();
    v
}

fn bytes_at(seed: u64, start: u64, len: u64) -> Vec<u8> {
    let mut s = Stream::new(seed, start + len);
    s.pos = start;
    let mut v = Vec::new();
    s.read_to_end(&mut v).unwrap();
    v
}

/// A writer that checks everything written against a stream and hashes it.
struct Compare {
    expect: Stream,
    hasher: Sha256,
    written: u64,
}

impl Write for Compare {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let mut want = vec![0u8; buf.len()];
        let n = self.expect.read(&mut want)?;
        assert_eq!(n, buf.len(), "more bytes than the content has");
        assert!(want == buf, "streamed bytes differ at offset {}", self.written);
        self.hasher.update(buf);
        self.written += buf.len() as u64;
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[test]
fn splitmix_reference_value() {
    // Published first output of SplitMix64 for seed 0; the Python script asserts the same.
    assert_eq!(u64::from_le_bytes(word_at(0, 0)), 0xE220_A839_7B1D_CDAF);
    assert_eq!(bytes_at(7, 13, 20), bytes(7, 33)[13..]);
}

// ---------------------------------------------------------------- fixtures

struct Case {
    seed: u64,
    size: u64,
    kind: String,
    address: String,
    chunks: Vec<String>,
}

fn field<'a>(o: &'a CValue, k: &str) -> &'a CValue {
    let CValue::Obj(m) = o else { panic!("not an object") };
    m.get(k).unwrap_or_else(|| panic!("missing {k}"))
}

fn text(v: &CValue) -> String {
    let CValue::Str(s) = v else { panic!("not a string: {v:?}") };
    s.clone()
}

fn number(v: &CValue) -> u64 {
    let CValue::Num(n) = v else { panic!("not a number: {v:?}") };
    *n as u64
}

fn cases() -> Vec<Case> {
    let raw = std::fs::read(Path::new(FIXTURES).join("blobs.json")).unwrap();
    let doc = fid_canon::parse(&raw).unwrap();
    assert_eq!(number(field(&doc, "chunk_size")), CHUNK_SIZE);
    let CValue::Arr(list) = field(&doc, "blobs") else { panic!("blobs") };
    list.iter()
        .map(|b| Case {
            seed: text(field(b, "seed")).parse().unwrap(),
            size: number(field(b, "size")),
            kind: text(field(b, "kind")),
            address: text(field(b, "address")),
            chunks: match field(b, "chunks") {
                CValue::Arr(c) => c.iter().map(text).collect(),
                _ => panic!("chunks"),
            },
        })
        .collect()
}

fn files_under(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for e in std::fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        if p.is_dir() {
            out.extend(files_under(&p));
        } else {
            out.push(p);
        }
    }
    out
}

fn flip_byte(path: &Path, at: usize) {
    let mut b = std::fs::read(path).unwrap();
    b[at] ^= 0x01;
    std::fs::write(path, b).unwrap();
}

// ---------------------------------------------------------------- AC-1

/// AC-1: 100 blobs from 0 B to 20 MiB; addresses equal the independent hashlib values and
/// every read returns the original bytes.
#[test]
fn ac1_fixture_blobs_round_trip() {
    let cases = cases();
    assert_eq!(cases.len(), 100);
    assert_eq!(cases.iter().map(|c| c.size).min(), Some(0));
    assert_eq!(cases.iter().map(|c| c.size).max(), Some(20 * MIB));
    let t = TempDir::new("ac1");
    let store = t.store();
    for c in &cases {
        let addr = store.put(Stream::new(c.seed, c.size), PutOpts::default()).unwrap();
        assert_eq!(addr.digest().as_str(), c.address, "seed {} size {}", c.seed, c.size);
        assert_eq!(addr.size(), c.size);
        match (&addr, c.kind.as_str()) {
            (Address::Blob(_), "blob") => {
                assert!(c.size <= CHUNK_SIZE);
                assert_eq!(store.get(&addr).unwrap(), bytes(c.seed, c.size));
            }
            (Address::Object(o), "object") => {
                let m = store.manifest(o).unwrap();
                let listed: Vec<&str> = m.chunks.iter().map(Digest::as_str).collect();
                assert_eq!(listed, c.chunks);
                let mut cmp = Compare { expect: Stream::new(c.seed, c.size), hasher: Sha256::new(), written: 0 };
                assert_eq!(store.stream(&addr, &mut cmp).unwrap(), c.size);
                assert_eq!(cmp.written, c.size);
            }
            other => panic!("kind mismatch: {other:?}"),
        }
        assert!(store.has(&addr).unwrap());
    }
    assert!(files_under(&store.root().join("tmp")).is_empty());
}

// ---------------------------------------------------------------- AC-2

/// AC-2: the same 1 MiB put twice gives one address and one file.
#[test]
fn ac2_same_content_twice_is_stored_once() {
    let t = TempDir::new("ac2");
    let store = t.store();
    let a = store.put(Stream::new(42, MIB), PutOpts::default()).unwrap();
    let b = store.put(Stream::new(42, MIB), PutOpts { durable: true }).unwrap();
    assert_eq!(a, b);
    let files = files_under(&store.root().join("sha256"));
    assert_eq!(files, vec![store.path_of(&a.digest())]);
    assert!(files_under(&store.root().join("tmp")).is_empty());
    // Fan-out: sha256/<h0h1>/<h2h3>/<hex>.
    let hex = a.digest().hex().to_string();
    let rel: Vec<String> = files[0]
        .strip_prefix(store.root())
        .unwrap()
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect();
    assert_eq!(rel, vec!["sha256".to_string(), hex[0..2].to_string(), hex[2..4].to_string(), hex.clone()]);
}

// ---------------------------------------------------------------- AC-3

/// AC-3: one flipped byte on disk gives `integrity` naming the address, and no bytes.
#[test]
fn ac3_flipped_byte_is_integrity_error() {
    let t = TempDir::new("ac3");
    let store = t.store();
    let addr = store.put(Stream::new(3, MIB), PutOpts { durable: true }).unwrap();
    flip_byte(&store.path_of(&addr.digest()), 123_456);
    for result in [store.get(&addr), store.read_range(&addr, 0..10)] {
        match result {
            Err(CasError::Integrity(d)) => assert_eq!(d, addr.digest()),
            other => panic!("expected integrity, got {other:?}"),
        }
    }
    let mut out = Vec::new();
    let e = store.stream(&addr, &mut out).unwrap_err();
    assert_eq!(e.code(), "integrity");
    assert!(out.is_empty());

    // A chunk of a chunked object: the bad chunk is named and none of its bytes come out.
    let obj = store.put(Stream::new(4, 2 * CHUNK_SIZE + 5), PutOpts::default()).unwrap();
    let Address::Object(o) = obj else { panic!("expected an object") };
    let second = store.manifest(&o).unwrap().chunks[1];
    flip_byte(&store.path_of(&second), 7);
    let mut out = Vec::new();
    match store.stream(&obj, &mut out) {
        Err(CasError::Integrity(d)) => assert_eq!(d, second),
        other => panic!("expected integrity, got {other:?}"),
    }
    assert_eq!(out.len() as u64, CHUNK_SIZE, "only the verified first chunk was written");
    assert_eq!(store.read_range(&obj, CHUNK_SIZE..CHUNK_SIZE + 1).unwrap_err().code(), "integrity");
    assert_eq!(store.read_range(&obj, 0..10).unwrap(), bytes(4, 10));
    assert_eq!(store.get(&obj).unwrap_err().code(), "integrity");
}

// ---------------------------------------------------------------- AC-4

/// AC-4: a 20 MiB object is three chunks (8, 8, 4 MiB) and its name is the SHA-256 of the
/// canonical manifest in `manifest_20mib.json`.
#[test]
fn ac4_twenty_mib_object_manifest() {
    let case = cases().into_iter().find(|c| c.size == 20 * MIB).unwrap();
    let expected_manifest = std::fs::read(Path::new(FIXTURES).join("manifest_20mib.json")).unwrap();
    let t = TempDir::new("ac4");
    let store = t.store();
    let addr = store.put(Stream::new(case.seed, case.size), PutOpts { durable: true }).unwrap();
    let Address::Object(o) = addr else { panic!("expected an object") };
    assert_eq!(o.manifest_digest.as_str(), case.address);
    let m = store.manifest(&o).unwrap();
    assert_eq!(m.chunks.len(), 3);
    assert_eq!((0..3).map(|i| m.chunk_len(i)).collect::<Vec<_>>(), vec![8 * MIB, 8 * MIB, 4 * MIB]);
    for (i, d) in m.chunks.iter().enumerate() {
        assert_eq!(std::fs::metadata(store.path_of(d)).unwrap().len(), m.chunk_len(i));
    }
    assert_eq!(std::fs::read(store.path_of(&o.manifest_digest)).unwrap(), expected_manifest);
    assert_eq!(m.to_canonical_bytes().unwrap(), expected_manifest);
    assert_eq!(ChunkManifest::parse(&expected_manifest).unwrap(), m);
}

// ---------------------------------------------------------------- AC-5

const BUDGET: u64 = 2 * CHUNK_SIZE + MIB;

fn streamed_round_trip(size: u64, seed: u64) {
    let t = TempDir::new("ac5");
    let store = t.store();
    let mut source_hash = Sha256::new();
    {
        let mut s = Stream::new(seed, size);
        let mut buf = vec![0u8; MIB as usize];
        loop {
            let n = s.read(&mut buf).unwrap();
            if n == 0 {
                break;
            }
            source_hash.update(&buf[..n]);
        }
    }
    let source = source_hash.finalize();
    store.reset_probe();
    let addr = store.put(Stream::new(seed, size), PutOpts { durable: true }).unwrap();
    let put_peak = store.probe().peak_buffer_bytes;
    assert!(put_peak <= BUDGET, "put peak {put_peak} > {BUDGET}");
    store.reset_probe();
    let mut cmp = Compare { expect: Stream::new(seed, size), hasher: Sha256::new(), written: 0 };
    assert_eq!(store.stream(&addr, &mut cmp).unwrap(), size);
    let read_peak = store.probe().peak_buffer_bytes;
    assert!(read_peak <= BUDGET, "stream peak {read_peak} > {BUDGET}");
    assert!(read_peak >= CHUNK_SIZE, "the probe saw the chunk buffer");
    assert_eq!(cmp.hasher.finalize(), source, "streamed digest");
}

/// AC-5 at CI scale: a 41 MiB object written and streamed back within 2 chunks + 1 MiB.
#[test]
fn ac5_streaming_memory_is_bounded() {
    streamed_round_trip(41 * MIB + 3, 5);
}

/// AC-5 full size: a 50 GB object (override with `FID_CAS_AC5_BYTES`). Nightly only; run with
/// `cargo test --release -p fid-store --test cas -- --ignored ac5_fifty_gb`.
#[test]
#[ignore = "nightly: writes and reads 50 GB"]
fn ac5_fifty_gb() {
    let size = std::env::var("FID_CAS_AC5_BYTES").ok().and_then(|s| s.parse().ok()).unwrap_or(50_000_000_000u64);
    streamed_round_trip(size, 50);
}

// ---------------------------------------------------------------- AC-6

/// AC-6: a 100-byte range of a 64 MiB object reads and verifies exactly one chunk.
#[test]
fn ac6_range_read_touches_one_chunk() {
    let t = TempDir::new("ac6");
    let store = t.store();
    let addr = store.put(Stream::new(6, 64 * MIB), PutOpts::default()).unwrap();
    store.reset_probe();
    let got = store.read_range(&addr, 20_000_000..20_000_100).unwrap();
    assert_eq!(got, bytes_at(6, 20_000_000, 100));
    assert_eq!(store.probe().chunks_verified, 1);

    // A range across a chunk boundary touches two; an empty range none.
    store.reset_probe();
    let got = store.read_range(&addr, CHUNK_SIZE - 3..CHUNK_SIZE + 4).unwrap();
    assert_eq!(got, bytes_at(6, CHUNK_SIZE - 3, 7));
    assert_eq!(store.probe().chunks_verified, 2);
    store.reset_probe();
    assert!(store.read_range(&addr, 5..5).unwrap().is_empty());
    assert_eq!(store.probe().chunks_verified, 0);
    assert_eq!(store.read_range(&addr, 0..64 * MIB + 1).unwrap_err().code(), "out_of_range");
}

// ---------------------------------------------------------------- AC-7 and REQ-009

fn manifest_text(size: u64, chunk_size: u64, chunks: &[String]) -> Vec<u8> {
    let listed: Vec<String> = chunks.iter().map(|c| format!("\"{c}\"")).collect();
    format!(
        "{{\"chunk_size\":\"{chunk_size}\",\"chunks\":[{}],\"format\":\"fid-chunks@1\",\"size\":\"{size}\"}}",
        listed.join(",")
    )
    .into_bytes()
}

fn fake_digest(i: u64) -> String {
    format!("sha256:{i:064x}")
}

/// AC-7: a stored manifest listing 2^20 + 1 chunks fails with `manifest_invalid`.
#[test]
fn ac7_too_many_chunks_is_manifest_invalid() {
    let n = MAX_CHUNKS as u64 + 1;
    let chunks: Vec<String> = (0..n).map(fake_digest).collect();
    let size = n * CHUNK_SIZE;
    let text = manifest_text(size, CHUNK_SIZE, &chunks);
    assert!(text.len() as u64 > MAX_MANIFEST_BYTES);
    // Placed directly where the store would keep it.
    let t = TempDir::new("ac7");
    let store = t.store();
    let digest = Digest::from_bytes(Sha256::digest(&text).into());
    let path = store.path_of(&digest);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, &text).unwrap();
    let addr = Address::Object(ObjectRef { manifest_digest: digest, size });
    let mut out = Vec::new();
    assert_eq!(store.stream(&addr, &mut out).unwrap_err().code(), "manifest_invalid");
    assert!(out.is_empty());
    assert_eq!(store.read_range(&addr, 0..1).unwrap_err().code(), "manifest_invalid");
    assert_eq!(ChunkManifest::parse(&text).unwrap_err().code(), "manifest_invalid");
}

/// REQ-009: the chunk-count limit on its own, size disagreements and malformed manifests.
#[test]
fn req9_malformed_manifests_are_refused_without_panic() {
    let five: Vec<String> = (0..5).map(fake_digest).collect();
    let ok = manifest_text(5 * 10, 10, &five);
    assert!(ChunkManifest::parse(&ok).is_ok());
    let e = ChunkManifest::parse_with_limits(&ok, MAX_MANIFEST_BYTES, 4).unwrap_err();
    assert_eq!(e.code(), "manifest_invalid");
    assert!(e.to_string().contains("chunks"), "{e}");
    let e = ChunkManifest::parse_with_limits(&ok, 20, MAX_CHUNKS).unwrap_err();
    assert_eq!(e.code(), "manifest_invalid");

    let edit = |from: &str, to: &str| String::from_utf8(ok.clone()).unwrap().replace(from, to).into_bytes();
    let mut bad_digest = five.clone();
    bad_digest[1] = "sha256:XYZ".to_string();
    let bad: Vec<Vec<u8>> = vec![
        // Sizes that disagree with the chunk list, and bad chunk sizes.
        manifest_text(5 * 10 + 1, 10, &five),
        manifest_text(4 * 10, 10, &five),
        manifest_text(0, 10, &[]),
        manifest_text(50, 0, &five),
        manifest_text(50, CHUNK_SIZE + 1, &five),
        manifest_text(50, 10, &bad_digest),
        // Wrong format, number forms, non-canonical text, extra keys.
        edit("fid-chunks@1", "fid-chunks@2"),
        edit("\"size\":\"50\"", "\"size\":50"),
        edit("\"size\":\"50\"", "\"size\":\"050\""),
        edit(",\"format\"", ", \"format\""),
        edit("{", "{\"extra\":\"1\","),
        // Not a manifest at all.
        b"[]".to_vec(),
        b"{".to_vec(),
        b"null".to_vec(),
        vec![0xff, 0xfe, 0x00],
        Vec::new(),
        format!("{}{}", "[".repeat(100_000), "]".repeat(100_000)).into_bytes(),
    ];
    for (i, b) in bad.iter().enumerate() {
        match ChunkManifest::parse(b) {
            Err(e) => assert_eq!(e.code(), "manifest_invalid", "case {i}"),
            Ok(m) => panic!("case {i} accepted: {m:?}"),
        }
    }

    // A stored manifest whose chunk is longer than it lists (11 bytes listed as 10).
    let t = TempDir::new("req9");
    let store = t.store();
    let short = store.put_bytes(b"short chunk", PutOpts::default()).unwrap();
    let chunks = vec![short.digest().as_str().to_string(), short.digest().as_str().to_string()];
    let text = manifest_text(20, 10, &chunks);
    let m = store.put_bytes(&text, PutOpts::default()).unwrap();
    let obj = Address::Object(ObjectRef { manifest_digest: m.digest(), size: 20 });
    assert_eq!(store.get(&obj).unwrap_err().code(), "manifest_invalid");
    // The reference's size must agree with the manifest.
    let wrong = Address::Object(ObjectRef { manifest_digest: m.digest(), size: 21 });
    assert_eq!(store.stream(&wrong, &mut Vec::new()).unwrap_err().code(), "manifest_invalid");
}

// ---------------------------------------------------------------- other requirements

/// REQ-001: the empty blob, the exact 8 MiB boundary and `sha256:` + lowercase hex.
#[test]
fn req1_addresses_and_boundaries() {
    let t = TempDir::new("req1");
    let store = t.store();
    let empty = store.put_bytes(b"", PutOpts { durable: true }).unwrap();
    assert_eq!(empty.digest().as_str(), "sha256:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
    assert!(matches!(empty, Address::Blob(_)));
    assert_eq!(store.get(&empty).unwrap(), b"");
    let abc = store.put_bytes(b"abc", PutOpts::default()).unwrap();
    assert_eq!(abc.digest().as_str(), "sha256:ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    assert!(matches!(store.put(Stream::new(1, CHUNK_SIZE), PutOpts::default()).unwrap(), Address::Blob(_)));
    assert!(matches!(store.put(Stream::new(1, CHUNK_SIZE + 1), PutOpts::default()).unwrap(), Address::Object(_)));
}

/// Errors: a missing address is `not_found`; ranges past the end are refused.
#[test]
fn not_found_and_ranges() {
    let t = TempDir::new("nf");
    let store = t.store();
    let missing = Digest::from_bytes([7; 32]);
    let blob = Address::Blob(fid_store::BlobRef { digest: missing, size: 1 });
    match store.get(&blob) {
        Err(CasError::NotFound(d)) => assert_eq!(d, missing),
        other => panic!("expected not_found, got {other:?}"),
    }
    assert!(!store.has(&blob).unwrap());
    let obj = Address::Object(ObjectRef { manifest_digest: missing, size: 1 });
    assert_eq!(store.stream(&obj, &mut Vec::new()).unwrap_err().code(), "not_found");
    assert!(!store.has(&obj).unwrap());
    let a = store.put_bytes(b"0123456789", PutOpts::default()).unwrap();
    assert_eq!(store.read_range(&a, 2..5).unwrap(), b"234");
    assert_eq!(store.read_range(&a, 5..11).unwrap_err().code(), "out_of_range");
    // An object with a missing chunk is not "had".
    let o = store.put(Stream::new(9, CHUNK_SIZE + 10), PutOpts::default()).unwrap();
    let Address::Object(oref) = o else { panic!("object") };
    std::fs::remove_file(store.path_of(&store.manifest(&oref).unwrap().chunks[1])).unwrap();
    assert!(!store.has(&o).unwrap());
    assert_eq!(store.get(&o).unwrap_err().code(), "not_found");
}

/// REQ-008: stray files in `tmp/` (a crash mid-write) never appear under an address, and a
/// write after them still succeeds.
#[test]
fn req8_temp_files_never_become_blobs() {
    let t = TempDir::new("req8");
    let store = t.store();
    std::fs::write(store.root().join("tmp").join("00deadbeef"), b"partial").unwrap();
    let a = store.put_bytes(b"partial write", PutOpts { durable: true }).unwrap();
    assert_eq!(files_under(&store.root().join("sha256")), vec![store.path_of(&a.digest())]);
    assert_eq!(store.get(&a).unwrap(), b"partial write");
}

/// The store opens on a project's `blobs/` directory from the layout module.
#[test]
fn for_project_uses_layout_blob_dir() {
    let t = TempDir::new("prj");
    let root = t.0.join("p");
    let (loc, lock) = fid_store::ProjectLocation::create(&root).unwrap();
    let store = BlobStore::for_project(&loc).unwrap();
    assert_eq!(store.root(), std::path::absolute(root.join("blobs")).unwrap());
    let a = store.put_bytes(b"in a project", PutOpts { durable: true }).unwrap();
    assert!(store.has(&a).unwrap());
    drop(lock);
}
