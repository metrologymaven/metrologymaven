//! The fixed file header and the page layout it declares.
//!
//! Header, 48 bytes, little-endian, at logical (and physical) offset 0 \[unverified: field
//! list and order to confirm against ASTM E2807\]:
//!
//! | bytes | field |
//! |---|---|
//! | 0..8 | signature `ASTM-E57` |
//! | 8..12 | major version (u32) |
//! | 12..16 | minor version (u32) |
//! | 16..24 | physical file length (u64) |
//! | 24..32 | XML section physical offset (u64) |
//! | 32..40 | XML section logical length (u64) |
//! | 40..48 | page size (u64) |
//!
//! Every page holds `page_size - 4` payload bytes followed by the CRC-32C of that payload,
//! stored big-endian \[unverified: byte order\].

use crate::error::{HeaderField, PhysError};
use fid_io_reg::LimitKind;

/// The file signature.
pub const SIGNATURE: [u8; 8] = *b"ASTM-E57";
/// Length of the file header in bytes.
pub const HEADER_LEN: usize = 48;
/// Length of the checksum at the end of every page.
pub const CHECKSUM_LEN: u64 = 4;
/// The page size [`crate::LogicalWriter`] users normally choose.
pub const DEFAULT_PAGE_SIZE: u64 = 1024;
/// Smallest accepted page size: page 0's payload must hold the whole header.
pub const MIN_PAGE_SIZE: u64 = HEADER_LEN as u64 + CHECKSUM_LEN;
/// Largest accepted page size (1 MiB): one page is buffered whole, so this bounds that
/// allocation (`limit_exceeded{alloc}` above it).
pub const MAX_PAGE_SIZE: u64 = 1 << 20;
/// The major version this crate reads and writes \[unverified\].
pub const MAJOR_VERSION: u32 = 1;
/// The minor version this crate writes \[unverified\].
pub const MINOR_VERSION: u32 = 0;

/// Byte offset of the page-size field, reported by `limit_exceeded{alloc}`.
const PAGE_SIZE_FIELD_AT: u64 = 40;

/// The decoded file header (signature already checked).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct E57Header {
    /// Major format version.
    pub major_version: u32,
    /// Minor format version.
    pub minor_version: u32,
    /// Physical file length in bytes, a whole number of pages.
    pub file_length: u64,
    /// Physical offset of the first byte of the XML section.
    pub xml_offset: u64,
    /// Logical length of the XML section in bytes.
    pub xml_length: u64,
    /// Page size in bytes, checksum included.
    pub page_size: u64,
}

fn u32_at(b: &[u8; HEADER_LEN], at: usize) -> u32 {
    let mut a = [0u8; 4];
    if let Some(s) = b.get(at..at + 4) {
        a.copy_from_slice(s);
    }
    u32::from_le_bytes(a)
}

fn u64_at(b: &[u8; HEADER_LEN], at: usize) -> u64 {
    let mut a = [0u8; 8];
    if let Some(s) = b.get(at..at + 8) {
        a.copy_from_slice(s);
    }
    u64::from_le_bytes(a)
}

impl E57Header {
    /// Decodes the header bytes; fails `malformed{signature}` if the signature is wrong.
    /// No other field is checked here; see [`validate`].
    pub fn parse(bytes: &[u8; HEADER_LEN]) -> Result<E57Header, PhysError> {
        if bytes.get(..8) != Some(&SIGNATURE[..]) {
            return Err(PhysError::malformed(HeaderField::Signature));
        }
        Ok(E57Header {
            major_version: u32_at(bytes, 8),
            minor_version: u32_at(bytes, 12),
            file_length: u64_at(bytes, 16),
            xml_offset: u64_at(bytes, 24),
            xml_length: u64_at(bytes, 32),
            page_size: u64_at(bytes, 40),
        })
    }

    /// Encodes the header.
    pub fn encode(&self) -> [u8; HEADER_LEN] {
        let mut out = Vec::with_capacity(HEADER_LEN);
        out.extend_from_slice(&SIGNATURE);
        out.extend_from_slice(&self.major_version.to_le_bytes());
        out.extend_from_slice(&self.minor_version.to_le_bytes());
        out.extend_from_slice(&self.file_length.to_le_bytes());
        out.extend_from_slice(&self.xml_offset.to_le_bytes());
        out.extend_from_slice(&self.xml_length.to_le_bytes());
        out.extend_from_slice(&self.page_size.to_le_bytes());
        let mut fixed = [0u8; HEADER_LEN];
        if let Some(src) = out.get(..HEADER_LEN) {
            fixed.copy_from_slice(src);
        }
        fixed
    }
}

/// The page geometry of a file: page size, payload per page and page count.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PageLayout {
    page_size: u64,
    payload: u64,
    page_count: u64,
}

impl PageLayout {
    /// A layout for `page_size` with `page_count` pages. Fails `malformed{page_size}` below
    /// [`MIN_PAGE_SIZE`], `limit_exceeded{alloc}` above [`MAX_PAGE_SIZE`], and
    /// `limit_exceeded{bytes}` if the physical length overflows `u64`.
    pub fn new(page_size: u64, page_count: u64) -> Result<PageLayout, PhysError> {
        if page_size < MIN_PAGE_SIZE {
            return Err(PhysError::malformed(HeaderField::PageSize));
        }
        if page_size > MAX_PAGE_SIZE {
            return Err(PhysError::LimitExceeded {
                limit: LimitKind::Alloc,
                value: MAX_PAGE_SIZE,
                at: PAGE_SIZE_FIELD_AT,
            });
        }
        if page_count.checked_mul(page_size).is_none() {
            return Err(PhysError::LimitExceeded { limit: LimitKind::Bytes, value: u64::MAX, at: 0 });
        }
        Ok(PageLayout { page_size, payload: page_size - CHECKSUM_LEN, page_count })
    }

    /// Page size in bytes, checksum included.
    pub fn page_size(&self) -> u64 {
        self.page_size
    }

    /// Payload bytes per page (`page_size - 4`).
    pub fn payload(&self) -> u64 {
        self.payload
    }

    /// Number of pages.
    pub fn page_count(&self) -> u64 {
        self.page_count
    }

    /// Physical length in bytes (`page_count × page_size`; cannot overflow, see [`PageLayout::new`]).
    pub fn physical_length(&self) -> u64 {
        self.page_count.saturating_mul(self.page_size)
    }

    /// Logical length in bytes (`page_count × payload`), padding of the last page included.
    pub fn logical_length(&self) -> u64 {
        self.page_count.saturating_mul(self.payload)
    }

    /// Maps logical offset `l` to its physical offset:
    /// `(l / payload) × page_size + l mod payload`. `None` on overflow.
    pub fn physical_of(&self, logical: u64) -> Option<u64> {
        (logical / self.payload).checked_mul(self.page_size)?.checked_add(logical % self.payload)
    }

    /// Maps physical offset `p` to its logical offset. `None` if `p` falls inside a page's
    /// checksum.
    pub fn logical_of(&self, physical: u64) -> Option<u64> {
        let within = physical % self.page_size;
        if within >= self.payload {
            return None;
        }
        (physical / self.page_size).checked_mul(self.payload)?.checked_add(within)
    }
}

/// Checks a decoded header against the actual file length and returns the page layout.
///
/// Order of checks (the first failure is reported): major version, page size, file length
/// (equal to the actual length and a whole number of pages), XML offset (inside the file, not
/// inside a checksum, not inside the header), XML length (the section ends within the
/// logical stream).
pub fn validate(h: &E57Header, actual_len: u64) -> Result<PageLayout, PhysError> {
    if h.major_version != MAJOR_VERSION {
        return Err(PhysError::malformed(HeaderField::MajorVersion));
    }
    // Page size first, so the remaining checks may divide by it.
    PageLayout::new(h.page_size, 0)?;
    if h.file_length != actual_len || h.file_length % h.page_size != 0 {
        return Err(PhysError::malformed(HeaderField::FileLength));
    }
    let layout = PageLayout::new(h.page_size, h.file_length / h.page_size)?;
    if h.xml_offset >= h.file_length {
        return Err(PhysError::malformed(HeaderField::XmlOffset));
    }
    let xml_logical = match layout.logical_of(h.xml_offset) {
        Some(l) if l >= HEADER_LEN as u64 => l,
        _ => return Err(PhysError::malformed(HeaderField::XmlOffset)),
    };
    match xml_logical.checked_add(h.xml_length) {
        Some(end) if end <= layout.logical_length() => Ok(layout),
        _ => Err(PhysError::malformed(HeaderField::XmlLength)),
    }
}
