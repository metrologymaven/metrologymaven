//! The bounded, fail-closed Result Package reader (spec node `TRUST.PKG.READER`).
//!
//! [`PackageReader::open`] is the only way verifiers open a package. It reads the ZIP end
//! records and central directory, every local header, and `manifest.json`, and refuses the
//! package unless all of this holds:
//!
//! - **Limits** ([`Limits`], `pkg.limit`): package size, entry count, name length, manifest
//!   size, a 1000:1 declared decompression ratio, and an allocation budget for the parsed
//!   index. Each limit is checked from the declared values *before* the bytes it guards are
//!   read or the memory is reserved.
//! - **Container** (`pkg.malformed`): one end record at the very end (no archive comment),
//!   zip64 records consistent with the classic ones, a single disk, a central directory that
//!   ends exactly where the end records begin and whose headers fill it exactly; per entry,
//!   only stored or deflate, no encryption (flag bits 0, 6, 13 or method 99), no data
//!   descriptors or other flags, version needed at most 4.5, Unix or MS-DOS host with a
//!   regular-file type (no symlinks, folders, devices or FIFOs), no entry comment, no extra
//!   field other than a well-formed zip64 one, a UTF-8 name that passes `PkgPath` (no
//!   absolute paths, `..`, backslashes, drive letters, non-NFC text) and the reader's own
//!   rules (no control characters, no slash or dot lookalikes, no invisible characters, no
//!   `:`, no segment ending in `.` or a space, no Windows device name); no duplicate names,
//!   no names equal after full Unicode case folding, no name that is also a folder of another;
//!   local headers that repeat the central ones field for field; and entries that tile the
//!   file from byte 0 to the central directory with no gap and no overlap.
//! - **Manifest** (TRUST.PKG.MANIFEST): `manifest.json` is at most
//!   [`MANIFEST_HEADER_BYTES`] + [`MANIFEST_MEMBER_BYTES`] per container entry (and never
//!   above [`MAX_MANIFEST_BYTES`]), parses within an element budget of
//!   [`MANIFEST_HEADER_VALUES`] + [`MANIFEST_VALUES_PER_MEMBER`] per entry (and never above
//!   [`MAX_MANIFEST_VALUES`]), so a small package cannot make the JSON tree large
//!   (`pkg.limit` `manifest_size` / `manifest_budget`), and validates (a different
//!   major version is `pkg.unsupported_version`); every entry other than `manifest.json`,
//!   `statement.json` and `signatures/*` is listed (`pkg.unlisted_member`); every listed
//!   member is present (`pkg.malformed` `missing_member`) with the declared length
//!   (`pkg.digest_mismatch` `length`).
//!
//! **Member bytes are handed out only after they are verified** (the pinned choice): a
//! member is decoded completely into a buffer of at most its manifest length (and at most
//! the caller's cap), its length, SHA-256 and CRC-32 are compared, and only then is the
//! buffer returned ([`PackageReader::read_member`]) or wrapped in a [`MemberStream`]. A
//! consumer never sees an unverified byte. [`PackageReader::verify_member`] and
//! [`PackageReader::verify_all`] stream the same checks without keeping the bytes. While
//! decoding, output beyond the declared length, or beyond 1000 times the compressed bytes
//! consumed so far, stops the member with `pkg.limit` (zip bombs are cut off while
//! streaming, after at most one 16 KiB chunk, never after the fact).
//!
//! **One view of the signed files.** The manifest bytes read at open are kept and served
//! for every later read of `manifest.json`, and the SHA-256 of `statement.json` is recorded
//! at open; a later read whose bytes hash differently is `pkg.malformed` `source_changed`.
//! So the signature check sees exactly the bytes the member digests were checked against,
//! even if the source changes underneath.
//!
//! The reader never writes to the file system, never panics and keeps its own memory within
//! [`Limits::max_index_bytes`] for the index, fixed decoding buffers, the manifest (at most
//! [`MAX_MANIFEST_BYTES`] of bytes plus a value tree charged [`MANIFEST_VALUE_COST`] per
//! element, at most [`MANIFEST_VALUE_BUDGET`]), and any member buffer the caller asks for.

mod command;
mod error;
mod inflate;
mod names;

pub use command::{
    declarations, inspect, inspect_package, invoke_cli, invoke_mcp, CliOutput, FileOpener, HandleTable, InspectParams,
    InventoryTier, MemberReport, MemberState, PackageInventory, PackageOpener, PackageSource, CLI_GROUP, CLI_INSPECT,
    COMMAND_ID, MCP_INSPECT,
};
pub use error::{ReadError, LIMIT_REASONS, MALFORMED_REASONS};

use crate::casefold::casefold;
use crate::container::crc32_update;
use crate::error::PkgError;
use crate::manifest::{hash_bytes, Manifest, Member, MANIFEST_PATH, STATEMENT_PATH};
use crate::path::PkgPath;
use fid_types::Digest;
use inflate::Inflater;
use sha2::{Digest as _, Sha256};
use std::collections::BTreeMap;
use std::io::{BufReader, ErrorKind, Read, Seek, SeekFrom};

/// Largest package, in bytes (REQ-001).
pub const MAX_PACKAGE_BYTES: u64 = 16 * 1024 * 1024 * 1024;
/// Most entries in the container (REQ-001).
pub const MAX_MEMBERS: u64 = 1_000_000;
/// Longest entry name, in bytes (REQ-001).
pub const MAX_NAME_BYTES: usize = 4096;
/// Largest `manifest.json`, uncompressed: 16 MiB, pinned well below the spec's 256 MiB
/// (REQ-001) so the parsed manifest stays within [`MANIFEST_VALUE_BUDGET`] plus its bytes.
pub const MAX_MANIFEST_BYTES: u64 = 16 * 1024 * 1024;
/// The spec's manifest limit (REQ-001); [`MAX_MANIFEST_BYTES`] is tighter.
pub const SPEC_MAX_MANIFEST_BYTES: u64 = 256 * 1024 * 1024;
/// Manifest bytes allowed besides the member entries (format, version, digests, `created_by`).
pub const MANIFEST_HEADER_BYTES: u64 = 64 * 1024;
/// Manifest bytes allowed per container entry: a 4096-byte path plus 512 bytes for the
/// rest of a member object (keys, media type, length, SHA-256).
pub const MANIFEST_MEMBER_BYTES: u64 = 4096 + 512;
/// Bytes charged per parsed manifest element (value or key): at least a `CValue`, a `String`
/// and the share of a `BTreeMap` node (checked at compile time).
pub const MANIFEST_VALUE_COST: u64 = 96;
/// Memory budget of the parsed manifest tree, besides its string bytes.
pub const MANIFEST_VALUE_BUDGET: u64 = 32 * 1024 * 1024;
/// Most elements a manifest may parse into: [`MANIFEST_VALUE_BUDGET`] / [`MANIFEST_VALUE_COST`].
pub const MAX_MANIFEST_VALUES: u64 = MANIFEST_VALUE_BUDGET / MANIFEST_VALUE_COST;
/// Manifest elements allowed besides the members: the root object and its keys and values.
pub const MANIFEST_HEADER_VALUES: u64 = 64;
/// Manifest elements per container entry: a member object, four keys and four values.
pub const MANIFEST_VALUES_PER_MEMBER: u64 = 9;
/// Deepest manifest nesting (root, members array, member object, value).
const MANIFEST_MAX_DEPTH: usize = 8;
/// Largest decompression ratio of a deflate member (REQ-003).
pub const MAX_RATIO: u64 = 1000;
/// Allocation budget of the parsed central directory (REQ-005).
pub const MAX_INDEX_BYTES: u64 = 64 * 1024 * 1024;

/// Bytes charged against the index budget for each entry, before its name: the `Entry`
/// itself in an exactly reserved vector, plus the case-fold map node and the sort index.
pub(crate) const ENTRY_FIXED_COST: u64 = 256;
/// Bytes charged per name byte: the name, its lowercase copy (which can grow up to three
/// times in UTF-8) in the case-fold map.
pub(crate) const NAME_COST_FACTOR: u64 = 4;

// The charged costs cover the real sizes (checked when the crate compiles).
const _: () = assert!(
    ENTRY_FIXED_COST as usize
        >= std::mem::size_of::<Entry>()
            + 2 * std::mem::size_of::<usize>()
            + std::mem::size_of::<String>()
            + std::mem::size_of::<&str>()
            + 64
);
// A name is stored once and folded once; full case folding grows UTF-8 at most three times.
// (one copy of the name plus a folding of up to three times its length)
const _: () = assert!(NAME_COST_FACTOR >= 4);
const _: () = assert!(
    MANIFEST_VALUE_COST as usize >= std::mem::size_of::<fid_canon::CValue>() + std::mem::size_of::<String>() + 32
);
const _: () = assert!(MAX_MANIFEST_BYTES as usize <= fid_canon::MAX_INPUT_BYTES);

const SIG_LOCAL: u32 = 0x0403_4b50;
const SIG_CENTRAL: u32 = 0x0201_4b50;
const SIG_EOCD: u32 = 0x0605_4b50;
const SIG_EOCD64: u32 = 0x0606_4b50;
const SIG_LOC64: u32 = 0x0706_4b50;
const EOCD_LEN: u64 = 22;
const MAX_COMMENT: u64 = 0xFFFF;
const FLAG_UTF8: u16 = 0x0800;
const FLAG_DEFLATE_OPTIONS: u16 = 0x0006;
const FLAG_ENCRYPTED: u16 = 0x0001 | 0x0040 | 0x2000;
const FLAG_DESCRIPTOR: u16 = 0x0008;
const METHOD_STORED: u16 = 0;
const METHOD_DEFLATE: u16 = 8;
const METHOD_AES: u16 = 99;
const MAX_VERSION_NEEDED: u16 = 45;
const CHUNK: usize = 64 * 1024;

/// The reader's limits. [`Limits::default`] holds the published values; a caller may only
/// tighten them in practice (tests use small values to reach each check cheaply).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    pub max_package_bytes: u64,
    pub max_members: u64,
    pub max_name_bytes: usize,
    pub max_manifest_bytes: u64,
    pub max_manifest_values: u64,
    pub max_ratio: u64,
    pub max_index_bytes: u64,
}

impl Default for Limits {
    fn default() -> Limits {
        Limits {
            max_package_bytes: MAX_PACKAGE_BYTES,
            max_members: MAX_MEMBERS,
            max_name_bytes: MAX_NAME_BYTES,
            max_manifest_bytes: MAX_MANIFEST_BYTES,
            max_manifest_values: MAX_MANIFEST_VALUES,
            max_ratio: MAX_RATIO,
            max_index_bytes: MAX_INDEX_BYTES,
        }
    }
}

/// One container entry after the central-directory and local-header checks.
#[derive(Debug, Clone)]
struct Entry {
    name: String,
    method: u16,
    flags: u16,
    needed: u16,
    time: u16,
    date: u16,
    crc: u32,
    csize: u64,
    usize: u64,
    offset: u64,
    data_start: u64,
}

/// What decoding a member produced.
struct Decoded {
    length: u64,
    sha256: Digest,
    crc: u32,
}

/// The allocation budget of the index.
struct Budget {
    left: u64,
}

impl Budget {
    fn charge(&mut self, bytes: u64) -> Result<(), ReadError> {
        match self.left.checked_sub(bytes) {
            Some(left) => {
                self.left = left;
                Ok(())
            }
            None => Err(ReadError::limit("index_budget", "the central directory needs more memory than allowed")),
        }
    }
}

// ------------------------------------------------------------------ byte helpers

fn le16(b: &[u8], at: usize) -> u16 {
    b.get(at..at + 2).and_then(|s| s.try_into().ok()).map(u16::from_le_bytes).unwrap_or(0)
}

fn le32(b: &[u8], at: usize) -> u32 {
    b.get(at..at + 4).and_then(|s| s.try_into().ok()).map(u32::from_le_bytes).unwrap_or(0)
}

fn le64(b: &[u8], at: usize) -> u64 {
    b.get(at..at + 8).and_then(|s| s.try_into().ok()).map(u64::from_le_bytes).unwrap_or(0)
}

fn io(e: &std::io::Error) -> ReadError {
    ReadError::from_io(e)
}

fn read_full<R: Read + ?Sized>(src: &mut R, buf: &mut [u8]) -> Result<(), ReadError> {
    src.read_exact(buf).map_err(|e| io(&e))
}

fn read_at<R: Read + Seek>(src: &mut R, off: u64, buf: &mut [u8]) -> Result<(), ReadError> {
    src.seek(SeekFrom::Start(off)).map_err(|e| io(&e))?;
    read_full(src, buf)
}

fn malformed(reason: &'static str, detail: impl Into<String>) -> ReadError {
    ReadError::malformed(reason, detail)
}

// ------------------------------------------------------------------ the reader

/// An opened, structurally verified package. See the module documentation.
pub struct PackageReader<R> {
    src: R,
    limits: Limits,
    /// Sorted by name bytes.
    entries: Vec<Entry>,
    manifest: Manifest,
    /// The exact `manifest.json` bytes read at open.
    manifest_bytes: Vec<u8>,
    manifest_sha256: Digest,
    /// SHA-256 of `statement.json` as read at open, if present.
    statement_sha256: Option<Digest>,
}

impl<R> std::fmt::Debug for PackageReader<R> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PackageReader").field("entries", &self.entries.len()).finish_non_exhaustive()
    }
}

impl<R: Read + Seek> PackageReader<R> {
    /// Opens a package with the published [`Limits`].
    pub fn open(src: R) -> Result<PackageReader<R>, ReadError> {
        PackageReader::open_with(src, Limits::default())
    }

    /// Opens a package with explicit limits.
    pub fn open_with(mut src: R, limits: Limits) -> Result<PackageReader<R>, ReadError> {
        let len = src.seek(SeekFrom::End(0)).map_err(|e| io(&e))?;
        if len > limits.max_package_bytes {
            return Err(ReadError::limit("package_size", format!("{len} bytes")));
        }
        if len < EOCD_LEN {
            return Err(malformed("truncated", format!("{len} bytes is shorter than an end record")));
        }
        let eocd_off = find_eocd(&mut src, len)?;
        let dir = read_end_records(&mut src, eocd_off)?;
        if dir.count > limits.max_members {
            return Err(ReadError::limit("member_count", format!("{} entries", dir.count)));
        }
        if dir.offset.checked_add(dir.size) != Some(dir.end) {
            return Err(malformed("cd_bounds", "the central directory does not end where the end records begin"));
        }
        if dir.count.saturating_mul(46) > dir.size {
            return Err(malformed("cd_size", "the central directory is too small for its entry count"));
        }
        let mut budget = Budget { left: limits.max_index_bytes };
        budget.charge(dir.count.saturating_mul(ENTRY_FIXED_COST))?;
        let capacity = usize::try_from(dir.count).map_err(|_| ReadError::limit("member_count", "entry count"))?;
        let mut entries = Vec::with_capacity(capacity);
        let manifest_cap = limits
            .max_manifest_bytes
            .min(MANIFEST_HEADER_BYTES.saturating_add(MANIFEST_MEMBER_BYTES.saturating_mul(dir.count)));
        read_central(&mut src, &dir, &limits, manifest_cap, &mut budget, &mut entries)?;
        check_names(&mut entries)?;
        check_layout(&mut src, &dir, &mut entries)?;

        let max_values = limits
            .max_manifest_values
            .min(MANIFEST_HEADER_VALUES.saturating_add(MANIFEST_VALUES_PER_MEMBER.saturating_mul(dir.count)));
        let (manifest, manifest_bytes) = load_manifest(&mut src, &entries, &limits, max_values)?;
        cross_check(&entries, &manifest)?;
        let manifest_sha256 = hash_bytes(&manifest_bytes);
        let mut reader =
            PackageReader { src, limits, entries, manifest, manifest_bytes, manifest_sha256, statement_sha256: None };
        if let Some(i) = reader.find(STATEMENT_PATH) {
            reader.statement_sha256 = Some(reader.decode_checked(i, &mut |_| Ok(()))?);
        }
        Ok(reader)
    }

    /// The exact `manifest.json` bytes read and parsed at open.
    pub fn manifest_bytes(&self) -> &[u8] {
        &self.manifest_bytes
    }

    /// SHA-256 of `statement.json` as read at open, if the package has one.
    pub fn statement_sha256(&self) -> Option<Digest> {
        self.statement_sha256
    }

    /// The parsed, validated manifest.
    pub fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    /// Every entry name, sorted by bytes.
    pub fn entry_names(&self) -> Vec<&str> {
        self.entries.iter().map(|e| e.name.as_str()).collect()
    }

    /// Number of entries under `signatures/`.
    pub fn signature_count(&self) -> usize {
        self.entries.iter().filter(|e| is_signature(&e.name)).count()
    }

    /// Whether `statement.json` is present.
    pub fn has_statement(&self) -> bool {
        self.find(STATEMENT_PATH).is_some()
    }

    fn find(&self, name: &str) -> Option<usize> {
        self.entries.binary_search_by(|e| e.name.as_str().cmp(name)).ok()
    }

    fn listed(&self, name: &str) -> Option<&Member> {
        let i = self.manifest.members.binary_search_by(|m| m.path.as_str().cmp(name)).ok()?;
        self.manifest.members.get(i)
    }

    /// Decodes entry `i` into `out` and checks it: against its manifest entry for a listed
    /// member (length, SHA-256), against the central directory otherwise (length); CRC-32
    /// always.
    fn check_entry(&mut self, i: usize, out: &mut dyn FnMut(&[u8]) -> Result<(), ReadError>) -> Result<(), ReadError> {
        self.decode_checked(i, out).map(|_| ())
    }

    /// Decodes and checks entry `i` (see [`PackageReader::check_entry`]); returns its SHA-256.
    /// `manifest.json` and `statement.json` must also hash to what was read at open
    /// (`source_changed`), checked before the CRC so a changed source is named as such.
    fn decode_checked(
        &mut self,
        i: usize,
        out: &mut dyn FnMut(&[u8]) -> Result<(), ReadError>,
    ) -> Result<Digest, ReadError> {
        let e = self.entries.get(i).ok_or_else(|| malformed("missing_member", "entry index"))?.clone();
        let decoded = decode_entry(&mut self.src, &e, self.limits.max_ratio, out)?;
        match self.listed(&e.name) {
            Some(m) => {
                if decoded.length != m.length {
                    return Err(ReadError::mismatch(&e.name, "length"));
                }
                if decoded.sha256 != m.sha256 {
                    return Err(ReadError::mismatch(&e.name, "sha256"));
                }
            }
            None => {
                if decoded.length != e.usize {
                    return Err(malformed(
                        "length",
                        format!("{}: {} bytes, {} declared", e.name, decoded.length, e.usize),
                    ));
                }
            }
        }
        let pinned = match e.name.as_str() {
            MANIFEST_PATH => Some(self.manifest_sha256),
            STATEMENT_PATH => self.statement_sha256,
            _ => None,
        };
        if pinned.is_some_and(|d| d != decoded.sha256) {
            return Err(malformed("source_changed", format!("{} differs from the bytes read at open", e.name)));
        }
        if decoded.crc != e.crc {
            return Err(malformed("crc", e.name));
        }
        Ok(decoded.sha256)
    }

    /// The verified bytes of a member (a manifest member, `statement.json` or a
    /// `signatures/` entry), or `None` if the package has no such entry. A member larger
    /// than `max_bytes` is `pkg.limit` `buffer_cap` before anything is read. Listed members
    /// are checked against the manifest; `statement.json` and signatures, which the manifest
    /// does not list, against the container's length and CRC only (their authenticity is
    /// the signature check's job).
    pub fn read_member(&mut self, path: &str, max_bytes: u64) -> Result<Option<Vec<u8>>, ReadError> {
        let Some(i) = self.find(path) else { return Ok(None) };
        let declared = self.entries.get(i).map_or(0, |e| e.usize);
        if declared > max_bytes {
            return Err(ReadError::limit("buffer_cap", format!("{path}: {declared} bytes, cap {max_bytes}")));
        }
        if path == MANIFEST_PATH {
            // The bytes parsed at open, never a second read.
            return Ok(Some(self.manifest_bytes.clone()));
        }
        let cap = usize::try_from(declared).map_err(|_| ReadError::limit("buffer_cap", path))?;
        let mut buf: Vec<u8> = Vec::with_capacity(cap);
        self.check_entry(i, &mut |chunk| {
            buf.extend_from_slice(chunk);
            Ok(())
        })?;
        Ok(Some(buf))
    }

    /// [`PackageReader::read_member`] as a bounded [`Read`] over the verified bytes.
    pub fn open_member(&mut self, path: &str, max_bytes: u64) -> Result<Option<MemberStream>, ReadError> {
        Ok(self.read_member(path, max_bytes)?.map(|bytes| MemberStream { path: path.to_string(), bytes, pos: 0 }))
    }

    /// Streams the checks of [`PackageReader::read_member`] without keeping the bytes.
    /// `Ok(None)` if the package has no such entry.
    pub fn verify_member(&mut self, path: &str) -> Result<Option<()>, ReadError> {
        let Some(i) = self.find(path) else { return Ok(None) };
        self.check_entry(i, &mut |_| Ok(()))?;
        Ok(Some(()))
    }

    /// The check result of every manifest member, in manifest order.
    pub fn member_results(&mut self) -> Vec<Result<(), ReadError>> {
        let paths: Vec<String> = self.manifest.members.iter().map(|m| m.path.as_str().to_string()).collect();
        paths
            .iter()
            .map(|p| match self.verify_member(p) {
                Ok(Some(())) => Ok(()),
                Ok(None) => Err(malformed("missing_member", p.as_str())),
                Err(e) => Err(e),
            })
            .collect()
    }

    /// Verifies every manifest member (manifest order), then `statement.json`, then every
    /// signature entry, and returns the first error.
    pub fn verify_all(&mut self) -> Result<(), ReadError> {
        for r in self.member_results() {
            r?;
        }
        let rest: Vec<String> = self
            .entries
            .iter()
            .filter(|e| e.name == STATEMENT_PATH || is_signature(&e.name))
            .map(|e| e.name.clone())
            .collect();
        for name in rest {
            self.verify_member(&name)?;
        }
        Ok(())
    }
}

/// A member's verified bytes as a [`Read`]. It exists only after the member passed every
/// check, so a consumer never reads an unverified byte.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemberStream {
    path: String,
    bytes: Vec<u8>,
    pos: usize,
}

impl MemberStream {
    /// The member path.
    pub fn path(&self) -> &str {
        &self.path
    }

    /// Total length in bytes.
    pub fn len(&self) -> usize {
        self.bytes.len()
    }

    /// Whether the member is empty.
    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }

    /// The verified bytes.
    pub fn into_bytes(self) -> Vec<u8> {
        self.bytes
    }
}

impl Read for MemberStream {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let rest = self.bytes.get(self.pos..).unwrap_or_default();
        let n = rest.len().min(buf.len());
        if let (Some(dst), Some(src)) = (buf.get_mut(..n), rest.get(..n)) {
            dst.copy_from_slice(src);
        }
        self.pos += n;
        Ok(n)
    }
}

fn is_signature(name: &str) -> bool {
    name.starts_with("signatures/")
}

// ------------------------------------------------------------------ end records

struct Directory {
    count: u64,
    size: u64,
    offset: u64,
    /// Where the central directory must end: the zip64 end record, or the end record.
    end: u64,
}

/// The end record must sit at the very end with an empty comment. A bounded backward
/// search only tells an archive comment (`archive_comment`) from a missing record (`no_eocd`).
fn find_eocd<R: Read + Seek>(src: &mut R, len: u64) -> Result<u64, ReadError> {
    let mut tail = [0u8; 22];
    read_at(src, len - EOCD_LEN, &mut tail)?;
    if le32(&tail, 0) == SIG_EOCD && le16(&tail, 20) == 0 {
        return Ok(len - EOCD_LEN);
    }
    let span = len.min(EOCD_LEN + MAX_COMMENT);
    let mut buf = vec![0u8; usize::try_from(span).unwrap_or(0)];
    read_at(src, len - span, &mut buf)?;
    let n = buf.len();
    for i in (0..=n.saturating_sub(22)).rev() {
        if le32(&buf, i) == SIG_EOCD && i + 22 + usize::from(le16(&buf, i + 20)) == n {
            return Err(malformed("archive_comment", "the package has an archive comment"));
        }
    }
    Err(malformed("no_eocd", "no end-of-central-directory record at the end of the package"))
}

fn read_end_records<R: Read + Seek>(src: &mut R, eocd_off: u64) -> Result<Directory, ReadError> {
    let mut e = [0u8; 22];
    read_at(src, eocd_off, &mut e)?;
    let (disk, cd_disk) = (le16(&e, 4), le16(&e, 6));
    let (n_disk, n_total) = (le16(&e, 8), le16(&e, 10));
    let (size32, off32) = (le32(&e, 12), le32(&e, 16));
    if disk != 0 || cd_disk != 0 {
        return Err(malformed("multi_disk", "the end record names another disk"));
    }
    let zip64 = n_disk == u16::MAX || n_total == u16::MAX || size32 == u32::MAX || off32 == u32::MAX;
    if !zip64 {
        if n_disk != n_total {
            return Err(malformed("eocd_mismatch", "entries on this disk differ from the total"));
        }
        return Ok(Directory { count: n_total.into(), size: size32.into(), offset: off32.into(), end: eocd_off });
    }
    let loc_off = eocd_off.checked_sub(20).ok_or_else(|| malformed("zip64_locator", "no room for the locator"))?;
    let mut l = [0u8; 20];
    read_at(src, loc_off, &mut l)?;
    if le32(&l, 0) != SIG_LOC64 {
        return Err(malformed("zip64_locator", "zip64 values without a zip64 locator"));
    }
    if le32(&l, 4) != 0 || le32(&l, 16) != 1 {
        return Err(malformed("zip64_locator", "the zip64 locator names another disk"));
    }
    let rec_off = le64(&l, 8);
    if rec_off.checked_add(56).map_or(true, |end| end > loc_off) {
        return Err(malformed("zip64_record", "the zip64 end record is out of place"));
    }
    let mut r = [0u8; 56];
    read_at(src, rec_off, &mut r)?;
    if le32(&r, 0) != SIG_EOCD64 {
        return Err(malformed("zip64_record", "bad zip64 end record signature"));
    }
    let size = le64(&r, 4);
    if size != 44 {
        return Err(malformed("zip64_record", "the zip64 end record has an extensible data sector"));
    }
    if rec_off.checked_add(56) != Some(loc_off) {
        return Err(malformed("zip64_record", "the zip64 end record does not end at the locator"));
    }
    if le32(&r, 16) != 0 || le32(&r, 20) != 0 {
        return Err(malformed("multi_disk", "the zip64 end record names another disk"));
    }
    let (d64, t64, s64, o64) = (le64(&r, 24), le64(&r, 32), le64(&r, 40), le64(&r, 48));
    let differs16 = |v: u16, w: u64| v != u16::MAX && u64::from(v) != w;
    let differs32 = |v: u32, w: u64| v != u32::MAX && u64::from(v) != w;
    if d64 != t64
        || differs16(n_disk, d64)
        || differs16(n_total, t64)
        || differs32(size32, s64)
        || differs32(off32, o64)
    {
        return Err(malformed("eocd_mismatch", "the classic and zip64 end records disagree"));
    }
    Ok(Directory { count: t64, size: s64, offset: o64, end: rec_off })
}

// ------------------------------------------------------------------ central directory

fn check_flags(flags: u16, method: u16) -> Result<(), ReadError> {
    if flags & FLAG_ENCRYPTED != 0 || method == METHOD_AES {
        return Err(malformed("encrypted", "an encrypted entry"));
    }
    if flags & FLAG_DESCRIPTOR != 0 {
        return Err(malformed("data_descriptor", "sizes in a data descriptor"));
    }
    let allowed = FLAG_UTF8 | if method == METHOD_DEFLATE { FLAG_DEFLATE_OPTIONS } else { 0 };
    if flags & !allowed != 0 {
        return Err(malformed("flags", format!("general-purpose flags {flags:#06x}")));
    }
    Ok(())
}

fn check_attrs(made_by: u16, ext: u32) -> Result<(), ReadError> {
    let host = made_by >> 8;
    if host != 0 && host != 3 {
        return Err(malformed("host", format!("host system {host}")));
    }
    if ext & 0x18 != 0 {
        return Err(malformed("special_file", "MS-DOS folder or volume attribute"));
    }
    let kind = (ext >> 16) & 0o170_000;
    if kind == 0o120_000 {
        return Err(malformed("symlink", "a symbolic link"));
    }
    if kind != 0 && kind != 0o100_000 {
        return Err(malformed("special_file", format!("file type {kind:#o}")));
    }
    Ok(())
}

fn cd_read<R: Read>(rd: &mut R, buf: &mut [u8]) -> Result<(), ReadError> {
    rd.read_exact(buf).map_err(|e| {
        if e.kind() == ErrorKind::UnexpectedEof {
            malformed("cd_size", "the central directory ends inside a header")
        } else {
            io(&e)
        }
    })
}

/// The zip64 extended-information values `(usize, csize, offset, disk)`, given the classic
/// values; `extra` may hold only that one record.
fn zip64_values(extra: &[u8], usize32: u32, csize32: u32, off32: u32, disk16: u16) -> Result<[u64; 4], ReadError> {
    let mut found: Option<&[u8]> = None;
    let mut i = 0;
    while i < extra.len() {
        let (tag, len) = (le16(extra, i), usize::from(le16(extra, i + 2)));
        let data = extra.get(i + 4..i + 4 + len).ok_or_else(|| malformed("extra_field", "a truncated extra record"))?;
        if i + 4 > extra.len() || tag != 1 {
            return Err(malformed("extra_field", format!("extra field {tag:#06x}")));
        }
        if found.is_some() {
            return Err(malformed("extra_field", "two zip64 records"));
        }
        found = Some(data);
        i += 4 + len;
    }
    let marks = [usize32 == u32::MAX, csize32 == u32::MAX, off32 == u32::MAX, disk16 == u16::MAX];
    let want: usize = marks.iter().zip([8, 8, 8, 4]).map(|(&m, n)| if m { n } else { 0 }).sum();
    let mut out = [u64::from(usize32), u64::from(csize32), u64::from(off32), u64::from(disk16)];
    match found {
        None if want == 0 => Ok(out),
        Some(d) if want != 0 && d.len() == want => {
            let mut at = 0;
            for (k, &m) in marks.iter().enumerate() {
                if m {
                    let v = if k == 3 { u64::from(le32(d, at)) } else { le64(d, at) };
                    if let Some(slot) = out.get_mut(k) {
                        *slot = v;
                    }
                    at += if k == 3 { 4 } else { 8 };
                }
            }
            Ok(out)
        }
        _ => Err(malformed("zip64_extra", "the zip64 record does not match the marked fields")),
    }
}

fn read_central<R: Read + Seek>(
    src: &mut R,
    dir: &Directory,
    limits: &Limits,
    manifest_cap: u64,
    budget: &mut Budget,
    entries: &mut Vec<Entry>,
) -> Result<(), ReadError> {
    src.seek(SeekFrom::Start(dir.offset)).map_err(|e| io(&e))?;
    let mut rd = BufReader::with_capacity(CHUNK, src.by_ref().take(dir.size));
    let mut pos: u64 = 0;
    for _ in 0..dir.count {
        let mut h = [0u8; 46];
        cd_read(&mut rd, &mut h)?;
        if le32(&h, 0) != SIG_CENTRAL {
            return Err(malformed("cd_signature", "bad central header signature"));
        }
        let (made_by, needed, flags, method) = (le16(&h, 4), le16(&h, 6), le16(&h, 8), le16(&h, 10));
        let (time, date, crc) = (le16(&h, 12), le16(&h, 14), le32(&h, 16));
        let (csize32, usize32) = (le32(&h, 20), le32(&h, 24));
        let (name_len, extra_len, comment_len) = (le16(&h, 28), le16(&h, 30), le16(&h, 32));
        let (disk16, ext, off32) = (le16(&h, 34), le32(&h, 38), le32(&h, 42));
        if usize::from(name_len) > limits.max_name_bytes {
            return Err(ReadError::limit("path_length", format!("an entry name of {name_len} bytes")));
        }
        check_flags(flags, method)?;
        if method != METHOD_STORED && method != METHOD_DEFLATE {
            return Err(malformed("method", format!("compression method {method}")));
        }
        if needed > MAX_VERSION_NEEDED {
            return Err(malformed("version_needed", format!("version needed {needed}")));
        }
        check_attrs(made_by, ext)?;
        if comment_len != 0 {
            return Err(malformed("entry_comment", "an entry comment"));
        }
        if disk16 != 0 && disk16 != u16::MAX {
            return Err(malformed("multi_disk", "an entry on another disk"));
        }
        let mut name_bytes = vec![0u8; usize::from(name_len)];
        cd_read(&mut rd, &mut name_bytes)?;
        let mut extra = vec![0u8; usize::from(extra_len)];
        cd_read(&mut rd, &mut extra)?;
        pos += 46 + u64::from(name_len) + u64::from(extra_len);
        let [usize, csize, offset, disk] = zip64_values(&extra, usize32, csize32, off32, disk16)?;
        if disk != 0 {
            return Err(malformed("multi_disk", "an entry on another disk"));
        }
        let name = if flags & FLAG_UTF8 != 0 || name_bytes.is_ascii() {
            String::from_utf8(name_bytes).map_err(|_| malformed("name_encoding", "a name that is not UTF-8"))?
        } else {
            return Err(malformed("name_encoding", "a non-ASCII name without the UTF-8 flag"));
        };
        match PkgPath::parse(&name) {
            Ok(_) => {}
            Err(PkgError::BadPath { reason, .. }) => return Err(malformed("bad_path", format!("{reason}: {name:?}"))),
            Err(other) => return Err(malformed("bad_path", other.to_string())),
        }
        names::check_name(&name).map_err(|why| malformed(why, format!("{name:?}")))?;
        if name == MANIFEST_PATH && usize > manifest_cap {
            return Err(ReadError::limit(
                "manifest_size",
                format!("manifest.json declares {usize} bytes, cap {manifest_cap} for {} entries", dir.count),
            ));
        }
        if method == METHOD_STORED && csize != usize {
            return Err(malformed("stored_size", format!("{name}: stored sizes differ")));
        }
        if method == METHOD_DEFLATE && usize > limits.max_ratio.saturating_mul(csize.max(1)) {
            return Err(ReadError::limit("ratio", format!("{name}: {usize} bytes from {csize}")));
        }
        budget.charge((name.len() as u64).saturating_mul(NAME_COST_FACTOR))?;
        entries.push(Entry { name, method, flags, needed, time, date, crc, csize, usize, offset, data_start: 0 });
    }
    if pos != dir.size {
        return Err(malformed("cd_size", "the central directory holds more than its entries"));
    }
    Ok(())
}

/// Sorts the entries by name and refuses duplicates and names that collide on a
/// case-insensitive file system (equal after full Unicode case folding, or one a folder of
/// the other after folding).
fn check_names(entries: &mut [Entry]) -> Result<(), ReadError> {
    entries.sort_by(|a, b| a.name.cmp(&b.name));
    for pair in entries.windows(2) {
        if let [a, b] = pair {
            if a.name == b.name {
                return Err(malformed("duplicate_entry", a.name.clone()));
            }
        }
    }
    let mut folded: BTreeMap<String, &str> = BTreeMap::new();
    for e in entries.iter() {
        if let Some(other) = folded.insert(casefold(&e.name), &e.name) {
            return Err(malformed("case_collision", format!("{other:?} and {:?}", e.name)));
        }
    }
    // One range query per key: the first key at or after `<key>/` shows whether any key is
    // below it.
    for (key, name) in &folded {
        let below = format!("{key}/");
        if folded.range(below.clone()..).next().is_some_and(|(k, _)| k.starts_with(&below)) {
            return Err(malformed("path_conflict", format!("{name:?} is also a folder")));
        }
    }
    Ok(())
}

/// Reads every local header in offset order, checks it against the central header, and
/// checks that the entries tile `[0, central directory)` exactly.
fn check_layout<R: Read + Seek>(src: &mut R, dir: &Directory, entries: &mut [Entry]) -> Result<(), ReadError> {
    let mut order: Vec<usize> = (0..entries.len()).collect();
    order.sort_by_key(|&i| (entries.get(i).map_or(0, |e| e.offset), i));
    for pair in order.windows(2) {
        if let [a, b] = pair {
            if entries.get(*a).map(|e| e.offset) == entries.get(*b).map(|e| e.offset) {
                return Err(malformed("overlap", "two entries share one local header"));
            }
        }
    }
    let mut expected: u64 = 0;
    for &i in &order {
        let e = entries.get_mut(i).ok_or_else(|| malformed("overlap", "entry index"))?;
        if e.offset < expected {
            return Err(malformed("overlap", format!("{} starts inside the previous entry", e.name)));
        }
        if e.offset > expected {
            return Err(malformed("gap", format!("{} bytes before {}", e.offset - expected, e.name)));
        }
        if e.offset.saturating_add(30) > dir.offset {
            return Err(malformed("overlap", format!("{} runs into the central directory", e.name)));
        }
        let start = check_local(src, e)?;
        let end = start.checked_add(e.csize).ok_or_else(|| malformed("overlap", "entry end overflows"))?;
        if end > dir.offset {
            return Err(malformed("overlap", format!("{} runs into the central directory", e.name)));
        }
        e.data_start = start;
        expected = end;
    }
    if expected != dir.offset {
        return Err(malformed("gap", "bytes between the last entry and the central directory"));
    }
    Ok(())
}

fn check_local<R: Read + Seek>(src: &mut R, e: &Entry) -> Result<u64, ReadError> {
    let mut h = [0u8; 30];
    read_at(src, e.offset, &mut h)?;
    if le32(&h, 0) != SIG_LOCAL {
        return Err(malformed("local_signature", format!("{}: bad local header signature", e.name)));
    }
    let mismatch = |what: &str| malformed("local_mismatch", format!("{}: {what}", e.name));
    let same = le16(&h, 4) == e.needed
        && le16(&h, 6) == e.flags
        && le16(&h, 8) == e.method
        && le16(&h, 10) == e.time
        && le16(&h, 12) == e.date
        && le32(&h, 14) == e.crc;
    if !same {
        return Err(mismatch("header fields differ from the central directory"));
    }
    let (csize32, usize32) = (le32(&h, 18), le32(&h, 22));
    let (name_len, extra_len) = (usize::from(le16(&h, 26)), usize::from(le16(&h, 28)));
    if name_len != e.name.len() {
        return Err(mismatch("name length"));
    }
    let mut rest = vec![0u8; name_len + extra_len];
    read_full(src, &mut rest)?;
    if rest.get(..name_len) != Some(e.name.as_bytes()) {
        return Err(mismatch("name"));
    }
    let extra = rest.get(name_len..).unwrap_or_default();
    let (lu, lc) = if csize32 == u32::MAX && usize32 == u32::MAX {
        if extra.len() != 20 || le16(extra, 0) != 1 || le16(extra, 2) != 16 {
            return Err(mismatch("zip64 local extra field"));
        }
        (le64(extra, 4), le64(extra, 12))
    } else {
        if !extra.is_empty() {
            return Err(mismatch("extra field"));
        }
        (u64::from(usize32), u64::from(csize32))
    };
    if lu != e.usize || lc != e.csize {
        return Err(mismatch("sizes"));
    }
    Ok(e.offset + 30 + (name_len + extra_len) as u64)
}

// ------------------------------------------------------------------ members

/// Decodes one entry, feeding every chunk to `out` after the size and ratio caps passed.
fn decode_entry<R: Read + Seek>(
    src: &mut R,
    e: &Entry,
    max_ratio: u64,
    out: &mut dyn FnMut(&[u8]) -> Result<(), ReadError>,
) -> Result<Decoded, ReadError> {
    src.seek(SeekFrom::Start(e.data_start)).map_err(|err| io(&err))?;
    let mut input = src.by_ref().take(e.csize);
    let mut hasher = Sha256::new();
    let mut crc = 0u32;
    let mut total: u64 = 0;
    let deflate = e.method == METHOD_DEFLATE;
    let cap = e.usize;
    {
        let mut sink = |chunk: &[u8], consumed: u64| -> Result<(), ReadError> {
            total = total.saturating_add(chunk.len() as u64);
            if total > cap {
                return Err(ReadError::limit("member_length", format!("{}: more than {cap} bytes", e.name)));
            }
            if deflate && total > max_ratio.saturating_mul(consumed.max(1)) {
                return Err(ReadError::limit("ratio", format!("{}: above {max_ratio}:1", e.name)));
            }
            hasher.update(chunk);
            crc = crc32_update(crc, chunk);
            out(chunk)
        };
        if deflate {
            Inflater::new(&mut input).run(&mut sink)?;
        } else {
            let mut buf = vec![0u8; CHUNK];
            let mut read: u64 = 0;
            while read < e.csize {
                let n = match input.read(&mut buf) {
                    Ok(0) => return Err(malformed("truncated", format!("{}: data ends early", e.name))),
                    Ok(n) => n.min(buf.len()),
                    Err(err) if err.kind() == ErrorKind::Interrupted => continue,
                    Err(err) => return Err(io(&err)),
                };
                read += n as u64;
                sink(buf.get(..n).unwrap_or_default(), read)?;
            }
        }
    }
    Ok(Decoded { length: total, sha256: Digest::from_bytes(hasher.finalize().into()), crc })
}

fn load_manifest<R: Read + Seek>(
    src: &mut R,
    entries: &[Entry],
    limits: &Limits,
    max_values: u64,
) -> Result<(Manifest, Vec<u8>), ReadError> {
    let i = entries
        .binary_search_by(|e| e.name.as_str().cmp(MANIFEST_PATH))
        .map_err(|_| malformed("no_manifest", "no manifest.json"))?;
    let e = entries.get(i).ok_or_else(|| malformed("no_manifest", "no manifest.json"))?;
    if e.usize > fid_canon::MAX_INPUT_BYTES as u64 {
        return Err(ReadError::limit(
            "manifest_size",
            format!("manifest.json declares {} bytes, above the JSON parser's cap", e.usize),
        ));
    }
    let mut buf: Vec<u8> = Vec::with_capacity(usize::try_from(e.usize).unwrap_or(0));
    let decoded = decode_entry(src, e, limits.max_ratio, &mut |chunk| {
        buf.extend_from_slice(chunk);
        Ok(())
    })?;
    if decoded.length != e.usize {
        return Err(malformed("length", format!("manifest.json: {} bytes, {} declared", decoded.length, e.usize)));
    }
    if decoded.crc != e.crc {
        return Err(malformed("crc", MANIFEST_PATH));
    }
    let max_elements = usize::try_from(max_values).unwrap_or(usize::MAX);
    let root = fid_canon::parse_with_budget(&buf, fid_canon::MAX_INPUT_BYTES, MANIFEST_MAX_DEPTH, max_elements)
        .map_err(|err| match err {
            fid_canon::CanonError::InputLimit => ReadError::limit(
                "manifest_budget",
                format!("manifest.json is deeper than {MANIFEST_MAX_DEPTH} or has more than {max_values} elements"),
            ),
            other => malformed("manifest", format!("manifest: {other}")),
        })?;
    let manifest = Manifest::from_value(&root).map_err(|err| match err {
        PkgError::UnsupportedVersion(v) => ReadError::UnsupportedVersion(v),
        other => malformed("manifest", other.to_string()),
    })?;
    Ok((manifest, buf))
}

fn cross_check(entries: &[Entry], manifest: &Manifest) -> Result<(), ReadError> {
    for e in entries {
        let n = e.name.as_str();
        if n == MANIFEST_PATH || n == STATEMENT_PATH || is_signature(n) {
            continue;
        }
        if manifest.members.binary_search_by(|m| m.path.as_str().cmp(n)).is_err() {
            return Err(ReadError::unlisted(n));
        }
    }
    for m in &manifest.members {
        let p = m.path.as_str();
        let i = entries.binary_search_by(|e| e.name.as_str().cmp(p)).map_err(|_| malformed("missing_member", p))?;
        let e = entries.get(i).ok_or_else(|| malformed("missing_member", p))?;
        if e.usize != m.length {
            return Err(ReadError::mismatch(p, "length"));
        }
    }
    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn budget_refuses_overdraft() {
        let mut b = Budget { left: 10 };
        assert!(b.charge(10).is_ok());
        assert_eq!(b.charge(1).unwrap_err().reason(), "index_budget");
    }

    #[test]
    fn flags_and_attrs() {
        assert!(check_flags(0x0800, 8).is_ok());
        assert!(check_flags(0x0806, 8).is_ok());
        assert_eq!(check_flags(0x0006, 0).unwrap_err().reason(), "flags");
        assert_eq!(check_flags(0x2000, 8).unwrap_err().reason(), "encrypted");
        assert_eq!(check_flags(0x0040, 8).unwrap_err().reason(), "encrypted");
        assert!(check_attrs(0x0314, 0o100_644 << 16).is_ok());
        assert!(check_attrs(0x0014, 0).is_ok());
        assert_eq!(check_attrs(0x0314, 0o060_644 << 16).unwrap_err().reason(), "special_file");
        assert_eq!(check_attrs(0x0314, 0o140_644 << 16).unwrap_err().reason(), "special_file");
        assert_eq!(check_attrs(0x0314, 0x08).unwrap_err().reason(), "special_file");
    }

    #[test]
    fn zip64_extra_rules() {
        let rec = |vals: &[u64]| {
            let mut v = vec![1, 0, (vals.len() * 8) as u8, 0];
            for x in vals {
                v.extend_from_slice(&x.to_le_bytes());
            }
            v
        };
        assert_eq!(zip64_values(&[], 1, 2, 3, 0).unwrap(), [1, 2, 3, 0]);
        assert_eq!(zip64_values(&rec(&[9, 8]), u32::MAX, u32::MAX, 3, 0).unwrap(), [9, 8, 3, 0]);
        assert_eq!(zip64_values(&rec(&[7]), 1, 2, u32::MAX, 0).unwrap(), [1, 2, 7, 0]);
        assert_eq!(zip64_values(&rec(&[9]), u32::MAX, u32::MAX, 3, 0).unwrap_err().reason(), "zip64_extra");
        assert_eq!(zip64_values(&[], u32::MAX, 2, 3, 0).unwrap_err().reason(), "zip64_extra");
        assert_eq!(zip64_values(&[1, 0, 9], 1, 2, 3, 0).unwrap_err().reason(), "extra_field");
        let mut two = rec(&[9, 8]);
        two.extend(rec(&[9, 8]));
        assert_eq!(zip64_values(&two, u32::MAX, u32::MAX, 3, 0).unwrap_err().reason(), "extra_field");
    }

    #[test]
    fn member_stream_reads_in_pieces() {
        let mut s = MemberStream { path: "results/a".into(), bytes: b"abcdef".to_vec(), pos: 0 };
        let mut buf = [0u8; 4];
        assert_eq!(s.read(&mut buf).unwrap(), 4);
        assert_eq!(&buf, b"abcd");
        assert_eq!(s.read(&mut buf).unwrap(), 2);
        assert_eq!(s.read(&mut buf).unwrap(), 0);
        assert_eq!((s.path(), s.len(), s.is_empty()), ("results/a", 6, false));
    }
}
