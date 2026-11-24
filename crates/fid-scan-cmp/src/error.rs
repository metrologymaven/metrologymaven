//! Typed errors of the deviation field (`SCAN.CMP.DEV.FIELD`) and deviation primitive
//! (`SCAN.CMP.DEV.PRIM`). Errors are values; nothing in this crate panics (CONSTITUTION §8).

use core::fmt;
use fid_geo_dist::{DistError, SignError};
use fid_geo_frame::GeoError;

/// What is wrong with the index named by [`FieldError::Incomplete`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IncompleteKind {
    /// The index was never set.
    Unset,
    /// The index was set more than once.
    SetTwice,
}

/// Every error the field builder, the provenance check or the statistics can return.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FieldError {
    /// `finish()` found an index that is unset or was set more than once; `index` is the
    /// lowest such index (REQ-SCAN.CMP.DEV.FIELD-003). Code `scan.cmp.field_incomplete`.
    Incomplete { index: u64, kind: IncompleteKind },
    /// A `VALID` value is NaN or infinite (`index` is the point), or a statistic overflowed
    /// to infinity (`index` is `None`). Code `scan.cmp.non_finite`.
    NonFinite { index: Option<u64> },
    /// `set` was called with an index not below the point count. Code
    /// `scan.cmp.field_index_out_of_range`.
    IndexOutOfRange { index: u64, len: u64 },
    /// A provenance parameter is out of its domain: `max_distance_m` not finite or not
    /// positive, `max_angle_rad` not in `[0, PI]`, or an empty `alignment_id` or
    /// `kernel_version`. Code `scan.cmp.invalid_parameter` (shared with SCAN.CMP.DEV.PRIM).
    InvalidParameter { name: &'static str },
    /// The builder's columns could not be allocated; `needed` is their total size in
    /// bytes (`u64::MAX` if the size overflows). Code `scan.cmp.allocation_failed`.
    AllocationFailed { needed: u64 },
}

impl FieldError {
    /// A stable machine-readable code.
    pub fn code(&self) -> &'static str {
        match self {
            FieldError::Incomplete { .. } => "scan.cmp.field_incomplete",
            FieldError::NonFinite { .. } => "scan.cmp.non_finite",
            FieldError::IndexOutOfRange { .. } => "scan.cmp.field_index_out_of_range",
            FieldError::InvalidParameter { .. } => "scan.cmp.invalid_parameter",
            FieldError::AllocationFailed { .. } => "scan.cmp.allocation_failed",
        }
    }
}

impl fmt::Display for FieldError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let code = self.code();
        match self {
            FieldError::Incomplete { index, kind: IncompleteKind::Unset } => write!(f, "{code}: index {index} unset"),
            FieldError::Incomplete { index, kind: IncompleteKind::SetTwice } => {
                write!(f, "{code}: index {index} set more than once")
            }
            FieldError::NonFinite { index: Some(i) } => write!(f, "{code}: value at index {i} is NaN or infinite"),
            FieldError::NonFinite { index: None } => write!(f, "{code}: statistic overflowed"),
            FieldError::IndexOutOfRange { index, len } => write!(f, "{code}: index {index} not below {len}"),
            FieldError::InvalidParameter { name } => write!(f, "{code}: {name}"),
            FieldError::AllocationFailed { needed } => write!(f, "{code}: {needed} bytes"),
        }
    }
}

impl std::error::Error for FieldError {}

/// Every error the deviation primitive or batch deviation calculation can return.
#[derive(Debug, Clone, PartialEq)]
pub enum DevError {
    /// A parameter or provenance/normal-slice shape is invalid. Code `scan.cmp.invalid_parameter`.
    InvalidParameter { name: &'static str },
    /// Point set and nominal are in different coordinate frames. Code `geo.frame.mismatch`.
    FrameMismatch(GeoError),
    /// The batch calculation was cancelled via its atomic flag. Code `scan.cmp.cancelled`.
    Cancelled,
    /// A query point or computed deviation is NaN or infinite. Code `scan.cmp.non_finite`.
    NonFiniteQuery { index: Option<u64> },
    /// A query coordinate exceeds `COORD_LIMIT` (`2^250` m). Code `scan.cmp.query_out_of_range`.
    QueryOutOfRange { index: Option<u64> },
    /// An actual normal is zero, non-finite, or exceeds `COORD_LIMIT`. Code `scan.cmp.invalid_normal`.
    InvalidNormal { index: Option<u64> },
    /// The nominal normal at the closest point is undefined or degenerate. Code `scan.cmp.undefined_normal`.
    UndefinedNormal,
    /// Worker thread count is outside `1..=MAX_THREADS`. Code `scan.cmp.invalid_parameter`.
    InvalidThreads { threads: usize },
    /// Batch block size is zero, not a multiple of `BLOCK_QUANTUM`, or above `MAX_BLOCK`. Code `scan.cmp.invalid_parameter`.
    InvalidBlockSize { block: usize },
    /// Fallible allocation failed. Code `scan.cmp.allocation_failed`.
    AllocationFailed { needed: u64 },
    /// Underlying closest-point distance error. Code `scan.cmp.projection_failed`.
    Dist(DistError),
    /// Underlying signed-distance error. Code `scan.cmp.projection_failed`.
    Sign(SignError),
    /// CAD nominal projection failed. Code `scan.cmp.projection_failed`.
    ProjectionFailed { reason: &'static str },
    /// Deviation field construction failed.
    Field(FieldError),
    /// A batch worker or mutex failed unexpectedly. Code `scan.cmp.worker_failed`.
    WorkerFailed,
}

impl DevError {
    /// A stable machine-readable code.
    pub fn code(&self) -> &'static str {
        match self {
            DevError::InvalidParameter { .. } | DevError::InvalidThreads { .. } | DevError::InvalidBlockSize { .. } => {
                "scan.cmp.invalid_parameter"
            }
            DevError::FrameMismatch(_) => "geo.frame.mismatch",
            DevError::Cancelled => "scan.cmp.cancelled",
            DevError::NonFiniteQuery { .. } => "scan.cmp.non_finite",
            DevError::QueryOutOfRange { .. } => "scan.cmp.query_out_of_range",
            DevError::InvalidNormal { .. } => "scan.cmp.invalid_normal",
            DevError::UndefinedNormal => "scan.cmp.undefined_normal",
            DevError::AllocationFailed { .. } => "scan.cmp.allocation_failed",
            DevError::Dist(_) | DevError::Sign(_) | DevError::ProjectionFailed { .. } => "scan.cmp.projection_failed",
            DevError::Field(e) => e.code(),
            DevError::WorkerFailed => "scan.cmp.worker_failed",
        }
    }
}

impl fmt::Display for DevError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let code = self.code();
        match self {
            DevError::InvalidParameter { name } => write!(f, "{code}: {name}"),
            DevError::FrameMismatch(e) => write!(f, "{e}"),
            DevError::Cancelled => write!(f, "{code}: cancelled"),
            DevError::NonFiniteQuery { index: Some(i) } => write!(f, "{code}: query point {i} is not finite"),
            DevError::NonFiniteQuery { index: None } => write!(f, "{code}: query or value is not finite"),
            DevError::QueryOutOfRange { index: Some(i) } => write!(f, "{code}: query point {i} exceeds 2^250 m"),
            DevError::QueryOutOfRange { index: None } => write!(f, "{code}: query coordinate exceeds 2^250 m"),
            DevError::InvalidNormal { index: Some(i) } => write!(f, "{code}: normal at index {i} is invalid"),
            DevError::InvalidNormal { index: None } => write!(f, "{code}: actual normal is invalid"),
            DevError::UndefinedNormal => write!(f, "{code}: nominal normal at closest point is undefined"),
            DevError::InvalidThreads { threads } => write!(f, "{code}: invalid thread count {threads}"),
            DevError::InvalidBlockSize { block } => write!(f, "{code}: invalid block size {block}"),
            DevError::AllocationFailed { needed } => write!(f, "{code}: {needed} bytes"),
            DevError::Dist(e) => write!(f, "{code}: {e}"),
            DevError::Sign(e) => write!(f, "{code}: {e}"),
            DevError::ProjectionFailed { reason } => write!(f, "{code}: {reason}"),
            DevError::Field(e) => write!(f, "{e}"),
            DevError::WorkerFailed => write!(f, "{code}: worker failed"),
        }
    }
}

impl std::error::Error for DevError {}

impl From<FieldError> for DevError {
    fn from(e: FieldError) -> Self {
        DevError::Field(e)
    }
}

impl From<GeoError> for DevError {
    fn from(e: GeoError) -> Self {
        DevError::FrameMismatch(e)
    }
}

impl From<DistError> for DevError {
    fn from(e: DistError) -> Self {
        match e {
            DistError::NonFiniteQuery => DevError::NonFiniteQuery { index: None },
            DistError::QueryOutOfRange => DevError::QueryOutOfRange { index: None },
            other => DevError::Dist(other),
        }
    }
}

impl From<SignError> for DevError {
    fn from(e: SignError) -> Self {
        match e {
            SignError::Dist(d) => d.into(),
            SignError::AllocationFailed { bytes } => DevError::AllocationFailed { needed: bytes },
            other => DevError::Sign(other),
        }
    }
}
