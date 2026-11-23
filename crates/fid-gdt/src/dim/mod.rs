//! Dimensional checks (spec subsystem `GDT.DIM`): the command layer over the pure evaluation
//! in `fid_gps::dim`.
//!
//! - [`angle`] (`GDT.DIM.ANGLE`): `check.eval_angle@1`.
//! - [`linear`] (`GDT.DIM.LINEAR`): `check.eval_distance@1` and `check.eval_caliper@1`.
//! - [`ptdev`] (`GDT.DIM.PTDEV`): `check.eval_deviation@1`.
//! - [`radial`] (`GDT.DIM.RADIAL`): `check.eval_diameter@1` and `check.eval_radius@1`.
//! - [`scalar`] (`GDT.DIM.SCALAR`): `check.eval_scalar@1` and `check.eval_minmax@1`.

pub mod angle;
pub mod linear;
pub mod ptdev;
pub mod radial;
pub mod scalar;
