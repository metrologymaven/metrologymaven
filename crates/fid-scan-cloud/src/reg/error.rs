//! The errors of the oriented-cloud target, the overlap fraction and stride sampling.

use core::fmt;

use fid_geo_frame::GeoError;
use fid_geo_index::kdtree::PointIndexError;

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
        }
    }
}

impl std::error::Error for RegError {}

impl From<PointIndexError> for RegError {
    fn from(e: PointIndexError) -> RegError {
        RegError::Index(e)
    }
}
