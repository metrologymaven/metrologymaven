//! Coordinate frames, transforms and units for Metrology Maven (spec node `GEO.FRAME`).
//!
//! Pure Rust, `f64`, no I/O, no `unsafe`, kernel graph (KNP-1, CONSTITUTION §4). Modules land
//! one spec leaf at a time:
//!
//! - [`units`]: authored lengths and angles, converted to SI metres or radians with exactly
//!   one rounding and back to display units with exactly one rounding (`GEO.FRAME.UNITS`).
//! - [`xform`] and [`rotation`]: rigid and similarity transforms with validated construction,
//!   composition, inversion and application, and the rotation parameterisations (matrix, unit
//!   quaternion, axis-angle, 12 Euler sequences intrinsic and extrinsic) (`GEO.FRAME.XFORM`).
//! - [`tree`]: the frame tree with one world root, cycle-free edits, chain resolution through
//!   the lowest common ancestor in a pinned order, and the tree digest (`GEO.FRAME.TREE`).

mod bigint;
pub mod rotation;
pub mod tree;
pub mod units;
pub mod xform;

pub use rotation::{AxisAngle, Euler, EulerKind, EulerSeq, Quaternion};
pub use tree::{Frame, FrameError, FrameId, FrameKind, FrameTree};
pub use units::{AngleUnit, AuthoredQuantity, LengthUnit, Unit, UnitsError};
pub use xform::{Matrix3, Rigid, Similarity, Transform, XformError};
