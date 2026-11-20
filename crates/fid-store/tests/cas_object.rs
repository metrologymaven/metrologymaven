//! `BlobStore::read_object` (CORE.STO.CAS REQ-010): a digest gives the content it names,
//! never a chunk manifest, bounded by `max_len` and fully verified.
//!
//! Expected digests come from `fixtures/core/sto/cas/make_fixtures.py` (Python hashlib),
//! never from this crate. Content is the SplitMix64 byte stream described there.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::CValue;
use fid_store::cas::CHUNK_SIZE;
use fid_store::{Address, BlobRef, BlobStore, CasError, ChunkManifest, PutOpts};
use fid_types::Digest;
use sha2::{Digest as _, Sha256};
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/core/sto/cas");
const MIB: u64 = 1024 * 1024;
const GAMMA: u64 = 0x9E37_79B9_7F4A_7C15;

struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> TempDir {
        static N: AtomicU32 = AtomicU32::new(0);
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().subsec_nanos();
        let name = format!("fid-casobj-{tag}-{}-{nanos}-{}", std::process::id(), N.fetch_add(1, Ordering::SeqCst));
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

fn mix(mut z: u64) -> u64 {
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// The SplitMix64 byte stream of `seed`, cut to `size` bytes.
struct Stream {
    seed: u64,
    pos: u64,
    size: u64,
}

impl Read for Stream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let n = buf.len().min((self.size - self.pos) as usize);
        let mut i = 0;
        while i < n {
            let p = self.pos + i as u64;
            let w = mix(self.seed.wrapping_add((p / 8).wrapping_add(1).wrapping_mul(GAMMA))).to_le_bytes();
            let off = (p % 8) as usize;
            let take = (8 - off).min(n - i);
            buf[i..i + take].copy_from_slice(&w[off..off + take]);
            i += take;
        }
        self.pos += n as u64;
        Ok(n)
    }
}

fn stream(seed: u64, size: u64) -> Stream {
    Stream { seed, pos: 0, size }
}

fn sha(bytes: &[u8]) -> String {
    Digest::from_bytes(Sha256::digest(bytes).into()).as_str().to_string()
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

struct Obj {
    address: Digest,
    content: String,
    manifest_bytes: u64,
    seed: u64,
    size: u64,
}

fn objects() -> Vec<Obj> {
    let raw = std::fs::read(Path::new(FIXTURES).join("objects.json")).unwrap();
    let doc = fid_canon::parse(&raw).unwrap();
    let CValue::Arr(list) = field(&doc, "objects") else { panic!("objects") };
    list.iter()
        .map(|o| Obj {
            address: Digest::parse(&text(field(o, "address"))).unwrap(),
            content: text(field(o, "content")),
            manifest_bytes: number(field(o, "manifest_bytes")),
            seed: text(field(o, "seed")).parse().unwrap(),
            size: number(field(o, "size")),
        })
        .collect()
}

fn flip_byte(path: &Path, at: usize) {
    let mut b = std::fs::read(path).unwrap();
    b[at] ^= 0x01;
    std::fs::write(path, b).unwrap();
}

/// Writes `bytes` straight to the path of their digest, bypassing `put`.
fn plant(store: &BlobStore, bytes: &[u8]) -> Digest {
    let digest = Digest::from_bytes(Sha256::digest(bytes).into());
    let path = store.path_of(&digest);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, bytes).unwrap();
    digest
}

fn manifest_text(size: u64, chunk_size: u64, chunks: &[String]) -> Vec<u8> {
    let listed: Vec<String> = chunks.iter().map(|c| format!("\"{c}\"")).collect();
    format!(
        "{{\"chunk_size\":\"{chunk_size}\",\"chunks\":[{}],\"format\":\"fid-chunks@1\",\"size\":\"{size}\"}}",
        listed.join(",")
    )
    .into_bytes()
}

fn code<T: std::fmt::Debug>(r: Result<T, CasError>) -> &'static str {
    r.unwrap_err().code()
}

/// Large chunked objects round-trip through their name, and the name never gives the
/// manifest: every chunked object of the fixture (8 MiB + 1 to 20 MiB), checked against the
/// independent whole-content SHA-256.
#[test]
fn chunked_objects_round_trip_by_name_and_never_give_the_manifest() {
    let objs = objects();
    assert_eq!(objs.len(), 4);
    assert!(objs.iter().any(|o| o.size == 20 * MIB));
    let t = TempDir::new("roundtrip");
    let store = t.store();
    for o in &objs {
        let addr = store.put(stream(o.seed, o.size), PutOpts::default()).unwrap();
        assert_eq!(addr.digest(), o.address, "the fixture's name");
        let bytes = store.read_object(&o.address, o.size).unwrap();
        assert_eq!(bytes.len() as u64, o.size);
        assert_eq!(sha(&bytes), o.content, "content of {}", o.address);
        // The low-level read of the same digest is the manifest; read_object never is.
        let low = store.get(&Address::Blob(BlobRef { digest: o.address, size: o.manifest_bytes })).unwrap();
        assert!(ChunkManifest::parse(&low).is_ok());
        assert_ne!(bytes, low);
        assert!(ChunkManifest::parse(&bytes).is_err());
    }
}

/// A single blob reads back as itself, bounded by `max_len`.
#[test]
fn single_blob_round_trip_and_bound() {
    let t = TempDir::new("blob");
    let store = t.store();
    let data: Vec<u8> = {
        let mut v = Vec::new();
        stream(42, MIB + 3).read_to_end(&mut v).unwrap();
        v
    };
    let d = store.put_bytes(&data, PutOpts::default()).unwrap().digest();
    assert_eq!(store.read_object(&d, MIB + 3).unwrap(), data);
    assert_eq!(store.read_object(&d, u64::MAX).unwrap(), data);
    match store.read_object(&d, MIB + 2) {
        Err(CasError::OutOfRange { start: 0, end, size }) => assert_eq!((end, size), (MIB + 3, MIB + 2)),
        other => panic!("{other:?}"),
    }
    let empty = store.put_bytes(b"", PutOpts::default()).unwrap().digest();
    assert_eq!(store.read_object(&empty, 0).unwrap(), Vec::<u8>::new());
    assert_eq!(code(store.read_object(&Digest::from_bytes([7; 32]), 10)), "not_found");
}

/// A tampered chunk is refused with `integrity` and no bytes; a missing chunk is `not_found`;
/// a tampered single blob is `integrity`.
#[test]
fn tampered_or_missing_chunks_are_refused() {
    let t = TempDir::new("tamper");
    let store = t.store();
    let size = 20 * MIB;
    let Address::Object(o) = store.put(stream(9, size), PutOpts::default()).unwrap() else { panic!("object") };
    let m = store.manifest(&o).unwrap();
    flip_byte(&store.path_of(&m.chunks[1]), 12345);
    assert_eq!(code(store.read_object(&o.manifest_digest, size)), "integrity");
    std::fs::remove_file(store.path_of(&m.chunks[2])).unwrap();
    let t2 = TempDir::new("missing");
    let s2 = t2.store();
    let Address::Object(o2) = s2.put(stream(9, size), PutOpts::default()).unwrap() else { panic!("object") };
    let m2 = s2.manifest(&o2).unwrap();
    std::fs::remove_file(s2.path_of(&m2.chunks[2])).unwrap();
    assert_eq!(code(s2.read_object(&o2.manifest_digest, size)), "not_found");
    // A tampered manifest no longer hashes to the name.
    flip_byte(&s2.path_of(&o2.manifest_digest), 20);
    assert_eq!(code(s2.read_object(&o2.manifest_digest, size)), "integrity");
    let b = store.put_bytes(b"plain bytes", PutOpts::default()).unwrap().digest();
    flip_byte(&store.path_of(&b), 0);
    assert_eq!(code(store.read_object(&b, 100)), "integrity");
}

/// `max_len` is checked against the manifest's size before the object's buffer is allocated
/// or any chunk is read: the probe sees no chunk and no more than the manifest buffered.
#[test]
fn max_len_is_enforced_before_allocation() {
    let t = TempDir::new("maxlen");
    let store = t.store();
    let size = 20 * MIB;
    let addr = store.put(stream(5, size), PutOpts::default()).unwrap();
    store.reset_probe();
    for max in [0, 1000, size - 1] {
        match store.read_object(&addr.digest(), max) {
            Err(CasError::OutOfRange { start: 0, end, size: limit }) => assert_eq!((end, limit), (size, max)),
            other => panic!("{max}: {other:?}"),
        }
    }
    let p = store.probe();
    assert_eq!(p.chunks_verified, 0, "no chunk is read for an object over max_len");
    assert!(p.peak_buffer_bytes < 64 * 1024, "only the manifest was buffered: {}", p.peak_buffer_bytes);
    assert_eq!(store.read_object(&addr.digest(), size).unwrap().len() as u64, size);
    assert_eq!(store.probe().chunks_verified, 3);
}

/// A manifest is never returned as content: a planted manifest whose chunks are absent is
/// refused, one whose chunk has the wrong length is `manifest_invalid`, and one whose chunks
/// are present is read as the object it names. A plain blob that merely starts like a
/// manifest is returned as itself.
#[test]
fn manifests_are_never_returned() {
    let t = TempDir::new("planted");
    let store = t.store();
    let missing =
        manifest_text(2 * CHUNK_SIZE, CHUNK_SIZE, &[format!("sha256:{:064x}", 1), format!("sha256:{:064x}", 2)]);
    let d = plant(&store, &missing);
    assert!(ChunkManifest::parse(&missing).is_ok());
    assert_eq!(code(store.read_object(&d, u64::MAX)), "not_found");
    // One real 10-byte chunk, listed with chunk_size 10: the object is those 10 bytes.
    let chunk = store.put_bytes(b"0123456789", PutOpts::default()).unwrap().digest();
    let small = manifest_text(10, 10, &[chunk.as_str().to_string()]);
    let ds = plant(&store, &small);
    assert_eq!(store.read_object(&ds, 10).unwrap(), b"0123456789");
    assert_eq!(code(store.read_object(&ds, 9)), "out_of_range");
    // The chunk is listed as 4 bytes long: the object cannot be verified.
    let wrong = manifest_text(4, 4, &[chunk.as_str().to_string()]);
    let dw = plant(&store, &wrong);
    assert_eq!(code(store.read_object(&dw, 100)), "manifest_invalid");
    // Starts like a manifest, is not one: plain data.
    let lookalike = b"{\"chunk_size\":\"not a manifest\"}".to_vec();
    let dl = store.put_bytes(&lookalike, PutOpts::default()).unwrap().digest();
    assert_eq!(store.read_object(&dl, 100).unwrap(), lookalike);
    assert_eq!(code(store.read_object(&dl, 5)), "out_of_range");
}
