//! Typed errors for mesh building (spec node `IO.MESH.MBLD`).

use fid_geo_types::GeoTypeError;
use fid_io_reg::{IoError, Position};
use std::fmt;

/// Errors returned while feeding or finalising a [`crate::MeshBuilder`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MeshBuildError {
    /// A face referenced a vertex index outside `0..vertex_count` (REQ-IO.MESH.MBLD-003).
    MalformedFace {
        /// Zero-based index of the offending face in input order.
        face: u64,
        /// The out-of-range vertex index found in that face.
        vertex_index: u64,
        /// Number of vertices present in the builder.
        vertex_count: u64,
    },
    /// A vertex coordinate was NaN or infinite.
    NonFiniteVertex {
        /// Zero-based index of the offending vertex.
        vertex: u64,
    },
    /// A per-vertex attribute column did not match the vertex count.
    AttrLengthMismatch {
        /// Attribute channel name.
        name: String,
        /// Expected number of entries (vertex count).
        expected: u64,
        /// Actual number of entries present in the column.
        found: u64,
    },
    /// A custom vertex attribute column was appended with a different scalar type than declared.
    AttrTypeMismatch {
        /// Custom attribute name.
        name: String,
    },
    /// A GEO geometry invariant failed during finalisation (e.g. non-unit normal).
    Geo(GeoTypeError),
    /// A reader harness budget or I/O limit was exceeded (`limit_exceeded`, etc.).
    Io(IoError),
    /// Host memory allocation failed while growing an internal buffer.
    AllocationFailed {
        /// Bytes requested when allocation failed.
        needed: u64,
    },
}

impl MeshBuildError {
    /// Stable snake_case error code aligned with `fid-io-reg` (`IO.REG.LIMITS`).
    pub fn code(&self) -> &'static str {
        match self {
            MeshBuildError::MalformedFace { .. }
            | MeshBuildError::NonFiniteVertex { .. }
            | MeshBuildError::AttrLengthMismatch { .. }
            | MeshBuildError::AttrTypeMismatch { .. }
            | MeshBuildError::Geo(_) => "malformed",
            MeshBuildError::Io(err) => err.code(),
            MeshBuildError::AllocationFailed { .. } => "limit_exceeded",
        }
    }

    /// The offending zero-based face index, when this is [`MeshBuildError::MalformedFace`].
    pub fn face(&self) -> Option<u64> {
        match self {
            MeshBuildError::MalformedFace { face, .. } => Some(*face),
            _ => None,
        }
    }

    /// Projects this builder error onto [`IoError`] at input position `at`.
    pub fn to_io_error(&self, at: Position) -> IoError {
        match self {
            MeshBuildError::MalformedFace { .. } => IoError::Malformed { at, reason: "face_index_out_of_range" },
            MeshBuildError::NonFiniteVertex { .. } => IoError::Malformed { at, reason: "non_finite_vertex" },
            MeshBuildError::AttrLengthMismatch { .. } => IoError::Malformed { at, reason: "attr_length_mismatch" },
            MeshBuildError::AttrTypeMismatch { .. } => IoError::Malformed { at, reason: "attr_type_mismatch" },
            MeshBuildError::Geo(_) => IoError::Malformed { at, reason: "invalid_geometry" },
            MeshBuildError::Io(err) => err.clone(),
            MeshBuildError::AllocationFailed { needed } => {
                IoError::LimitExceeded { limit: fid_io_reg::LimitKind::Alloc, value: *needed, at }
            }
        }
    }
}

impl From<IoError> for MeshBuildError {
    fn from(err: IoError) -> Self {
        MeshBuildError::Io(err)
    }
}

impl From<GeoTypeError> for MeshBuildError {
    fn from(err: GeoTypeError) -> Self {
        MeshBuildError::Geo(err)
    }
}

impl fmt::Display for MeshBuildError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MeshBuildError::MalformedFace { face, vertex_index, vertex_count } => write!(
                f,
                "malformed{{face: {face}}}: vertex index {vertex_index} out of range (vertex_count = {vertex_count})"
            ),
            MeshBuildError::NonFiniteVertex { vertex } => {
                write!(f, "malformed: non-finite coordinate at vertex {vertex}")
            }
            MeshBuildError::AttrLengthMismatch { name, expected, found } => {
                write!(f, "malformed: attribute '{name}' has {found} values, expected {expected}")
            }
            MeshBuildError::AttrTypeMismatch { name } => {
                write!(f, "malformed: custom attribute '{name}' scalar type mismatch")
            }
            MeshBuildError::Geo(err) => write!(f, "malformed: {err}"),
            MeshBuildError::Io(err) => write!(f, "{err}"),
            MeshBuildError::AllocationFailed { needed } => {
                write!(f, "limit_exceeded: allocation failed for {needed} bytes")
            }
        }
    }
}

impl std::error::Error for MeshBuildError {}
