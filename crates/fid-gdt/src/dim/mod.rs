//! Dimensional checks (spec subsystem `GDT.DIM`): the command layer over the pure evaluation
//! in `fid_gps::dim`.
//!
//! - [`ptdev`] (`GDT.DIM.PTDEV`): `check.eval_deviation@1`.
//! - [`radial`] (`GDT.DIM.RADIAL`): `check.eval_diameter@1` and `check.eval_radius@1`.

pub mod ptdev;
pub mod radial;
