//! Datum reference frames and datum mobility (spec node `GDT.DRF`).
//!
//! Built so far: [`datum`] (`GDT.DRF.DATUM`), the declaration of datums (label, datum feature,
//! nominal faces, feature type and datum targets) and the command `gdt.define_datum@1`. A
//! DRF can only be built from datums declared here.

pub mod datum;
