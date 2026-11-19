//! Datum reference frames (spec node `GDT.DRF`). Today it holds [`build`] (`GDT.DRF.BUILD`):
//! assembling a DRF from one to three simulated datums and its degree-of-freedom bookkeeping.

pub mod build;
