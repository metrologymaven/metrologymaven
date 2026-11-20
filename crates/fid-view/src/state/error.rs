//! Typed errors of the ViewState model.

use std::fmt;

/// Why a ViewState is refused. Every variant names the field it concerns, as a path such as
/// `camera.eye_m[1]`, `clips[3].normal` or `visibility[12].entity` (indices are positions in the
/// input array or `Vec`, before canonical sorting). `$` is the whole document, for faults found
/// before any field is known (size, nesting, JSON syntax).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ViewStateError {
    /// Not a ViewState: bad JSON, a missing or unknown field, or a value of the wrong type.
    Malformed { field: String, reason: String },
    /// NaN or an infinity (in JSON: `NaN`, `Infinity` or a literal that overflows to infinity).
    NonFiniteNumber { field: String },
    /// Over a size limit: input bytes, nesting depth, clip planes or collection entries.
    LimitExceeded { field: String, limit: usize },
    /// A schema version this build does not read or write.
    UnsupportedSchema { found: u64 },
    /// Zero up vector, eye equal to target, up parallel to the view direction, near not below
    /// far, or a field of view or orthographic height out of range.
    DegenerateCamera { field: String, reason: &'static str },
    /// A clip-plane normal that is the zero vector.
    DegenerateNormal { field: String },
    /// A clip-plane normal whose squared length is not within the unit tolerance of 1.
    NonUnitNormal { field: String },
    /// A value outside its range (alpha, viewport size).
    OutOfRange { field: String, reason: &'static str },
    /// Two clip planes with the same ID, or two entries for the same entity in one collection.
    DuplicateKey { field: String },
    /// A reference (entity, ColorMapSpec, legend layout, LabelSet) the document does not hold.
    UnknownReference { field: String },
    /// Valid, but not in canonical form; `at` is the first byte that differs.
    NotCanonical { at: usize },
}

impl ViewStateError {
    /// Stable machine-readable code.
    pub fn code(&self) -> &'static str {
        match self {
            ViewStateError::Malformed { .. } => "malformed",
            ViewStateError::NonFiniteNumber { .. } => "non_finite_number",
            ViewStateError::LimitExceeded { .. } => "limit_exceeded",
            ViewStateError::UnsupportedSchema { .. } => "unsupported_schema",
            ViewStateError::DegenerateCamera { .. } => "degenerate_camera",
            ViewStateError::DegenerateNormal { .. } => "degenerate_normal",
            ViewStateError::NonUnitNormal { .. } => "non_unit_normal",
            ViewStateError::OutOfRange { .. } => "out_of_range",
            ViewStateError::DuplicateKey { .. } => "duplicate_key",
            ViewStateError::UnknownReference { .. } => "unknown_reference",
            ViewStateError::NotCanonical { .. } => "not_canonical",
        }
    }

    /// The field the error concerns (`$` for the whole document).
    pub fn field(&self) -> &str {
        match self {
            ViewStateError::Malformed { field, .. }
            | ViewStateError::NonFiniteNumber { field }
            | ViewStateError::LimitExceeded { field, .. }
            | ViewStateError::DegenerateCamera { field, .. }
            | ViewStateError::DegenerateNormal { field }
            | ViewStateError::NonUnitNormal { field }
            | ViewStateError::OutOfRange { field, .. }
            | ViewStateError::DuplicateKey { field }
            | ViewStateError::UnknownReference { field } => field,
            ViewStateError::UnsupportedSchema { .. } => "schema",
            ViewStateError::NotCanonical { .. } => "$",
        }
    }
}

impl fmt::Display for ViewStateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} at {}", self.code(), self.field())?;
        match self {
            ViewStateError::Malformed { reason, .. } => write!(f, ": {reason}"),
            ViewStateError::LimitExceeded { limit, .. } => write!(f, ": limit {limit}"),
            ViewStateError::UnsupportedSchema { found } => write!(f, ": version {found}"),
            ViewStateError::DegenerateCamera { reason, .. } | ViewStateError::OutOfRange { reason, .. } => {
                write!(f, ": {reason}")
            }
            ViewStateError::NotCanonical { at } => write!(f, ": first difference at byte {at}"),
            _ => Ok(()),
        }
    }
}

impl std::error::Error for ViewStateError {}
