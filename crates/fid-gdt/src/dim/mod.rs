//! Dimensional checks (spec subsystem `GDT.DIM`): the command layer over the pure evaluation
//! in `fid_gps::dim`.
//!
//! - [`radial`] (`GDT.DIM.RADIAL`): `check.eval_diameter@1` and `check.eval_radius@1`.

pub mod radial;
