//! Content-addressed blob store with chunking and integrity checks (spec node `CORE.STO.CAS`).
//!
//! Every blob is addressed by the SHA-256 of its content (`sha256:` plus 64 lowercase hex
//! digits) and stored once, at `<blob dir>/sha256/<h0h1>/<h2h3>/<hex>`. An object larger
//! than [`CHUNK_SIZE`] is cut into chunks of that size (the last may be shorter), each chunk
//! is stored as a blob, and the object is named by the SHA-256 of its canonical
//! `fid-chunks@1` [`ChunkManifest`] (RFC 8785 through `fid-canon`), itself stored as a blob.
//!
//! Writes go to `<blob dir>/tmp/<random hex>` and are renamed into place, so a crash never
//! leaves a partial file under an address. With [`PutOpts::durable`] the file is flushed
//! before the rename and its directory after it, with `File::sync_all` (Linux `fsync`; macOS
//! `F_FULLFSYNC`; Windows `FlushFileBuffers`).
//!
//! Every read is checked against its address: a mismatch is the typed [`CasError::Integrity`]
//! and no bytes of that blob are returned. Streaming and range reads work one chunk at a time,
//! so their buffer memory is one chunk plus the chunk list (71 bytes per chunk), whatever the
//! object size; [`BlobStore::probe`] reports the peak.
//!
//! **Reading by digest.** A chunked object's name is the digest of its manifest, and the
//! manifest is stored as an ordinary blob under that digest. A consumer that holds only a
//! digest must read it with [`BlobStore::read_object`], which returns the object's own bytes
//! (one blob, or the chunks reassembled and verified against the manifest) and never the
//! manifest. [`BlobStore::get`] with [`Address::Blob`] is the low-level read of one stored
//! blob: given a manifest digest it returns the manifest bytes.

use crate::layout::{ProjectLocation, BLOBS_DIR};
use fid_canon::CValue;
use fid_types::Digest;
use sha2::{Digest as _, Sha256};
use std::collections::BTreeMap;
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// Chunk size, and the largest object stored as a single blob (REQ-CORE.STO.CAS-003).
pub const CHUNK_SIZE: u64 = 8 * 1024 * 1024;
/// Format tag of chunk manifests.
pub const MANIFEST_FORMAT: &str = "fid-chunks@1";
/// Largest accepted manifest, in bytes (REQ-CORE.STO.CAS-009).
pub const MAX_MANIFEST_BYTES: u64 = 16 * 1024 * 1024;
/// Most chunks a manifest may list (REQ-CORE.STO.CAS-009).
pub const MAX_CHUNKS: usize = 1 << 20;
/// Most chunks of an object this store writes or reads: 2^15 chunks, 256 GiB. Caps the chunk
/// list (71 B per chunk) and the manifest (about 74 B per chunk) at about 2.3 MiB each, so the
/// buffer bound of REQ-CORE.STO.CAS-005 (two chunks plus 1 MiB) holds for every object it
/// accepts while covering the 50 GB of REQ-CORE.STO.CAS-004. Manifests up to [`MAX_CHUNKS`]
/// still parse ([`ChunkManifest::parse`]); the store refuses them as `manifest_invalid`.
pub const MAX_OBJECT_CHUNKS: usize = 1 << 15;
/// Largest manifest file the store reads for an object (bounds [`MAX_OBJECT_CHUNKS`]).
const MAX_OBJECT_MANIFEST_BYTES: u64 = MAX_OBJECT_CHUNKS as u64 * 80 + 1024;
/// Directory below the blob directory that holds the addressed blobs.
pub const SHA256_DIR: &str = "sha256";
/// Directory below the blob directory that holds files being written.
pub const TMP_DIR: &str = "tmp";

/// Largest blob [`BlobStore::get`] returns in memory: a chunk or a manifest.
const MAX_BLOB_READ: u64 = MAX_MANIFEST_BYTES;
/// Bytes of one digest in the probe's accounting of chunk lists.
const DIGEST_COST: u64 = 71;

/// A single stored blob.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlobRef {
    pub digest: Digest,
    pub size: u64,
}

/// A chunked object, named by the digest of its manifest.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ObjectRef {
    pub manifest_digest: Digest,
    pub size: u64,
}

/// What `put` returns: a blob for content up to [`CHUNK_SIZE`], an object above it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Address {
    Blob(BlobRef),
    Object(ObjectRef),
}

impl Address {
    /// The name: the blob's digest, or the object's manifest digest.
    pub fn digest(&self) -> Digest {
        match self {
            Address::Blob(b) => b.digest,
            Address::Object(o) => o.manifest_digest,
        }
    }

    /// Content size in bytes.
    pub fn size(&self) -> u64 {
        match self {
            Address::Blob(b) => b.size,
            Address::Object(o) => o.size,
        }
    }
}

/// Options of [`BlobStore::put`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PutOpts {
    /// Return only after file and directory entry are on stable storage (REQ-CORE.STO.CAS-006).
    pub durable: bool,
}

/// What [`BlobStore::check_stored`] found under a digest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Stored {
    /// Nothing is stored under this digest (the blob itself, or a chunk its manifest lists).
    Missing(Digest),
    /// This blob or chunk does not hash to its address (or has the wrong length).
    Corrupt(Digest),
    /// Sound; these blobs (the blob, then its chunks) are to be flushed.
    Sound(Vec<Digest>),
}

/// Typed errors with stable codes.
#[derive(Debug)]
pub enum CasError {
    /// The bytes stored under this address do not hash to it.
    Integrity(Digest),
    /// Nothing is stored under this address.
    NotFound(Digest),
    /// A chunk manifest is malformed, too large or disagrees with its chunks.
    ManifestInvalid(String),
    /// A range read past the end of the content.
    OutOfRange {
        start: u64,
        end: u64,
        size: u64,
    },
    Io(io::Error),
}

impl CasError {
    /// Stable snake_case code.
    pub fn code(&self) -> &'static str {
        match self {
            CasError::Integrity(_) => "integrity",
            CasError::NotFound(_) => "not_found",
            CasError::ManifestInvalid(_) => "manifest_invalid",
            CasError::OutOfRange { .. } => "out_of_range",
            CasError::Io(_) => "io",
        }
    }

    /// The OS error kind of an `io` error.
    pub fn io_kind(&self) -> Option<io::ErrorKind> {
        match self {
            CasError::Io(e) => Some(e.kind()),
            _ => None,
        }
    }
}

impl fmt::Display for CasError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CasError::Integrity(d) => write!(f, "integrity: content stored under {d} does not match it"),
            CasError::NotFound(d) => write!(f, "not_found: {d}"),
            CasError::ManifestInvalid(why) => write!(f, "manifest_invalid: {why}"),
            CasError::OutOfRange { start, end, size } => {
                write!(f, "out_of_range: {start}..{end} of {size} bytes")
            }
            CasError::Io(e) => write!(f, "io: {e}"),
        }
    }
}

impl std::error::Error for CasError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            CasError::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<io::Error> for CasError {
    fn from(e: io::Error) -> Self {
        CasError::Io(e)
    }
}

fn invalid(why: impl Into<String>) -> CasError {
    CasError::ManifestInvalid(why.into())
}

// ------------------------------------------------------------------ manifest

/// The `fid-chunks@1` manifest of a chunked object.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChunkManifest {
    pub size: u64,
    pub chunk_size: u64,
    pub chunks: Vec<Digest>,
}

impl ChunkManifest {
    /// RFC 8785 canonical bytes; sizes are decimal strings (project rule of `fid-canon`).
    pub fn to_canonical_bytes(&self) -> Result<Vec<u8>, CasError> {
        let mut obj = BTreeMap::new();
        obj.insert("chunk_size".to_string(), CValue::U64(self.chunk_size));
        obj.insert(
            "chunks".to_string(),
            CValue::Arr(self.chunks.iter().map(|d| CValue::Str(d.as_str().to_string())).collect()),
        );
        obj.insert("format".to_string(), CValue::Str(MANIFEST_FORMAT.to_string()));
        obj.insert("size".to_string(), CValue::U64(self.size));
        fid_canon::to_canonical_bytes(&CValue::Obj(obj)).map_err(|e| invalid(format!("canonical form: {e}")))
    }

    /// Parses and validates a manifest with the limits of REQ-CORE.STO.CAS-009.
    pub fn parse(bytes: &[u8]) -> Result<ChunkManifest, CasError> {
        ChunkManifest::parse_with_limits(bytes, MAX_MANIFEST_BYTES, MAX_CHUNKS)
    }

    /// [`ChunkManifest::parse`] with caller-chosen limits (never above the defaults). The
    /// bytes must be the canonical form, the chunk size 1..=[`CHUNK_SIZE`], and the number of
    /// chunks must be exactly the one `size` and `chunk_size` imply.
    pub fn parse_with_limits(bytes: &[u8], max_bytes: u64, max_chunks: usize) -> Result<ChunkManifest, CasError> {
        let max_bytes = max_bytes.min(MAX_MANIFEST_BYTES);
        let max_chunks = max_chunks.min(MAX_CHUNKS);
        if bytes.len() as u64 > max_bytes {
            return Err(invalid(format!("larger than {max_bytes} bytes")));
        }
        let limit = usize::try_from(max_bytes).unwrap_or(usize::MAX);
        let value = fid_canon::parse_with_limits(bytes, limit, 4).map_err(|e| invalid(format!("json: {e}")))?;
        let CValue::Obj(obj) = &value else { return Err(invalid("not an object")) };
        if obj.len() != 4 {
            return Err(invalid("expected exactly format, size, chunk_size and chunks"));
        }
        match obj.get("format") {
            Some(CValue::Str(f)) if f == MANIFEST_FORMAT => {}
            _ => return Err(invalid("format is not fid-chunks@1")),
        }
        let size = decimal_field(obj, "size")?;
        let chunk_size = decimal_field(obj, "chunk_size")?;
        if chunk_size == 0 || chunk_size > CHUNK_SIZE {
            return Err(invalid(format!("chunk_size {chunk_size} outside 1..={CHUNK_SIZE}")));
        }
        let Some(CValue::Arr(list)) = obj.get("chunks") else { return Err(invalid("chunks is not an array")) };
        if list.len() > max_chunks {
            return Err(invalid(format!("{} chunks, limit {max_chunks}", list.len())));
        }
        let mut chunks = Vec::with_capacity(list.len());
        for item in list {
            let CValue::Str(s) = item else { return Err(invalid("chunk entry is not a string")) };
            chunks.push(Digest::parse(s).map_err(|_| invalid("chunk entry is not a sha256 digest"))?);
        }
        let expected = size.div_ceil(chunk_size);
        if size == 0 || chunks.len() as u64 != expected {
            return Err(invalid(format!("size {size} with chunk_size {chunk_size} needs {expected} chunks")));
        }
        let canonical = fid_canon::to_canonical_bytes(&value).map_err(|e| invalid(format!("canonical form: {e}")))?;
        if canonical != bytes {
            return Err(invalid("not in canonical form"));
        }
        Ok(ChunkManifest { size, chunk_size, chunks })
    }

    /// Length of chunk `i`.
    pub fn chunk_len(&self, i: usize) -> u64 {
        let start = (i as u64).saturating_mul(self.chunk_size);
        self.size.saturating_sub(start).min(self.chunk_size)
    }
}

/// The address [`BlobStore::put`] gives `bytes`, computed without writing anything: one blob
/// up to [`CHUNK_SIZE`] bytes, else the digest of the chunk manifest.
pub fn address_of(bytes: &[u8]) -> Result<Address, CasError> {
    let size = bytes.len() as u64;
    if size <= CHUNK_SIZE {
        return Ok(Address::Blob(BlobRef { digest: Digest::from_bytes(Sha256::digest(bytes).into()), size }));
    }
    let chunks: Vec<Digest> =
        bytes.chunks(CHUNK_SIZE as usize).map(|c| Digest::from_bytes(Sha256::digest(c).into())).collect();
    if chunks.len() > MAX_OBJECT_CHUNKS {
        return Err(invalid(format!("object needs more than {MAX_OBJECT_CHUNKS} chunks")));
    }
    let manifest = ChunkManifest { size, chunk_size: CHUNK_SIZE, chunks };
    let m = manifest.to_canonical_bytes()?;
    Ok(Address::Object(ObjectRef { manifest_digest: Digest::from_bytes(Sha256::digest(&m).into()), size }))
}

fn decimal_field(obj: &BTreeMap<String, CValue>, key: &str) -> Result<u64, CasError> {
    let Some(CValue::Str(s)) = obj.get(key) else { return Err(invalid(format!("{key} is not a decimal string"))) };
    let digits = !s.is_empty() && s.len() <= 20 && s.bytes().all(|b| b.is_ascii_digit());
    let ok = digits && (s == "0" || !s.starts_with('0'));
    if !ok {
        return Err(invalid(format!("{key} is not a decimal string")));
    }
    s.parse().map_err(|_| invalid(format!("{key} out of range")))
}

// ------------------------------------------------------------------ memory probe

/// Buffer accounting of a store: heap buffers held for data (chunk buffers, manifests,
/// chunk lists) and the chunks read and verified.
#[derive(Debug, Default)]
struct Counters {
    current: AtomicU64,
    peak: AtomicU64,
    chunks_verified: AtomicU64,
}

/// A snapshot of [`BlobStore::probe`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Probe {
    /// Largest number of buffer bytes held at once since the last reset.
    pub peak_buffer_bytes: u64,
    /// Chunks of chunked objects read and verified since the last reset.
    pub chunks_verified: u64,
}

/// A charge against the counters, released on drop.
struct Charge<'a> {
    counters: &'a Counters,
    bytes: u64,
}

impl<'a> Charge<'a> {
    fn new(counters: &'a Counters, bytes: u64) -> Charge<'a> {
        let now = counters.current.fetch_add(bytes, Ordering::SeqCst).saturating_add(bytes);
        counters.peak.fetch_max(now, Ordering::SeqCst);
        Charge { counters, bytes }
    }
}

impl Charge<'_> {
    /// Changes the charged amount to `bytes`.
    fn set(&mut self, bytes: u64) {
        if bytes > self.bytes {
            let add = bytes - self.bytes;
            let now = self.counters.current.fetch_add(add, Ordering::SeqCst).saturating_add(add);
            self.counters.peak.fetch_max(now, Ordering::SeqCst);
        } else {
            self.counters.current.fetch_sub(self.bytes - bytes, Ordering::SeqCst);
        }
        self.bytes = bytes;
    }
}

impl Drop for Charge<'_> {
    fn drop(&mut self) {
        self.counters.current.fetch_sub(self.bytes, Ordering::SeqCst);
    }
}

// ------------------------------------------------------------------ store

/// A content-addressed blob store over one blob directory.
#[derive(Debug)]
pub struct BlobStore {
    root: PathBuf,
    counters: Counters,
}

impl BlobStore {
    /// Opens (creating `sha256/` and `tmp/` if missing) the store in `blob_dir`.
    pub fn open(blob_dir: &Path) -> Result<BlobStore, CasError> {
        let root = std::path::absolute(blob_dir)?;
        refuse_link(&root, true)?;
        for d in [SHA256_DIR, TMP_DIR] {
            make_dir(&root.join(d))?;
        }
        Ok(BlobStore { root, counters: Counters::default() })
    }

    /// The store in the `blobs/` directory of a project (`CORE.STO.LAYOUT`).
    pub fn for_project(project: &ProjectLocation) -> Result<BlobStore, CasError> {
        BlobStore::open(&project.root().join(BLOBS_DIR))
    }

    /// The blob directory.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Where the blob with this digest lives: `sha256/<h0h1>/<h2h3>/<hex>`.
    pub fn path_of(&self, digest: &Digest) -> PathBuf {
        let hex = digest.hex();
        self.root.join(SHA256_DIR).join(&hex[0..2]).join(&hex[2..4]).join(hex)
    }

    /// Buffer and verification counters since the last [`BlobStore::reset_probe`].
    pub fn probe(&self) -> Probe {
        Probe {
            peak_buffer_bytes: self.counters.peak.load(Ordering::SeqCst),
            chunks_verified: self.counters.chunks_verified.load(Ordering::SeqCst),
        }
    }

    pub fn reset_probe(&self) {
        self.counters.peak.store(self.counters.current.load(Ordering::SeqCst), Ordering::SeqCst);
        self.counters.chunks_verified.store(0, Ordering::SeqCst);
    }

    /// Stores everything `reader` yields. Content up to [`CHUNK_SIZE`] becomes one blob;
    /// longer content becomes chunks plus a manifest. Holds one chunk in memory at a time.
    pub fn put(&self, mut reader: impl Read, opts: PutOpts) -> Result<Address, CasError> {
        let cs = CHUNK_SIZE as usize;
        let charge = Charge::new(&self.counters, CHUNK_SIZE);
        let mut buf = vec![0u8; cs];
        let mut filled = fill(&mut reader, &mut buf)?;
        // The first byte of the next chunk, read ahead to tell "exactly full" from "more".
        let mut next = if filled == cs { read_byte(&mut reader)? } else { None };
        if next.is_none() {
            let blob = self.write_blob(&buf[..filled], opts)?;
            return Ok(Address::Blob(blob));
        }
        let mut chunks: Vec<Digest> = Vec::new();
        let mut list_charge = Charge::new(&self.counters, 0);
        let mut size: u64 = 0;
        loop {
            if chunks.len() >= MAX_OBJECT_CHUNKS {
                return Err(invalid(format!("object needs more than {MAX_OBJECT_CHUNKS} chunks")));
            }
            let blob = self.write_blob(&buf[..filled], opts)?;
            chunks.push(blob.digest);
            list_charge.set(chunks.capacity() as u64 * DIGEST_COST);
            size = size.saturating_add(blob.size);
            let Some(b) = next else { break };
            buf[0] = b;
            filled = 1 + fill(&mut reader, &mut buf[1..])?;
            next = if filled == cs { read_byte(&mut reader)? } else { None };
        }
        drop(buf);
        drop(charge);
        let manifest = ChunkManifest { size, chunk_size: CHUNK_SIZE, chunks };
        let bytes = manifest.to_canonical_bytes()?;
        if bytes.len() as u64 > MAX_MANIFEST_BYTES {
            return Err(invalid(format!("manifest of {} bytes exceeds {MAX_MANIFEST_BYTES}", bytes.len())));
        }
        let _manifest_charge = Charge::new(&self.counters, bytes.len() as u64);
        let m = self.write_blob(&bytes, opts)?;
        drop(list_charge);
        Ok(Address::Object(ObjectRef { manifest_digest: m.digest, size }))
    }

    /// [`BlobStore::put`] of an in-memory slice.
    pub fn put_bytes(&self, bytes: &[u8], opts: PutOpts) -> Result<Address, CasError> {
        self.put(bytes, opts)
    }

    /// True if the blob is present (for an object: its manifest and every chunk). Presence
    /// is not verification; reads verify.
    pub fn has(&self, addr: &Address) -> Result<bool, CasError> {
        match addr {
            Address::Blob(b) => self.has_blob(&b.digest),
            Address::Object(o) => {
                if !self.has_blob(&o.manifest_digest)? {
                    return Ok(false);
                }
                let manifest = self.manifest(o)?;
                for d in &manifest.chunks {
                    if !self.has_blob(d)? {
                        return Ok(false);
                    }
                }
                Ok(true)
            }
        }
    }

    /// True if a regular file is stored under `digest`.
    pub fn has_blob(&self, digest: &Digest) -> Result<bool, CasError> {
        match fs::symlink_metadata(self.path_of(digest)) {
            Ok(m) => Ok(m.is_file()),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(false),
            Err(e) => Err(e.into()),
        }
    }

    /// Checks a blob that is already stored before a commit references it
    /// (`CORE.STO.SQL.TXN`): the file under `digest` must hash to it, and if it is a chunk
    /// manifest, every chunk must be stored with the listed length and hash to its digest.
    /// Reads in small pieces (no whole-blob buffer) and writes nothing. On success returns the
    /// digests to flush with [`BlobStore::flush_stored`]: the blob, then its chunks.
    pub(crate) fn check_stored(&self, digest: &Digest) -> Result<Stored, CasError> {
        let len = match fs::symlink_metadata(self.path_of(digest)) {
            Ok(m) if m.is_file() => m.len(),
            Ok(_) => return Err(not_regular()),
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Stored::Missing(*digest)),
            Err(e) => return Err(e.into()),
        };
        // No blob this store writes is longer than a manifest; a longer file is not the
        // content of its address and is not hashed.
        if len > MAX_BLOB_READ || !self.existing_matches(digest, len, true)? {
            return Ok(Stored::Corrupt(*digest));
        }
        let mut flush = vec![*digest];
        if len > MAX_OBJECT_MANIFEST_BYTES {
            return Ok(Stored::Sound(flush));
        }
        let mut head = [0u8; 14];
        let n = fill(&mut self.open_blob(digest)?, &mut head)?;
        if head.get(..n) != Some(b"{\"chunk_size\":".as_slice()) {
            return Ok(Stored::Sound(flush));
        }
        let bytes = self.read_blob(digest, MAX_OBJECT_MANIFEST_BYTES)?;
        let Ok(manifest) = ChunkManifest::parse_with_limits(&bytes, MAX_OBJECT_MANIFEST_BYTES, MAX_OBJECT_CHUNKS)
        else {
            // A plain blob that merely starts like a manifest.
            return Ok(Stored::Sound(flush));
        };
        for (i, chunk) in manifest.chunks.iter().enumerate() {
            if !self.has_blob(chunk)? {
                return Ok(Stored::Missing(*chunk));
            }
            if !self.existing_matches(chunk, manifest.chunk_len(i), true)? {
                return Ok(Stored::Corrupt(*chunk));
            }
            flush.push(*chunk);
        }
        Ok(Stored::Sound(flush))
    }

    /// `sync_all` on a stored blob file and on its two fan-out directories and `sha256/`.
    pub(crate) fn flush_stored(&self, digest: &Digest) -> Result<(), CasError> {
        let target = self.path_of(digest);
        // Write access: `FlushFileBuffers` refuses a read-only handle on Windows.
        OpenOptions::new().write(true).open(&target)?.sync_all()?;
        let hex = digest.hex();
        let d1 = self.root.join(SHA256_DIR).join(&hex[0..2]);
        sync_dir(&d1.join(&hex[2..4]))?;
        sync_dir(&d1)?;
        sync_dir(&self.root.join(SHA256_DIR))?;
        Ok(())
    }

    /// The whole content, verified. For an object this holds the whole object in memory;
    /// use [`BlobStore::stream`] for bounded memory.
    ///
    /// Low-level: with [`Address::Blob`] this returns the one blob stored under the digest,
    /// whatever it is, so the digest of a chunked object gives its **manifest** bytes. A
    /// caller that holds only a digest and wants the content uses [`BlobStore::read_object`].
    pub fn get(&self, addr: &Address) -> Result<Vec<u8>, CasError> {
        match addr {
            Address::Blob(b) => {
                let bytes = self.read_blob(&b.digest, MAX_BLOB_READ)?;
                if bytes.len() as u64 != b.size {
                    return Err(CasError::Io(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        format!("blob {} has {} bytes, the reference says {}", b.digest, bytes.len(), b.size),
                    )));
                }
                Ok(bytes)
            }
            Address::Object(_) => {
                let mut out = Vec::new();
                self.stream(addr, &mut out)?;
                Ok(out)
            }
        }
    }

    /// Writes the content to `out` one verified chunk at a time and returns its length. A
    /// chunk that fails verification is never written; chunks before it already were.
    pub fn stream(&self, addr: &Address, out: &mut impl Write) -> Result<u64, CasError> {
        match addr {
            Address::Blob(_) => {
                let bytes = self.get(addr)?;
                out.write_all(&bytes)?;
                Ok(bytes.len() as u64)
            }
            Address::Object(o) => {
                let manifest = self.manifest(o)?;
                let _list_charge = Charge::new(&self.counters, manifest.chunks.len() as u64 * DIGEST_COST);
                let _charge = Charge::new(&self.counters, manifest.chunk_size);
                let mut buf = vec![0u8; usize::try_from(manifest.chunk_size).unwrap_or(0)];
                for (i, d) in manifest.chunks.iter().enumerate() {
                    let n = self.read_chunk(d, manifest.chunk_len(i), &mut buf)?;
                    out.write_all(&buf[..n])?;
                }
                Ok(manifest.size)
            }
        }
    }

    /// Bytes `range` of the content. For an object only the chunks overlapping the range are
    /// read and verified (REQ-CORE.STO.CAS-007).
    pub fn read_range(&self, addr: &Address, range: Range<u64>) -> Result<Vec<u8>, CasError> {
        let (start, end) = (range.start, range.end);
        let size = addr.size();
        if start > end || end > size {
            return Err(CasError::OutOfRange { start, end, size });
        }
        match addr {
            Address::Blob(_) => {
                let bytes = self.get(addr)?;
                let (s, e) = (usize::try_from(start).unwrap_or(0), usize::try_from(end).unwrap_or(0));
                Ok(bytes.get(s..e).map(<[u8]>::to_vec).unwrap_or_default())
            }
            Address::Object(o) => {
                let mut out = Vec::new();
                if start == end {
                    return Ok(out);
                }
                let manifest = self.manifest(o)?;
                let cs = manifest.chunk_size;
                let _list_charge = Charge::new(&self.counters, manifest.chunks.len() as u64 * DIGEST_COST);
                let _charge = Charge::new(&self.counters, cs);
                let mut buf = vec![0u8; usize::try_from(cs).unwrap_or(0)];
                let first = start / cs;
                let last = (end - 1) / cs;
                for i in first..=last {
                    let idx = usize::try_from(i).unwrap_or(usize::MAX);
                    let d = manifest.chunks.get(idx).ok_or_else(|| invalid("range beyond chunk list"))?;
                    let n = self.read_chunk(d, manifest.chunk_len(idx), &mut buf)? as u64;
                    let chunk_start = i * cs;
                    let lo = start.max(chunk_start) - chunk_start;
                    let hi = end.min(chunk_start + n) - chunk_start;
                    let piece =
                        buf.get(lo as usize..hi as usize).ok_or_else(|| invalid("chunk shorter than listed"))?;
                    out.extend_from_slice(piece);
                }
                Ok(out)
            }
        }
    }

    /// The verified, validated manifest of an object.
    pub fn manifest(&self, o: &ObjectRef) -> Result<ChunkManifest, CasError> {
        let path = self.path_of(&o.manifest_digest);
        let len = match fs::symlink_metadata(&path) {
            Ok(m) if m.is_file() => m.len(),
            Ok(_) => return Err(not_regular()),
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Err(CasError::NotFound(o.manifest_digest)),
            Err(e) => return Err(e.into()),
        };
        // Bounded before anything is read (REQ-CORE.STO.CAS-009, and the object cap that keeps
        // REQ-CORE.STO.CAS-005's memory bound).
        if len > MAX_MANIFEST_BYTES {
            return Err(invalid(format!("manifest of {len} bytes exceeds {MAX_MANIFEST_BYTES}")));
        }
        if len > MAX_OBJECT_MANIFEST_BYTES {
            return Err(invalid(format!("manifest of {len} bytes lists more than {MAX_OBJECT_CHUNKS} chunks")));
        }
        let bytes = self.read_blob(&o.manifest_digest, MAX_MANIFEST_BYTES)?;
        let _charge = Charge::new(&self.counters, bytes.len() as u64);
        let manifest = ChunkManifest::parse_with_limits(&bytes, MAX_OBJECT_MANIFEST_BYTES, MAX_OBJECT_CHUNKS)?;
        if manifest.size != o.size {
            return Err(invalid(format!("manifest size {} but the reference says {}", manifest.size, o.size)));
        }
        Ok(manifest)
    }

    /// The content named by `digest`, verified, and never a chunk manifest
    /// (REQ-CORE.STO.CAS-010).
    ///
    /// If the blob under `digest` is a `fid-chunks@1` manifest (it parses as one within the
    /// object limits), the result is the object it names: every chunk read, checked for its
    /// listed length and its digest, and concatenated. Otherwise the result is the blob
    /// itself, verified. Anything that cannot be fully verified is refused and no bytes are
    /// returned: a blob or chunk that does not hash to its address is `integrity`, a missing
    /// blob or chunk `not_found`, a manifest the store would refuse (or a chunk of the wrong
    /// length) `manifest_invalid`. Content that is itself a valid manifest is always read as
    /// the object it names, so a manifest whose chunks are absent is refused, not returned;
    /// such bytes stored as plain data are reachable only through [`BlobStore::get`].
    ///
    /// `max_len` bounds the result: content longer than `max_len` bytes is refused with
    /// `out_of_range` (`start` 0, `end` the content size, `size` the limit) before its buffer
    /// is allocated or any chunk is read. Only a manifest, at most
    /// `MAX_OBJECT_MANIFEST_BYTES` (about 2.6 MiB), is read before that check. The result is
    /// held in memory; use [`BlobStore::stream`] with a typed [`Address`] for large objects.
    pub fn read_object(&self, digest: &Digest, max_len: u64) -> Result<Vec<u8>, CasError> {
        let len = match fs::symlink_metadata(self.path_of(digest)) {
            Ok(m) if m.is_file() => m.len(),
            Ok(_) => return Err(not_regular()),
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Err(CasError::NotFound(*digest)),
            Err(e) => return Err(e.into()),
        };
        // No blob this store writes is longer than a manifest; a longer file is not the
        // content of its address and is not read.
        if len > MAX_BLOB_READ {
            return Err(CasError::Integrity(*digest));
        }
        let too_long = |size: u64| CasError::OutOfRange { start: 0, end: size, size: max_len };
        let mut head = [0u8; 14];
        let n = fill(&mut self.open_blob(digest)?, &mut head)?;
        let manifest_like = head.get(..n) == Some(b"{\"chunk_size\":".as_slice());
        if !manifest_like || len > MAX_OBJECT_MANIFEST_BYTES {
            if len > max_len {
                return Err(too_long(len));
            }
            let bytes = self.read_blob(digest, len)?;
            // Read once more: a file that grew since `len` was taken is refused by
            // `read_blob`; one that shrank is short here.
            if bytes.len() as u64 != len {
                return Err(CasError::Integrity(*digest));
            }
            if manifest_like && ChunkManifest::parse(&bytes).is_ok() {
                return Err(invalid(format!(
                    "{digest} is a chunk manifest listing more than {MAX_OBJECT_CHUNKS} chunks"
                )));
            }
            return Ok(bytes);
        }
        let bytes = self.read_blob(digest, MAX_OBJECT_MANIFEST_BYTES)?;
        let manifest_charge = Charge::new(&self.counters, bytes.len() as u64);
        let manifest = match ChunkManifest::parse_with_limits(&bytes, MAX_OBJECT_MANIFEST_BYTES, MAX_OBJECT_CHUNKS) {
            Ok(m) => m,
            Err(_) if ChunkManifest::parse(&bytes).is_ok() => {
                return Err(invalid(format!(
                    "{digest} is a chunk manifest listing more than {MAX_OBJECT_CHUNKS} chunks"
                )));
            }
            // A plain blob that merely starts like a manifest.
            Err(_) => {
                if bytes.len() as u64 > max_len {
                    return Err(too_long(bytes.len() as u64));
                }
                return Ok(bytes);
            }
        };
        drop(bytes);
        drop(manifest_charge);
        if manifest.size > max_len {
            return Err(too_long(manifest.size));
        }
        let total = usize::try_from(manifest.size).map_err(|_| too_long(manifest.size))?;
        let mut out = Vec::new();
        out.try_reserve_exact(total)
            .map_err(|_| CasError::Io(io::Error::new(io::ErrorKind::OutOfMemory, "object buffer")))?;
        let _out_charge = Charge::new(&self.counters, manifest.size);
        let _list_charge = Charge::new(&self.counters, manifest.chunks.len() as u64 * DIGEST_COST);
        let _chunk_charge = Charge::new(&self.counters, manifest.chunk_size);
        let mut buf = vec![0u8; usize::try_from(manifest.chunk_size).unwrap_or(0)];
        for (i, d) in manifest.chunks.iter().enumerate() {
            let k = self.read_chunk(d, manifest.chunk_len(i), &mut buf)?;
            out.extend_from_slice(buf.get(..k).ok_or_else(|| invalid("chunk longer than chunk_size"))?);
        }
        if out.len() != total {
            return Err(invalid(format!("{digest}: chunks hold {} bytes, the manifest says {total}", out.len())));
        }
        Ok(out)
    }

    // -------------------------------------------------------------- internals

    /// Reads and verifies one blob of at most `limit` bytes. Low-level: the digest of a
    /// chunked object gives its manifest; see [`BlobStore::read_object`].
    fn read_blob(&self, digest: &Digest, limit: u64) -> Result<Vec<u8>, CasError> {
        let mut f = self.open_blob(digest)?;
        let mut buf = Vec::new();
        Read::by_ref(&mut f).take(limit).read_to_end(&mut buf)?;
        // A file longer than any blob this store writes cannot be the content of its address;
        // it is refused at once, without reading the rest.
        if read_byte(&mut f)?.is_some() {
            return Err(CasError::Integrity(*digest));
        }
        if Digest::from_bytes(Sha256::digest(&buf).into()) != *digest {
            return Err(CasError::Integrity(*digest));
        }
        Ok(buf)
    }

    /// Reads chunk `digest` (expected `len` bytes) into `buf`, verifies it and returns `len`.
    fn read_chunk(&self, digest: &Digest, len: u64, buf: &mut [u8]) -> Result<usize, CasError> {
        let n = usize::try_from(len).unwrap_or(usize::MAX);
        let target = buf.get_mut(..n).ok_or_else(|| invalid("chunk longer than chunk_size"))?;
        let mut f = self.open_blob(digest)?;
        let got = fill(&mut f, target)?;
        self.counters.chunks_verified.fetch_add(1, Ordering::SeqCst);
        // Longer than listed: stop reading at once (a planted oversize file is never hashed).
        if got == n && read_byte(&mut f)?.is_some() {
            return Err(invalid(format!("chunk {digest} is longer than the manifest lists")));
        }
        if Digest::from_bytes(Sha256::digest(&target[..got]).into()) != *digest {
            return Err(CasError::Integrity(*digest));
        }
        if got != n {
            return Err(invalid(format!("chunk {digest} is shorter than the manifest lists")));
        }
        Ok(n)
    }

    /// True if a regular file of `len` bytes is stored under `digest` and, with `verify`, hashes
    /// to it. Reads at most `len + 1` bytes.
    fn existing_matches(&self, digest: &Digest, len: u64, verify: bool) -> Result<bool, CasError> {
        match fs::symlink_metadata(self.path_of(digest)) {
            Ok(m) if m.is_file() && m.len() == len => {}
            Ok(m) if m.is_file() => return Ok(false),
            Ok(_) => return Err(not_regular()),
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(false),
            Err(e) => return Err(e.into()),
        }
        if !verify {
            return Ok(true);
        }
        let mut f = self.open_blob(digest)?;
        let mut hasher = Sha256::new();
        let mut scratch = [0u8; 8192];
        let mut total = 0u64;
        loop {
            let k = read_some(&mut f, &mut scratch)?;
            if k == 0 {
                break;
            }
            total = total.saturating_add(k as u64);
            if total > len {
                return Ok(false);
            }
            hasher.update(&scratch[..k]);
        }
        Ok(total == len && Digest::from_bytes(hasher.finalize().into()) == *digest)
    }

    fn open_blob(&self, digest: &Digest) -> Result<File, CasError> {
        let path = self.path_of(digest);
        match fs::symlink_metadata(&path) {
            Ok(m) if m.is_file() => {}
            Ok(_) => return Err(not_regular()),
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Err(CasError::NotFound(*digest)),
            Err(e) => return Err(e.into()),
        }
        Ok(File::open(&path)?)
    }

    /// Stores one blob (at most a chunk or a manifest) and returns its reference.
    fn write_blob(&self, bytes: &[u8], opts: PutOpts) -> Result<BlobRef, CasError> {
        let digest = Digest::from_bytes(Sha256::digest(bytes).into());
        let blob = BlobRef { digest, size: bytes.len() as u64 };
        let target = self.path_of(&digest);
        let hex = digest.hex();
        let d1 = self.root.join(SHA256_DIR).join(&hex[0..2]);
        let d2 = d1.join(&hex[2..4]);
        // Deduplicated only if the earlier copy is sound: an earlier non-durable put that was
        // cut short by a crash can leave a short or garbled file. Its length is always checked,
        // its content too when durable; anything else is rewritten below by temp file + rename.
        if self.existing_matches(&digest, bytes.len() as u64, opts.durable)? {
            if opts.durable {
                self.flush_stored(&digest)?;
            }
            return Ok(blob);
        }
        make_dir(&d1)?;
        make_dir(&d2)?;
        let tmp = self.root.join(TMP_DIR).join(temp_name());
        if let Err(e) = write_temp(&tmp, bytes, opts.durable) {
            let _ = fs::remove_file(&tmp);
            return Err(e.into());
        }
        if let Err(e) = fs::rename(&tmp, &target) {
            let _ = fs::remove_file(&tmp);
            // A concurrent put of the same content won the race; the address is stored.
            if !self.existing_matches(&digest, bytes.len() as u64, true)? {
                return Err(e.into());
            }
        }
        // Every level, every time: a directory an earlier non-durable put created may not be
        // on stable storage yet.
        if opts.durable {
            sync_dir(&d2)?;
            sync_dir(&d1)?;
            sync_dir(&self.root.join(SHA256_DIR))?;
        }
        Ok(blob)
    }
}

fn not_regular() -> CasError {
    CasError::Io(io::Error::new(io::ErrorKind::InvalidData, "blob path is not a regular file"))
}

/// Refuses a symbolic link at `path` (the store never follows links inside the blob directory).
fn refuse_link(path: &Path, must_be_dir: bool) -> Result<(), CasError> {
    match fs::symlink_metadata(path) {
        Ok(m) if m.file_type().is_symlink() => {
            Err(CasError::Io(io::Error::new(io::ErrorKind::InvalidData, "symbolic link in the blob directory")))
        }
        Ok(m) if must_be_dir && !m.is_dir() => {
            Err(CasError::Io(io::Error::new(io::ErrorKind::InvalidData, "blob directory is not a directory")))
        }
        Ok(_) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::NotFound && must_be_dir => {
            fs::create_dir_all(path)?;
            Ok(())
        }
        Err(e) => Err(e.into()),
    }
}

/// Creates `path` if missing; returns whether it was created. Refuses links.
fn make_dir(path: &Path) -> Result<bool, CasError> {
    match fs::create_dir(path) {
        Ok(()) => Ok(true),
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => match fs::symlink_metadata(path) {
            Ok(m) if m.is_dir() && !m.file_type().is_symlink() => Ok(false),
            Ok(_) => Err(CasError::Io(io::Error::new(
                io::ErrorKind::InvalidData,
                "blob store directory is a link or a file",
            ))),
            Err(e) => Err(e.into()),
        },
        Err(e) => Err(e.into()),
    }
}

/// Reads until `buf` is full or the reader ends; returns the bytes read.
fn fill(r: &mut impl Read, buf: &mut [u8]) -> io::Result<usize> {
    let mut n = 0;
    while n < buf.len() {
        match r.read(&mut buf[n..]) {
            Ok(0) => break,
            Ok(k) => n += k,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    Ok(n)
}

fn write_temp(path: &Path, bytes: &[u8], durable: bool) -> io::Result<()> {
    let mut f = OpenOptions::new().write(true).create_new(true).open(path)?;
    f.write_all(bytes)?;
    if durable {
        f.sync_all()?;
    }
    Ok(())
}

fn read_byte(r: &mut impl Read) -> io::Result<Option<u8>> {
    let mut b = [0u8; 1];
    Ok(if read_some(r, &mut b)? == 0 { None } else { Some(b[0]) })
}

fn read_some(r: &mut impl Read, buf: &mut [u8]) -> io::Result<usize> {
    loop {
        match r.read(buf) {
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            other => return other,
        }
    }
}

/// A fresh random temporary file name (32 lowercase hex digits).
pub(crate) fn temp_name() -> String {
    use std::hash::BuildHasher;
    static N: AtomicU64 = AtomicU64::new(0);
    let n = N.fetch_add(1, Ordering::SeqCst);
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    let pid = std::process::id();
    let a = std::collections::hash_map::RandomState::new().hash_one((nanos, pid, n, 1u8));
    let b = std::collections::hash_map::RandomState::new().hash_one((nanos, pid, n, 2u8, a));
    format!("{a:016x}{b:016x}")
}

/// Flushes a directory's entries to stable storage.
#[cfg(unix)]
fn sync_dir(path: &Path) -> io::Result<()> {
    // `sync_all` is `fsync` on Linux and `fcntl(F_FULLFSYNC)` on macOS.
    File::open(path)?.sync_all()
}

/// Flushes a directory's entries to stable storage. `FlushFileBuffers` needs a handle with
/// write access, opened with backup semantics to reach a directory. File systems that do not
/// support flushing a directory handle refuse with access denied or invalid function; NTFS
/// journals the rename itself, so those refusals are tolerated [unverified for every file system].
#[cfg(windows)]
fn sync_dir(path: &Path) -> io::Result<()> {
    use std::os::windows::fs::OpenOptionsExt;
    const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
    const ERROR_ACCESS_DENIED: i32 = 5;
    const ERROR_INVALID_FUNCTION: i32 = 1;
    // Failing to open the directory is an error; only the flush call's refusal is tolerated.
    let dir = OpenOptions::new().read(true).write(true).custom_flags(FILE_FLAG_BACKUP_SEMANTICS).open(path)?;
    match dir.sync_all() {
        Err(e) if matches!(e.raw_os_error(), Some(ERROR_ACCESS_DENIED | ERROR_INVALID_FUNCTION)) => Ok(()),
        other => other,
    }
}

#[cfg(not(any(unix, windows)))]
fn sync_dir(_path: &Path) -> io::Result<()> {
    Ok(())
}
