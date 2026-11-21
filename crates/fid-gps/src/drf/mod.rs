//! Datum reference frames (spec node `GDT.DRF`). Today it holds [`build`] (`GDT.DRF.BUILD`):
//! assembling a DRF from one to three simulated datums and its degree-of-freedom bookkeeping,
//! and [`plane_sim`], the measured planar path of the same leaf (certified tangential planes
//! from measured points, in precedence order).

pub mod build;
pub mod plane_sim;
