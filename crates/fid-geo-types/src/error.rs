//! The typed error of every constructor and guard in GEO.TYPES. Spec node `GEO.TYPES.ENV`.

use crate::envelope::ElementKind;
use core::fmt;

/// Every error an element constructor, kind guard or bounding-box query can return.
/// Errors are values; nothing in this crate panics (CONSTITUTION §8).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GeoTypeError {
    /// An operation needed one element kind and was given another (REQ-GEO.TYPES.ENV-003).
    WrongKind { expected: ElementKind, found: ElementKind },
    /// The element has no points, so it has no bounding box (REQ-GEO.TYPES.ENV-005).
    Empty,
    /// A coordinate is NaN or infinite. `index` is the point (or vertex) index; for
    /// [`crate::Aabb::new`] it counts the six corner values min x, y, z, max x, y, z.
    NonFinite { index: u64 },
    /// Triangle `triangle` refers to vertex `index`, which does not exist.
    IndexOutOfRange { triangle: u64, index: u64 },
    /// A per-point array has a length different from the point count.
    AttributeLength { name: String, expected: u64, found: u64 },
    /// A normal is zero or not finite (used by GEO.TYPES.POINTS).
    BadNormal { index: u64 },
    /// A bounding box has `min > max` (IEEE 754 totalOrder) on `axis` (0 = x, 1 = y, 2 = z).
    InvertedAabb { axis: u8 },
}

impl GeoTypeError {
    /// A stable machine-readable code.
    pub fn code(&self) -> &'static str {
        match self {
            GeoTypeError::WrongKind { .. } => "wrong_kind",
            GeoTypeError::Empty => "empty",
            GeoTypeError::NonFinite { .. } => "non_finite",
            GeoTypeError::IndexOutOfRange { .. } => "index_out_of_range",
            GeoTypeError::AttributeLength { .. } => "attribute_length",
            GeoTypeError::BadNormal { .. } => "bad_normal",
            GeoTypeError::InvertedAabb { .. } => "inverted_aabb",
        }
    }
}

impl fmt::Display for GeoTypeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GeoTypeError::WrongKind { expected, found } => {
                write!(f, "wrong element kind: expected {expected}, found {found}")
            }
            GeoTypeError::Empty => f.write_str("element has no points"),
            GeoTypeError::NonFinite { index } => write!(f, "non-finite coordinate at index {index}"),
            GeoTypeError::IndexOutOfRange { triangle, index } => {
                write!(f, "triangle {triangle} refers to missing vertex {index}")
            }
            GeoTypeError::AttributeLength { name, expected, found } => {
                write!(f, "attribute '{name}' has {found} values, expected {expected}")
            }
            GeoTypeError::BadNormal { index } => write!(f, "bad normal at index {index}"),
            GeoTypeError::InvertedAabb { axis } => write!(f, "bounding box has min > max on axis {axis}"),
        }
    }
}

impl core::error::Error for GeoTypeError {}
