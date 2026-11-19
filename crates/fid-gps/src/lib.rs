//! The GD&T evaluation library of Metrology Maven (CONSTITUTION §2.1 L0: `fid-gps`). It grows
//! one spec leaf at a time; today it holds:
//!
//! - [`dmt`] (`GDT.RES.SCHEMA`): the Dialect Mapping Table schema (D-009): row keys, rows,
//!   operator chains in the ISO 17450 operator vocabulary, the versioned table container,
//!   its strict bounded loader, the row, chain and table digests, the exported JSON Schema
//!   and the defaults-profile key vocabulary.
//! - [`resolve`] (`GDT.RES.RESOLVE`): the pure interpretation resolver from a characteristic
//!   and its context to a DMT row, operator chain and digests, or `UNRESOLVED` /
//!   `UNSUPPORTED`, and the exact row lookup of `gdt.get_dmt_row@1`.
//! - [`zone`] (`GDT.ZONE.ZTYPE`): the tolerance-zone shapes and their fixed-pose usage and
//!   driving point, in closed form.
//! - [`drf`] (`GDT.DRF.BUILD`): DRF construction from simulated datums and DoF bookkeeping.
//!
//! Pure: no I/O (callers pass bytes), no `HashMap`/`HashSet`, and nothing here panics on any
//! input: every outcome is a returned `Result`. The only floating-point arithmetic is in
//! [`zone`] and [`drf`], written in KNP-1 style (pinned evaluation order, no fused multiply-add, no
//! `f64::min`/`f64::max`, square root through `fid-num`, NaN rejected), so its results are
//! bit-identical on every platform including wasm32.

pub mod dmt;
pub mod drf;
pub mod resolve;
pub mod zone;
