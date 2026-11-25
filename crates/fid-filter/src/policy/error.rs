//! The typed policy error (REQ-KRN.OUT.POL-001, -002, -003, -004, -005).

use core::fmt;

use fid_num::NumError;

/// Why a policy, a mask or a reduction was refused. Every variant has a stable
/// [`code`](PolicyError::code).
#[derive(Debug, Clone, PartialEq)]
pub enum PolicyError {
    /// A parameter is missing, non-finite, out of range or (when decoding) malformed.
    /// `field` names the parameter (`encoding` for the byte form as a whole) and `reason` is
    /// one of the reasons listed in the [module documentation](super).
    InvalidParameter { field: &'static str, reason: &'static str },
    /// Applying the mask would leave `remaining` points; the target primitive needs
    /// `required` (REQ-KRN.OUT.POL-003).
    TooFewRemaining { remaining: usize, required: usize },
    /// The mask excludes `fraction` of the input, more than `limit`, the policy's
    /// `max_excluded_fraction` (REQ-KRN.OUT.POL-004).
    ExcessiveExclusion { fraction: f64, limit: f64 },
    /// A mask index is not below the input point count (REQ-KRN.OUT.POL-005).
    IndexOutOfRange { index: u32 },
    /// A mask names `index` more than once; `index` is the smallest repeated one. A method
    /// that names a point twice has miscounted, so the mask is refused rather than merged.
    DuplicateIndex { index: u32 },
    /// Nominal distances were given to a method that does not use them (`required: false`),
    /// or not given to a gate method that needs them (`required: true`).
    NominalDistances { required: bool },
    /// An input that must have one value per point does not: `what` is `residuals`,
    /// `nominal_distances` or `mask`.
    LengthMismatch { what: &'static str, expected: usize, got: usize },
    /// A residual or nominal distance is NaN or infinite (KNP-1: NaN is an error).
    Num(NumError),
    /// A buffer could not be reserved.
    Alloc { op: &'static str },
}

impl PolicyError {
    /// Stable code.
    pub const fn code(&self) -> &'static str {
        match self {
            PolicyError::InvalidParameter { .. } => "invalid_parameter",
            PolicyError::TooFewRemaining { .. } => "too_few_remaining",
            PolicyError::ExcessiveExclusion { .. } => "excessive_exclusion",
            PolicyError::IndexOutOfRange { .. } => "index_out_of_range",
            PolicyError::DuplicateIndex { .. } => "duplicate_index",
            PolicyError::NominalDistances { .. } => "nominal_distances",
            PolicyError::LengthMismatch { .. } => "length_mismatch",
            PolicyError::Num(_) => "num",
            PolicyError::Alloc { .. } => "alloc_failed",
        }
    }
}

impl fmt::Display for PolicyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PolicyError::InvalidParameter { field, reason } => write!(f, "invalid_parameter: {field}: {reason}"),
            PolicyError::TooFewRemaining { remaining, required } => {
                write!(f, "too_few_remaining: {remaining} points would remain, the primitive needs {required}")
            }
            PolicyError::ExcessiveExclusion { fraction, limit } => {
                write!(f, "excessive_exclusion: {fraction} of the input excluded, limit {limit}")
            }
            PolicyError::IndexOutOfRange { index } => write!(f, "index_out_of_range: {index}"),
            PolicyError::DuplicateIndex { index } => write!(f, "duplicate_index: {index}"),
            PolicyError::NominalDistances { required: true } => {
                write!(f, "nominal_distances: the method needs them and none were given")
            }
            PolicyError::NominalDistances { required: false } => {
                write!(f, "nominal_distances: given to a method that does not use them")
            }
            PolicyError::LengthMismatch { what, expected, got } => {
                write!(f, "length_mismatch: {what} has {got} values, expected {expected}")
            }
            PolicyError::Num(e) => write!(f, "num: {e}"),
            PolicyError::Alloc { op } => write!(f, "alloc_failed: {op}"),
        }
    }
}

impl core::error::Error for PolicyError {}

impl From<NumError> for PolicyError {
    fn from(e: NumError) -> Self {
        PolicyError::Num(e)
    }
}

pub(crate) const fn invalid(field: &'static str, reason: &'static str) -> PolicyError {
    PolicyError::InvalidParameter { field, reason }
}

pub(crate) const fn alloc(op: &'static str) -> PolicyError {
    PolicyError::Alloc { op }
}
