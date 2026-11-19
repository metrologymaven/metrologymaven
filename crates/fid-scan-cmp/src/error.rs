//! The typed error of the deviation field (spec node `SCAN.CMP.DEV.FIELD`). Errors are
//! values; nothing in this crate panics (CONSTITUTION §8).

use core::fmt;

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
