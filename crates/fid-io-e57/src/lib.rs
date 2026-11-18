//! E57 point-cloud files, pure Rust (spec node IO.CLOUD.E57 and its leaves).
//!
//! This crate currently holds the bottom layer, IO.CLOUD.E57.PHYS: the fixed file header
//! and the paged physical layout in which every page ends with a CRC-32C of its payload.
//! [`LogicalReader`] turns physical offsets into a logical byte stream, verifying each
//! page's checksum once and keeping verified pages in a byte-bounded [`PageCache`];
//! [`LogicalWriter`] appends logical bytes, emits full pages with checksums and patches the
//! header at finish. Everything above (XML, binary sections) works on logical bytes.
//!
//! The format follows ASTM E2807, cited by designator only. The header field list and the
//! byte order of the stored checksum are implemented from the leaf's description and are
//! \[unverified\] against the standard and a real corpus file (the leaf's AC-4).
//!
//! All input is hostile: every size and offset from the header is checked against the
//! actual file length before any page is read, page arithmetic is checked for overflow, and
//! no code path panics.

mod cache;
mod crc;
mod error;
mod header;
mod reader;
mod writer;

pub use cache::{PageCache, DEFAULT_CACHE_BUDGET};
pub use crc::{crc32c, crc32c_update};
pub use error::{HeaderField, PhysError};
pub use header::{
    validate, E57Header, PageLayout, CHECKSUM_LEN, DEFAULT_PAGE_SIZE, HEADER_LEN, MAJOR_VERSION, MAX_PAGE_SIZE,
    MINOR_VERSION, MIN_PAGE_SIZE, SIGNATURE,
};
pub use reader::{LogicalReader, ReaderOptions};
pub use writer::LogicalWriter;
