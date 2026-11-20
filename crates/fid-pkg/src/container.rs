//! The deterministic ZIP container (REQ-TRUST.PKG.MANIFEST-005, -007).
//!
//! Every entry has: general-purpose flags `0x0800` (UTF-8 names) only; modification time
//! 1980-01-01 00:00:00 (DOS time 0, date `0x0021`); no comment; "version made by" Unix 4.5
//! with external attributes of a regular file, mode 0644; and an extra field only when it
//! needs zip64 (tag `0x0001`). The archive has no comment. Zip64 is used when a size, an
//! offset or the entry count does not fit the classic fields ([`Zip64::AsNeeded`]), or for
//! every entry and the end record ([`Zip64::Always`]).
//!
//! The method comes from the media type alone ([`method_for_media_type`]): stored (0) for
//! already-compressed types, deflate (8) otherwise. Deflate output is made of RFC 1951
//! stored blocks (no Huffman coding), so it is the same bytes on every platform and every
//! deflate reader inflates it.

use crate::error::PkgError;
use crate::manifest::{
    check_media_type, check_member_path, check_members, hash_bytes, EvidenceTier, Manifest, Member, Statement,
    FORMAT_NAME, JSON_MEDIA_TYPE, MANIFEST_PATH, STATEMENT_PATH,
};
use crate::path::PkgPath;
use crate::version::FORMAT_VERSION;
use fid_types::Digest;
use std::borrow::Cow;
use std::io::Write;

/// Media types written with method 0 (stored) because they are already compressed. Every
/// other media type is written with method 8 (deflate).
pub const STORED_MEDIA_TYPES: [&str; 7] = [
    "application/gzip",
    "application/x-bzip2",
    "application/x-xz",
    "application/zip",
    "application/zstd",
    "image/jpeg",
    "image/png",
];

const SIG_LOCAL: u32 = 0x0403_4b50;
const SIG_CENTRAL: u32 = 0x0201_4b50;
const SIG_EOCD64: u32 = 0x0606_4b50;
const SIG_EOCD64_LOCATOR: u32 = 0x0706_4b50;
const SIG_EOCD: u32 = 0x0605_4b50;
const FLAG_UTF8: u16 = 0x0800;
const DOS_TIME: u16 = 0x0000;
const DOS_DATE: u16 = 0x0021;
const VERSION_ZIP64: u16 = 45;
const VERSION_DEFAULT: u16 = 20;
const MADE_BY: u16 = (3 << 8) | VERSION_ZIP64;
const EXTERNAL_ATTR: u32 = 0o100_644 << 16;
const ZIP64_TAG: u16 = 0x0001;
const U16_LIMIT: u64 = 0xFFFF;
const U32_LIMIT: u64 = 0xFFFF_FFFF;
const STORED_BLOCK_MAX: usize = 0xFFFF;

/// Compression method of an entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    /// Method 0.
    Stored,
    /// Method 8, written as stored deflate blocks.
    Deflate,
}

impl Method {
    /// The ZIP method number.
    pub fn code(self) -> u16 {
        match self {
            Method::Stored => 0,
            Method::Deflate => 8,
        }
    }
}

/// The method for a media type: [`Method::Stored`] for [`STORED_MEDIA_TYPES`], otherwise
/// [`Method::Deflate`].
pub fn method_for_media_type(media_type: &str) -> Method {
    if STORED_MEDIA_TYPES.contains(&media_type) {
        Method::Stored
    } else {
        Method::Deflate
    }
}

/// When the container uses zip64 records.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Zip64 {
    /// Only for entries and end records whose values overflow the classic fields.
    #[default]
    AsNeeded,
    /// For every entry and the end record.
    Always,
}

/// One member to package.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemberInput {
    pub path: PkgPath,
    pub media_type: String,
    pub body: Vec<u8>,
}

/// The logical contents of a package.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageInput {
    pub created_by: String,
    pub evidence_tier: EvidenceTier,
    pub profile_digest: Digest,
    pub kernel_manifest_digest: Digest,
    /// Number of results, carried in the statement's predicate.
    pub result_count: u64,
    /// In any order; the package sorts them.
    pub members: Vec<MemberInput>,
}

/// A serialised package.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuiltPackage {
    pub manifest: Manifest,
    pub manifest_bytes: Vec<u8>,
    pub statement: Statement,
    pub statement_bytes: Vec<u8>,
    /// The ZIP container.
    pub container: Vec<u8>,
}

impl BuiltPackage {
    /// Writes the container bytes; a write error is [`PkgError::Io`].
    pub fn write_to<W: Write>(&self, out: &mut W) -> Result<(), PkgError> {
        out.write_all(&self.container).map_err(|e| PkgError::Io(e.to_string()))
    }
}

/// Builds the manifest, statement and container for `input` with [`Zip64::AsNeeded`].
pub fn build_package(input: &PackageInput) -> Result<BuiltPackage, PkgError> {
    build_package_with(input, Zip64::AsNeeded)
}

/// [`build_package`] with an explicit zip64 mode.
///
/// Errors: a member outside the member folders is [`PkgError::BadPath`]; a repeated path,
/// or a path that is a folder of another, is [`PkgError::DuplicatePath`]; a bad media type
/// or an empty `created_by` is [`PkgError::SchemaInvalid`].
pub fn build_package_with(input: &PackageInput, zip64: Zip64) -> Result<BuiltPackage, PkgError> {
    let mut sorted: Vec<&MemberInput> = input.members.iter().collect();
    sorted.sort_by(|a, b| a.path.cmp(&b.path));
    let mut members = Vec::with_capacity(sorted.len());
    for m in &sorted {
        check_member_path(&m.path)?;
        check_media_type(&m.media_type)?;
        members.push(Member {
            path: m.path.clone(),
            media_type: m.media_type.clone(),
            length: m.body.len() as u64,
            sha256: hash_bytes(&m.body),
        });
    }
    check_members(&members)?;
    let manifest = Manifest {
        format: FORMAT_NAME.to_string(),
        format_version: FORMAT_VERSION,
        created_by: input.created_by.clone(),
        evidence_tier: input.evidence_tier,
        profile_digest: input.profile_digest,
        kernel_manifest_digest: input.kernel_manifest_digest,
        members,
    };
    let manifest_bytes = manifest.to_canonical_bytes()?;
    let statement = Statement::for_manifest(&manifest, &manifest_bytes, input.result_count);
    let statement_bytes = statement.to_canonical_bytes()?;

    let json = method_for_media_type(JSON_MEDIA_TYPE);
    let mut entries: Vec<Entry<'_>> = Vec::with_capacity(sorted.len() + 2);
    entries.push(Entry { name: MANIFEST_PATH, method: json, data: &manifest_bytes });
    for m in &sorted {
        entries.push(Entry { name: m.path.as_str(), method: method_for_media_type(&m.media_type), data: &m.body });
    }
    entries.push(Entry { name: STATEMENT_PATH, method: json, data: &statement_bytes });
    let container = write_zip(&entries, zip64)?;
    Ok(BuiltPackage { manifest, manifest_bytes, statement, statement_bytes, container })
}

// ------------------------------------------------------------------ ZIP writer

struct Entry<'a> {
    name: &'a str,
    method: Method,
    data: &'a [u8],
}

fn put16(out: &mut Vec<u8>, v: u16) {
    out.extend_from_slice(&v.to_le_bytes());
}

fn put32(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_le_bytes());
}

fn put64(out: &mut Vec<u8>, v: u64) {
    out.extend_from_slice(&v.to_le_bytes());
}

/// `v` as a classic 32-bit field, or the zip64 marker `0xFFFFFFFF`.
fn field32(v: u64, zip64: bool) -> u32 {
    if zip64 {
        u32::MAX
    } else {
        u32::try_from(v).unwrap_or(u32::MAX)
    }
}

/// `v` as a classic 16-bit field, or the zip64 marker `0xFFFF`.
fn field16(v: u64, zip64: bool) -> u16 {
    if zip64 {
        u16::MAX
    } else {
        u16::try_from(v).unwrap_or(u16::MAX)
    }
}

fn write_zip(entries: &[Entry<'_>], mode: Zip64) -> Result<Vec<u8>, PkgError> {
    let force = mode == Zip64::Always;
    let mut out: Vec<u8> = Vec::new();
    let mut central: Vec<u8> = Vec::new();
    for e in entries {
        let name = e.name.as_bytes();
        let name_len = u16::try_from(name.len()).map_err(|_| PkgError::bad_path(e.name, "too_long"))?;
        let comp: Cow<'_, [u8]> = match e.method {
            Method::Stored => Cow::Borrowed(e.data),
            Method::Deflate => Cow::Owned(stored_deflate(e.data)),
        };
        let crc = crc32(e.data);
        let size = e.data.len() as u64;
        let csize = comp.len() as u64;
        let offset = out.len() as u64;
        let sizes64 = force || size >= U32_LIMIT || csize >= U32_LIMIT;
        let offset64 = force || offset >= U32_LIMIT;
        let needed = if sizes64 || offset64 { VERSION_ZIP64 } else { VERSION_DEFAULT };

        // Local file header.
        put32(&mut out, SIG_LOCAL);
        put16(&mut out, needed);
        put16(&mut out, FLAG_UTF8);
        put16(&mut out, e.method.code());
        put16(&mut out, DOS_TIME);
        put16(&mut out, DOS_DATE);
        put32(&mut out, crc);
        put32(&mut out, field32(csize, sizes64));
        put32(&mut out, field32(size, sizes64));
        put16(&mut out, name_len);
        put16(&mut out, if sizes64 { 20 } else { 0 });
        out.extend_from_slice(name);
        if sizes64 {
            put16(&mut out, ZIP64_TAG);
            put16(&mut out, 16);
            put64(&mut out, size);
            put64(&mut out, csize);
        }
        out.extend_from_slice(&comp);

        // Central directory header.
        let mut extra = Vec::new();
        if sizes64 {
            put64(&mut extra, size);
            put64(&mut extra, csize);
        }
        if offset64 {
            put64(&mut extra, offset);
        }
        let mut extra_field = Vec::new();
        if !extra.is_empty() {
            put16(&mut extra_field, ZIP64_TAG);
            put16(&mut extra_field, field16(extra.len() as u64, false));
            extra_field.extend_from_slice(&extra);
        }
        put32(&mut central, SIG_CENTRAL);
        put16(&mut central, MADE_BY);
        put16(&mut central, needed);
        put16(&mut central, FLAG_UTF8);
        put16(&mut central, e.method.code());
        put16(&mut central, DOS_TIME);
        put16(&mut central, DOS_DATE);
        put32(&mut central, crc);
        put32(&mut central, field32(csize, sizes64));
        put32(&mut central, field32(size, sizes64));
        put16(&mut central, name_len);
        put16(&mut central, field16(extra_field.len() as u64, false));
        put16(&mut central, 0); // comment length
        put16(&mut central, 0); // disk number start
        put16(&mut central, 0); // internal attributes
        put32(&mut central, EXTERNAL_ATTR);
        put32(&mut central, field32(offset, offset64));
        central.extend_from_slice(name);
        central.extend_from_slice(&extra_field);
    }

    let cd_offset = out.len() as u64;
    let cd_size = central.len() as u64;
    let count = entries.len() as u64;
    out.extend_from_slice(&central);
    let end64 = force || count >= U16_LIMIT || cd_size >= U32_LIMIT || cd_offset >= U32_LIMIT;
    if end64 {
        let record_offset = out.len() as u64;
        put32(&mut out, SIG_EOCD64);
        put64(&mut out, 44);
        put16(&mut out, VERSION_ZIP64);
        put16(&mut out, VERSION_ZIP64);
        put32(&mut out, 0);
        put32(&mut out, 0);
        put64(&mut out, count);
        put64(&mut out, count);
        put64(&mut out, cd_size);
        put64(&mut out, cd_offset);
        put32(&mut out, SIG_EOCD64_LOCATOR);
        put32(&mut out, 0);
        put64(&mut out, record_offset);
        put32(&mut out, 1);
    }
    put32(&mut out, SIG_EOCD);
    put16(&mut out, 0);
    put16(&mut out, 0);
    put16(&mut out, field16(count, end64));
    put16(&mut out, field16(count, end64));
    put32(&mut out, field32(cd_size, end64));
    put32(&mut out, field32(cd_offset, end64));
    put16(&mut out, 0); // archive comment length
    Ok(out)
}

/// A deflate stream of stored blocks (RFC 1951 BTYPE 00), at most 65535 bytes each; empty
/// input is one empty final block.
fn stored_deflate(data: &[u8]) -> Vec<u8> {
    if data.is_empty() {
        return vec![0x01, 0x00, 0x00, 0xff, 0xff];
    }
    let mut out = Vec::with_capacity(data.len() + 5 * (data.len() / STORED_BLOCK_MAX + 1));
    let mut chunks = data.chunks(STORED_BLOCK_MAX).peekable();
    while let Some(chunk) = chunks.next() {
        let last = chunks.peek().is_none();
        let len = u16::try_from(chunk.len()).unwrap_or(u16::MAX);
        out.push(u8::from(last));
        put16(&mut out, len);
        put16(&mut out, !len);
        out.extend_from_slice(chunk);
    }
    out
}

/// Slicing-by-8 tables: `CRC_TABLES[0]` is the classic byte table; `CRC_TABLES[k][i]` is the
/// CRC state after byte `i` followed by `k` zero bytes.
const CRC_TABLES: [[u32; 256]; 8] = crc_tables();

const fn crc_tables() -> [[u32; 256]; 8] {
    let mut t = [[0u32; 256]; 8];
    let mut i = 0;
    while i < 256 {
        let mut c = i as u32;
        let mut k = 0;
        while k < 8 {
            c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
            k += 1;
        }
        t[0][i] = c;
        i += 1;
    }
    let mut n = 1;
    while n < 8 {
        let mut i = 0;
        while i < 256 {
            let prev = t[n - 1][i];
            t[n][i] = t[0][(prev & 0xff) as usize] ^ (prev >> 8);
            i += 1;
        }
        n += 1;
    }
    t
}

/// CRC-32 (ISO-HDLC, reflected polynomial `0xEDB88320`) as used by ZIP.
pub fn crc32(data: &[u8]) -> u32 {
    crc32_update(0, data)
}

/// Continues a CRC-32: `crc32_update(crc32(a), b) == crc32(a ++ b)`.
pub(crate) fn crc32_update(crc: u32, data: &[u8]) -> u32 {
    let t = &CRC_TABLES;
    let mut c = !crc;
    let mut chunks = data.chunks_exact(8);
    for b in &mut chunks {
        let lo = c ^ u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
        c = t[7][(lo & 0xff) as usize]
            ^ t[6][((lo >> 8) & 0xff) as usize]
            ^ t[5][((lo >> 16) & 0xff) as usize]
            ^ t[4][(lo >> 24) as usize]
            ^ t[3][usize::from(b[4])]
            ^ t[2][usize::from(b[5])]
            ^ t[1][usize::from(b[6])]
            ^ t[0][usize::from(b[7])];
    }
    for &b in chunks.remainder() {
        c = t[0][((c ^ u32::from(b)) & 0xff) as usize] ^ (c >> 8);
    }
    !c
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn crc32_check_value() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
        assert_eq!(crc32(b""), 0);
        // Slicing-by-8 agrees with the bytewise definition at every length and split.
        let data: Vec<u8> = (0..100u32).map(|i| (i * 37 + 11) as u8).collect();
        let bytewise = |d: &[u8]| {
            let mut c = !0u32;
            for &b in d {
                c = CRC_TABLES[0][((c ^ u32::from(b)) & 0xff) as usize] ^ (c >> 8);
            }
            !c
        };
        for n in 0..data.len() {
            assert_eq!(crc32(&data[..n]), bytewise(&data[..n]), "{n}");
            assert_eq!(crc32_update(crc32(&data[..n]), &data[n..]), crc32(&data), "{n}");
        }
    }

    #[test]
    fn stored_blocks() {
        assert_eq!(stored_deflate(b""), [1, 0, 0, 0xff, 0xff]);
        assert_eq!(stored_deflate(b"ab"), [1, 2, 0, 0xfd, 0xff, b'a', b'b']);
        let big = vec![7u8; STORED_BLOCK_MAX + 3];
        let d = stored_deflate(&big);
        assert_eq!(d.len(), big.len() + 10);
        assert_eq!(&d[..5], &[0, 0xff, 0xff, 0, 0]);
        let second = 5 + STORED_BLOCK_MAX;
        assert_eq!(&d[second..second + 5], &[1, 3, 0, 0xfc, 0xff]);
        let exact = stored_deflate(&big[..STORED_BLOCK_MAX]);
        assert_eq!(exact[0], 1);
        assert_eq!(exact.len(), STORED_BLOCK_MAX + 5);
    }

    #[test]
    fn methods() {
        assert_eq!(method_for_media_type("application/zstd"), Method::Stored);
        assert_eq!(method_for_media_type("application/json"), Method::Deflate);
        assert_eq!(method_for_media_type("application/octet-stream"), Method::Deflate);
        let mut sorted = STORED_MEDIA_TYPES;
        sorted.sort_unstable();
        assert_eq!(sorted, STORED_MEDIA_TYPES);
    }
}
