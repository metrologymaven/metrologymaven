//! The closed error set of the LAS layer (leaf Design, Errors).

use fid_io_reg::LimitKind;
use std::fmt;
use std::io;

/// The header field or structure a [`LasError::Malformed`] names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum LasField {
    /// The 4-byte file signature.
    Signature,
    /// The declared size of the public header block.
    HeaderSize,
    /// The offset of the first point record.
    OffsetToPointData,
    /// The number of VLRs: their headers do not fit before the point data.
    NumberOfVlrs,
    /// A VLR's payload length runs past the point data offset.
    VlrLength,
    /// The point data record format ID.
    PointDataFormat,
    /// The point data record length.
    PointDataRecordLength,
    /// The legacy 32-bit point count disagrees with the 64-bit one (version 1.4).
    LegacyPointCount,
    /// The offset of the waveform data packet record.
    StartOfWaveformData,
    /// The offset of the first EVLR.
    StartOfFirstEvlr,
    /// The number of EVLRs: their headers do not fit in the file.
    NumberOfEvlrs,
    /// An EVLR's payload length runs past the end of the file.
    EvlrLength,
    /// The extra-bytes descriptor record.
    ExtraBytesDescriptor,
    /// Writer input: a point batch column is missing, unexpected or of the wrong length.
    Column,
    /// Writer input: a decoded value does not fit its bit field.
    ValueRange,
    /// Writer input: the number of points written differs from the header's count.
    PointCount,
    /// Writer input: an EVLR whose payload was not loaded (waveform data) cannot be written.
    EvlrPayload,
}

impl LasField {
    /// The stable snake_case name.
    pub fn as_str(self) -> &'static str {
        match self {
            LasField::Signature => "signature",
            LasField::HeaderSize => "header_size",
            LasField::OffsetToPointData => "offset_to_point_data",
            LasField::NumberOfVlrs => "number_of_vlrs",
            LasField::VlrLength => "vlr_length",
            LasField::PointDataFormat => "point_data_format",
            LasField::PointDataRecordLength => "point_data_record_length",
            LasField::LegacyPointCount => "legacy_point_count",
            LasField::StartOfWaveformData => "start_of_waveform_data",
            LasField::StartOfFirstEvlr => "start_of_first_evlr",
            LasField::NumberOfEvlrs => "number_of_evlrs",
            LasField::EvlrLength => "evlr_length",
            LasField::ExtraBytesDescriptor => "extra_bytes_descriptor",
            LasField::Column => "column",
            LasField::ValueRange => "value_range",
            LasField::PointCount => "point_count",
            LasField::EvlrPayload => "evlr_payload",
        }
    }
}

impl fmt::Display for LasField {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// What the two numbers of a [`LasError::TruncatedFile`] count.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CountUnit {
    /// Point records: the header's count against the whole records the data holds.
    Points,
    /// Bytes: the header block needs more bytes than the file has.
    Bytes,
}

impl CountUnit {
    /// The stable name.
    pub fn as_str(self) -> &'static str {
        match self {
            CountUnit::Points => "points",
            CountUnit::Bytes => "bytes",
        }
    }
}

/// Errors of the LAS layer. Every `at` is a byte offset in the file. No variant carries
/// text taken from the file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LasError {
    /// The data does not hold what the header declares (REQ-IO.CLOUD.LAS.HDR-001). For
    /// points, `actual` is the number of whole records between the point data offset and
    /// the first EVLR (or the end of the file).
    TruncatedFile {
        /// What the header declares.
        declared: u64,
        /// What the file holds.
        actual: u64,
        /// What both numbers count.
        unit: CountUnit,
    },
    /// A field is out of range, points outside the file or overlaps another structure
    /// (REQ-IO.CLOUD.LAS.HDR-002); for the writer, its input is inconsistent.
    Malformed {
        /// The field.
        field: LasField,
    },
    /// A configured limit was exceeded. Record payloads beyond the per-record bound are
    /// reported as [`LimitKind::Alloc`] with the bound as `value`.
    LimitExceeded {
        /// Which limit.
        limit: LimitKind,
        /// Its configured value.
        value: u64,
        /// Byte offset reached.
        at: u64,
    },
    /// The version is not LAS 1.0 to 1.4.
    LasVersionUnsupported {
        /// Major version byte.
        major: u8,
        /// Minor version byte.
        minor: u8,
    },
    /// The underlying reader or writer failed.
    Io {
        /// The I/O error kind.
        kind: io::ErrorKind,
        /// Byte offset of the failed operation.
        at: u64,
    },
}

impl LasError {
    /// The stable snake_case code.
    pub fn code(&self) -> &'static str {
        match self {
            LasError::TruncatedFile { .. } => "truncated_file",
            LasError::Malformed { .. } => "malformed",
            LasError::LimitExceeded { .. } => "limit_exceeded",
            LasError::LasVersionUnsupported { .. } => "las_version_unsupported",
            LasError::Io { .. } => "io",
        }
    }

    pub(crate) fn malformed(field: LasField) -> LasError {
        LasError::Malformed { field }
    }

    pub(crate) fn io(err: &io::Error, at: u64) -> LasError {
        LasError::Io { kind: err.kind(), at }
    }
}

impl fmt::Display for LasError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LasError::TruncatedFile { declared, actual, unit } => {
                write!(f, "truncated_file: header declares {declared} {u}, file holds {actual} {u}", u = unit.as_str())
            }
            LasError::Malformed { field } => write!(f, "malformed: field {field}"),
            LasError::LimitExceeded { limit, value, at } => {
                write!(f, "limit_exceeded: {limit} limit {value} {} at byte {at}", limit.unit())
            }
            LasError::LasVersionUnsupported { major, minor } => {
                write!(f, "las_version_unsupported: version {major}.{minor}")
            }
            LasError::Io { kind, at } => write!(f, "io: {kind} at byte {at}"),
        }
    }
}

impl std::error::Error for LasError {}
