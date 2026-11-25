//! The tolerance-zone engine (spec node `GDT.ZONE`). It grows one leaf at a time; today it
//! holds:
//!
//! - [`shape`] (`GDT.ZONE.ZTYPE`): the zone shapes and their fixed-pose usage, the closed-form
//!   vocabulary the pose optimizer (`GDT.ZONE.POSE`) and the tolerance families build on.
//! - [`group`] (`GDT.ZONE.GROUP.IDS`): shared-pose group identity (UUIDv5), canonical pose
//!   digest, `GroupRef` and the all-or-nothing rule for incomplete groups.
//! - [`matl`] (`GDT.ZONE.MATL.BONUS`): bonus tolerance from actual size (MMC/LMC).

pub mod group;
pub mod matl;
pub mod shape;
