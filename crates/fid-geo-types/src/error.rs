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
    /// Normal `index` is not finite or its length differs from 1 by more than
    /// [`crate::points::NORMAL_TOLERANCE`] (GEO.TYPES.POINTS; never normalised).
    BadNormal { index: u64 },
    /// A declared organised grid has `rows * cols != count` (GEO.TYPES.POINTS).
    GridShape { rows: u64, cols: u64, count: u64 },
    /// The scan origin of `scanner_id` is not finite, repeats an ID, or is out of
    /// ascending ID order (GEO.TYPES.POINTS).
    ScanOrigin { scanner_id: u32 },
    /// A bounding box has `min > max` (IEEE 754 totalOrder) on `axis` (0 = x, 1 = y, 2 = z).
    InvertedAabb { axis: u8 },
    /// Construction would allocate `needed` working bytes, more than the caller's budget
    /// `limit` (GEO.TYPES.MESH). `needed` is `u64::MAX` if the size overflows.
    AllocationBudget { needed: u64, limit: u64 },
    /// A working allocation of construction failed; `needed` is the total working size
    /// in bytes (GEO.TYPES.MESH). Nothing is created.
    AllocationFailed { needed: u64 },
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
            GeoTypeError::GridShape { .. } => "grid_shape",
            GeoTypeError::ScanOrigin { .. } => "scan_origin",
            GeoTypeError::InvertedAabb { .. } => "inverted_aabb",
            GeoTypeError::AllocationBudget { .. } => "allocation_budget",
            GeoTypeError::AllocationFailed { .. } => "allocation_failed",
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
            GeoTypeError::GridShape { rows, cols, count } => {
                write!(f, "grid {rows} x {cols} does not match {count} points")
            }
            GeoTypeError::ScanOrigin { scanner_id } => {
                write!(f, "scan origin of scanner {scanner_id} is non-finite, repeated or out of order")
            }
            GeoTypeError::InvertedAabb { axis } => write!(f, "bounding box has min > max on axis {axis}"),
            GeoTypeError::AllocationBudget { needed, limit } => {
                write!(f, "mesh construction needs {needed} working bytes, budget is {limit}")
            }
            GeoTypeError::AllocationFailed { needed } => {
                write!(f, "could not allocate {needed} working bytes for mesh construction")
            }
        }
    }
}

impl core::error::Error for GeoTypeError {}
