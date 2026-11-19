//! Characteristic model, dialects and result records (spec node `GDT.CHAR`).
//!
//! Built so far: [`dprof`] (`GDT.CHAR.DPROF`), defaults profiles as immutable versioned
//! objects, and [`spec`] (`GDT.CHAR.SPEC`), the governing spec of each characteristic, its
//! inheritance from plan and tenant defaults, and the gate every evaluation calls first.

pub mod dprof;
pub mod spec;
