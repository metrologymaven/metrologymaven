//! The tolerance-zone engine (spec node `GDT.ZONE`). It grows one leaf at a time; today it
//! holds:
//!
//! - [`shape`] (`GDT.ZONE.ZTYPE`): the zone shapes and their fixed-pose usage, the closed-form
//!   vocabulary the pose optimizer (`GDT.ZONE.POSE`) and the tolerance families build on.

pub mod shape;
