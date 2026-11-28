//! The errors of the oriented-cloud target, the overlap fraction, stride sampling and the
//! pair graph.

use core::fmt;

use fid_geo_frame::GeoError;
use fid_geo_index::kdtree::PointIndexError;

use super::graph::PairGraph;

/// Why a cloud cannot be a projection target (`scan.cloud.no_normals`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoNormalsReason {
    /// The cloud carries no normal channel at all.
    NoNormalChannel,
    /// The cloud has normals, but the valid-normal mask marks none of its points.
    NoneMarkedValid,
}

impl NoNormalsReason {
    /// A stable machine-readable reason.
    pub fn code(self) -> &'static str {
        match self {
            NoNormalsReason::NoNormalChannel => "no_normal_channel",
            NoNormalsReason::NoneMarkedValid => "none_marked_valid",
        }
    }
}

/// Every error of this module. Errors are values; nothing panics.
#[derive(Debug, Clone, PartialEq)]
pub enum RegError {
    /// The target cloud has no point with a valid normal (REQ-002).
    NoNormals {
        /// Which of the two cases it is.
        reason: NoNormalsReason,
    },
    /// A parameter or input is outside its domain.
    InvalidParameter {
        /// The parameter (`d`, `max_samples`, `m`, `threads`, `valid`, `cloud`, `moving`,
        /// `pose`).
        name: &'static str,
        /// What is wrong with it, in words.
        reason: String,
    },
    /// The moving cloud and the target are in different frames. Nothing was computed; no
    /// frame is ever converted here (transform the element first with GEO.FRAME.APPLY).
    FrameMismatch(GeoError),
    /// The GEO.INDEX.KDTREE build or a query failed; the upstream error is kept as is.
    Index(PointIndexError),
    /// A working allocation failed.
    AllocationFailed {
        /// Bytes requested (saturating).
        bytes: u64,
    },
    /// A worker thread or an internal invariant failed (never expected).
    Inconsistent,
    /// The pair graph has more than one connected component
    /// (`scan.cloud.reg_disconnected`, REQ-SCAN.CLOUD.REG.GRAPH-003): some scans cannot be
    /// tied to the rest at this minimum overlap.
    Disconnected {
        /// Each component as a sorted list of scan indexes, ordered by smallest index.
        components: Vec<Vec<u32>>,
        /// The whole graph (matrix, edges, components), for the report.
        graph: Box<PairGraph>,
    },
    /// Preparing scan `index` of a pair graph failed; `error` is the error as is (its code is
    /// this error's code).
    Scan {
        /// The scan index.
        index: u32,
        /// What failed.
        error: Box<RegError>,
    },
    /// The overlap fraction of scan `moving` against scan `target` failed; `error` is the
    /// error as is (its code is this error's code).
    Pair {
        /// The moving scan index (row of the matrix).
        moving: u32,
        /// The target scan index (column of the matrix).
        target: u32,
        /// What failed.
        error: Box<RegError>,
    },
}

impl RegError {
    /// A stable machine-readable code.
    pub fn code(&self) -> &'static str {
        match self {
            RegError::NoNormals { .. } => "scan.cloud.no_normals",
            RegError::InvalidParameter { .. } => "scan.cloud.invalid_parameter",
            RegError::FrameMismatch(_) => "geo.frame.mismatch",
            RegError::Index(_) => "scan.cloud.index_failed",
            RegError::AllocationFailed { .. } => "scan.cloud.allocation_failed",
            RegError::Inconsistent => "scan.cloud.inconsistent",
            RegError::Disconnected { .. } => "scan.cloud.reg_disconnected",
            RegError::Scan { error, .. } | RegError::Pair { error, .. } => error.code(),
        }
    }

    pub(crate) fn invalid(name: &'static str, reason: String) -> RegError {
        RegError::InvalidParameter { name, reason }
    }
}

impl fmt::Display for RegError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RegError::NoNormals { reason } => write!(f, "{} ({})", self.code(), reason.code()),
            RegError::InvalidParameter { name, reason } => write!(f, "{} ({name}: {reason})", self.code()),
            RegError::FrameMismatch(e) => write!(f, "{}: {e}", self.code()),
            RegError::Index(e) => write!(f, "{}: {} ({e})", self.code(), e.code()),
            RegError::AllocationFailed { bytes } => write!(f, "{} ({bytes} bytes)", self.code()),
            RegError::Inconsistent => f.write_str(self.code()),
            RegError::Disconnected { components, .. } => write!(f, "{} (components {components:?})", self.code()),
            RegError::Scan { index, error } => write!(f, "scan {index}: {error}"),
            RegError::Pair { moving, target, error } => write!(f, "scan {moving} against scan {target}: {error}"),
        }
    }
}

impl std::error::Error for RegError {}

impl From<PointIndexError> for RegError {
    fn from(e: PointIndexError) -> RegError {
        RegError::Index(e)
    }
}
