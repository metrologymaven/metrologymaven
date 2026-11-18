//! The physical layer's closed error set (leaf Design, Errors).

use fid_io_reg::LimitKind;
use std::fmt;
use std::io;

/// The header field a [`PhysError::Malformed`] names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum HeaderField {
    /// The 8-byte file signature.
    Signature,
    /// The major format version.
    MajorVersion,
    /// The page size.
    PageSize,
    /// The declared physical file length.
    FileLength,
    /// The physical offset of the XML section.
    XmlOffset,
    /// The logical length of the XML section.
    XmlLength,
}

impl HeaderField {
    /// The stable snake_case name.
    pub fn as_str(self) -> &'static str {
        match self {
            HeaderField::Signature => "signature",
            HeaderField::MajorVersion => "major_version",
            HeaderField::PageSize => "page_size",
            HeaderField::FileLength => "file_length",
            HeaderField::XmlOffset => "xml_offset",
            HeaderField::XmlLength => "xml_length",
        }
    }
}

impl fmt::Display for HeaderField {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Errors of the E57 physical layer. Every offset is a physical byte offset in the file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PhysError {
    /// A page's stored checksum does not match its payload (REQ-IO.CLOUD.E57.PHYS-001).
    ChecksumMismatch {
        /// Physical offset of the page's first byte.
        offset: u64,
    },
    /// A header field is wrong or inconsistent with the file (REQ-IO.CLOUD.E57.PHYS-002).
    Malformed {
        /// The field.
        field: HeaderField,
    },
    /// The file, or a requested logical range, ends before the bytes asked for.
    TruncatedFile {
        /// Physical offset where the data ran out.
        at: u64,
    },
    /// A configured limit was exceeded.
    LimitExceeded {
        /// Which limit.
        limit: LimitKind,
        /// Its configured value.
        value: u64,
        /// Physical offset reached.
        at: u64,
    },
    /// The underlying reader or writer failed.
    Io {
        /// The I/O error kind.
        kind: io::ErrorKind,
        /// Physical offset of the failed operation.
        at: u64,
    },
}

impl PhysError {
    /// The stable snake_case code.
    pub fn code(&self) -> &'static str {
        match self {
            PhysError::ChecksumMismatch { .. } => "checksum_mismatch",
            PhysError::Malformed { .. } => "malformed",
            PhysError::TruncatedFile { .. } => "truncated_file",
            PhysError::LimitExceeded { .. } => "limit_exceeded",
            PhysError::Io { .. } => "io",
        }
    }

    pub(crate) fn malformed(field: HeaderField) -> PhysError {
        PhysError::Malformed { field }
    }

    pub(crate) fn from_io(err: &io::Error, at: u64) -> PhysError {
        match err.kind() {
            io::ErrorKind::UnexpectedEof => PhysError::TruncatedFile { at },
            kind => PhysError::Io { kind, at },
        }
    }
}

impl fmt::Display for PhysError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PhysError::ChecksumMismatch { offset } => write!(f, "checksum_mismatch: page at byte {offset}"),
            PhysError::Malformed { field } => write!(f, "malformed: header field {field}"),
            PhysError::TruncatedFile { at } => write!(f, "truncated_file: at byte {at}"),
            PhysError::LimitExceeded { limit, value, at } => {
                write!(f, "limit_exceeded: {limit} limit {value} {} at byte {at}", limit.unit())
            }
            PhysError::Io { kind, at } => write!(f, "io: {kind} at byte {at}"),
        }
    }
}

impl std::error::Error for PhysError {}
