//! Typed errors of the chunk layout, the byte formats and the streaming writer.

use core::fmt;
use fid_geo_types::GeoTypeError;

/// What is wrong with the bytes of a chunk or a manifest (inside
/// [`OocError::CorruptChunk`] and [`OocError::CorruptManifest`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fault {
    /// The bytes do not start with the format tag.
    BadMagic,
    /// The bytes end before the format says they do: `needed` bytes, `found` present.
    Truncated { needed: u64, found: u64 },
    /// `extra` bytes follow the last field.
    TrailingBytes { extra: u64 },
    /// The bytes are (or declare) `needed` bytes, more than the reader's `limit`.
    /// `needed` is `u64::MAX` when the declared size overflows.
    Oversized { needed: u64, limit: u64 },
    /// A cell code is not a valid [`crate::CellCode`].
    BadCell { code: u64 },
    /// A chunk with zero points (never written).
    ZeroCount,
    /// The channel list names an unknown channel, gives a wrong type byte, or is not in
    /// strictly ascending name order.
    BadChannel,
    /// Original indexes are not strictly ascending at position `at` of the chunk.
    IndexOrder { at: u64 },
    /// A NaN in `column` at position `at`.
    NaN { column: &'static str, at: u64 },
    /// An infinite coordinate or normal component in `column` at position `at`.
    NonFinite { column: &'static str, at: u64 },
    /// A column of the chunk has a length different from its point count.
    ColumnLength { column: &'static str },
    /// A chunk carries cloud-level attributes (scan origins or grid), which live in the
    /// manifest.
    CloudAttributeInChunk,
    /// The chunk's cell differs from its manifest entry.
    WrongCell { expected: u64, found: u64 },
    /// The chunk's point count differs from its manifest entry.
    WrongCount { expected: u64, found: u64 },
    /// The chunk's channel list differs from the manifest's.
    WrongChannels,
    /// A blob has a byte length different from its manifest entry.
    WrongSize { expected: u64, found: u64 },
    /// An original index is not below the cloud's point count.
    IndexOutOfRange { index: u64 },
    /// An original index occurs in two chunks.
    DuplicateIndex { index: u64 },
    /// The point with original index `index` does not lie in its chunk's cell.
    PointOutsideCell { index: u64 },
    /// The manifest's bounding box is invalid (non-finite, inverted, or with an extent
    /// that overflows).
    BadBounds,
    /// The manifest's octree parameters are outside their valid range.
    BadParams,
    /// The manifest's scan origins are non-finite, repeated or out of ascending ID order.
    BadScanOrigins,
    /// The manifest's grid flag is not 0 or 1, or `rows * cols` differs from the count.
    BadGrid,
    /// Chunk entries are not in strictly ascending Morton order, or their cells overlap.
    ChunkOrder { at: u64 },
    /// A chunk entry above `max_chunk_points` that is not at the depth limit.
    OverfullChunk { at: u64 },
    /// The chunk counts do not add up to the manifest's point count.
    CountMismatch,
    /// The manifest digest is not the SHA-256 of the manifest bytes.
    DigestMismatch,
}

/// Every error of this crate. Errors are values; nothing here panics (CONSTITUTION §8).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OocError {
    /// The operation would need `needed` bytes of memory, more than its budget
    /// (REQ-GEO.OOC.LAYOUT-005). Returned before anything is allocated.
    BudgetExceeded { needed: u64 },
    /// The writer's spill budget cannot hold one chunk plus its fixed buffers; `needed`
    /// is the smallest budget that can.
    BudgetTooSmall { needed: u64 },
    /// A chunk carries a version byte this reader does not know (REQ-GEO.OOC.LAYOUT-006).
    UnsupportedChunkVersion { found: u8 },
    /// A manifest carries a version byte this reader does not know.
    UnsupportedManifestVersion { found: u8 },
    /// The bytes of a chunk are malformed or disagree with the manifest.
    CorruptChunk { fault: Fault },
    /// The bytes or fields of a manifest are malformed.
    CorruptManifest { fault: Fault },
    /// Layout parameters outside their range (`what` names the parameter).
    InvalidParams { what: &'static str },
    /// A cloud with no points has no bounding box and is not chunked.
    EmptyCloud,
    /// The extent of the bounds on `axis` overflows `f64`.
    BoundsTooLarge { axis: u8 },
    /// Point `index` lies outside the bounds given for the partition.
    OutOfBounds { index: u64 },
    /// A NaN value in `channel` at point `index`; chunks refuse NaN in every float field.
    NaNValue { channel: &'static str, index: u64 },
    /// Point `index` given to the writer does not carry exactly the declared channels.
    ChannelMismatch { index: u64 },
    /// The partition has more chunks than `limit`.
    TooManyChunks { limit: u64 },
    /// The cloud rebuilt from chunks does not have the manifest's original digest.
    DigestMismatch,
    /// A geometry validation error (coordinates, normals, scan origins, grid).
    Geometry(GeoTypeError),
    /// An error of the blob store (`code` is its stable code).
    Store { code: &'static str, detail: String },
    /// An I/O error on the writer's spill files.
    Spill { kind: std::io::ErrorKind, detail: String },
    /// A memory allocation of `needed` bytes failed.
    AllocationFailed { needed: u64 },
    /// An earlier I/O error left this writer unusable.
    WriterFailed,
}

impl OocError {
    /// A stable machine-readable code.
    pub fn code(&self) -> &'static str {
        match self {
            OocError::BudgetExceeded { .. } => "budget_exceeded",
            OocError::BudgetTooSmall { .. } => "budget_too_small",
            OocError::UnsupportedChunkVersion { .. } => "unsupported_chunk_version",
            OocError::UnsupportedManifestVersion { .. } => "unsupported_manifest_version",
            OocError::CorruptChunk { .. } => "corrupt_chunk",
            OocError::CorruptManifest { .. } => "corrupt_manifest",
            OocError::InvalidParams { .. } => "invalid_params",
            OocError::EmptyCloud => "empty_cloud",
            OocError::BoundsTooLarge { .. } => "bounds_too_large",
            OocError::OutOfBounds { .. } => "out_of_bounds",
            OocError::NaNValue { .. } => "nan_value",
            OocError::ChannelMismatch { .. } => "channel_mismatch",
            OocError::TooManyChunks { .. } => "too_many_chunks",
            OocError::DigestMismatch => "digest_mismatch",
            OocError::Geometry(_) => "geometry",
            OocError::Store { .. } => "store",
            OocError::Spill { .. } => "spill",
            OocError::AllocationFailed { .. } => "allocation_failed",
            OocError::WriterFailed => "writer_failed",
        }
    }
}

impl fmt::Display for OocError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OocError::BudgetExceeded { needed } => write!(f, "budget_exceeded: needs {needed} bytes"),
            OocError::BudgetTooSmall { needed } => write!(f, "budget_too_small: needs at least {needed} bytes"),
            OocError::UnsupportedChunkVersion { found } => write!(f, "unsupported_chunk_version: {found}"),
            OocError::UnsupportedManifestVersion { found } => write!(f, "unsupported_manifest_version: {found}"),
            OocError::CorruptChunk { fault } => write!(f, "corrupt_chunk: {fault:?}"),
            OocError::CorruptManifest { fault } => write!(f, "corrupt_manifest: {fault:?}"),
            OocError::InvalidParams { what } => write!(f, "invalid_params: {what}"),
            OocError::EmptyCloud => write!(f, "empty_cloud"),
            OocError::BoundsTooLarge { axis } => write!(f, "bounds_too_large: axis {axis}"),
            OocError::OutOfBounds { index } => write!(f, "out_of_bounds: point {index}"),
            OocError::NaNValue { channel, index } => write!(f, "nan_value: {channel} at point {index}"),
            OocError::ChannelMismatch { index } => write!(f, "channel_mismatch: point {index}"),
            OocError::TooManyChunks { limit } => write!(f, "too_many_chunks: limit {limit}"),
            OocError::DigestMismatch => write!(f, "digest_mismatch"),
            OocError::Geometry(e) => write!(f, "geometry: {e}"),
            OocError::Store { code, detail } => write!(f, "store: {code}: {detail}"),
            OocError::Spill { kind, detail } => write!(f, "spill: {kind:?}: {detail}"),
            OocError::AllocationFailed { needed } => write!(f, "allocation_failed: {needed} bytes"),
            OocError::WriterFailed => write!(f, "writer_failed"),
        }
    }
}

impl std::error::Error for OocError {}

impl From<GeoTypeError> for OocError {
    fn from(e: GeoTypeError) -> OocError {
        OocError::Geometry(e)
    }
}

pub(crate) fn chunk_fault(fault: Fault) -> OocError {
    OocError::CorruptChunk { fault }
}

pub(crate) fn manifest_fault(fault: Fault) -> OocError {
    OocError::CorruptManifest { fault }
}

pub(crate) fn spill(e: std::io::Error) -> OocError {
    OocError::Spill { kind: e.kind(), detail: e.to_string() }
}
