//! Form tolerance checks (spec subsystem `GDT.FORM`): the command layer over the pure
//! evaluation in `fid_gps::form`.
//!
//! - [`flat`] (`GDT.FORM.FLAT`): `check.eval_flatness@1` (`fid check eval-flatness` /
//!   `check_eval_flatness`).

pub mod cyl;
pub mod flat;
pub mod round;
pub mod straightness;
