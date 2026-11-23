//! Dimensional checks (spec subsystem `GDT.DIM`): the command layer over the pure evaluation
//! in `fid_gps::dim`.
//!
//! - [`radial`] (`GDT.DIM.RADIAL`): `check.eval_diameter@1` and `check.eval_radius@1`.
//! - [`scalar`] (`GDT.DIM.SCALAR`): `check.eval_scalar@1` and `check.eval_minmax@1`.

pub mod radial;
pub mod scalar;
