//! Characteristic model, dialects and result records (spec node `GDT.CHAR`).
//!
//! Built so far: [`dprof`] (`GDT.CHAR.DPROF`), defaults profiles as immutable versioned
//! objects, and [`spec`] (`GDT.CHAR.SPEC`), the governing spec of each characteristic, its
//! inheritance from plan and tenant defaults, and the gate every evaluation calls first, and
//! [`record`] (`GDT.CHAR.RECORD`), the ResultRecord builder, its decision binding and
//! `gdt.get_result@1`, and [`qpid`] (`GDT.CHAR.IDENT`), the stable identity of each
//! characteristic (imported QPId or assigned UUIDv7, edit check, `fx:qpid` mapping), and
//! [`interp`] (`GDT.CHAR.INTERP`), the interpretation block and provisional label of every
//! record (D-045).

pub mod dprof;
pub mod interp;
pub mod qpid;
pub mod record;
pub mod spec;
