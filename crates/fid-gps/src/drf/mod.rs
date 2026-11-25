//! Datum reference frames (spec node `GDT.DRF`). Today it holds [`build`] (`GDT.DRF.BUILD`):
//! assembling a DRF from one to three simulated datums and its degree-of-freedom bookkeeping,
//! [`plane_sim`], the measured planar path of the same leaf (certified tangential planes from
//! measured points, in precedence order), and [`cyl_sim`], the measured path with planar and
//! external cylindrical datum features (certified minimum circumscribed cylinders), and
//! [`targets`], the datum target coverage check (`GDT.DRF.COMMON.COVER`).

pub mod build;
pub mod cyl_sim;
pub mod plane_sim;
pub mod targets;
