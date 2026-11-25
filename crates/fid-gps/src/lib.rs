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
//! - [`drf`] (`GDT.DRF.BUILD`): DRF construction from simulated datums and DoF bookkeeping,
//!   and the datum target coverage check (`GDT.DRF.COMMON.COVER`).
//! - [`dim`] (`GDT.DIM.RADIAL`): diameter and radius of a feature fitted by `fid-fit`'s
//!   least-squares circle, cylinder or sphere, checked against the chain's association.
//! - [`prof`] (`GDT.PROF.DEV`): per-point signed deviations from the nominal CAD faces
//!   (GEO.DIST.FACE.BODY closest points, GEO.DIST.FACE.INCID signs) with each face's
//!   nominal approximation error, boundary and no-projection flags; and (`GDT.PROF.ZDEF.UNIF`)
//!   the constant-width profile zone definitions as allowed intervals on those deviations;
//!   and (`GDT.PROF.LINE.SECT`) the section bands and line-element deviations of profile of a
//!   line, with exact band membership.
//! - [`loc`] (`GDT.LOC.DERIV.TYPES`): the derived-element type (axis, median line, median
//!   surface, median points) with the producing algorithm ID, `derive()` dispatch on the
//!   chain's reduction operator through a registry keyed by algorithm ID, and the typed
//!   derivation failure reasons, with its exported JSON Schema.
//!
//! Pure: no I/O (callers pass bytes), no `HashMap`/`HashSet`, and nothing here panics on any
//! input: every outcome is a returned `Result`. The only floating-point arithmetic is in
//! [`zone`], [`drf`], [`dim`], [`prof`] and [`loc`] (outside the fits of `fid-fit`), written in KNP-1 style
//! (pinned evaluation order, no fused multiply-add, no `f64::min`/`f64::max`, square root
//! through `fid-num`, NaN rejected), so its results are bit-identical on every platform.
//! CI runs `zone_shape` on wasm32 too; the [`prof`] output pin (GDT.PROF.DEV AC-5) is
//! checked on Linux, Windows and macOS only, not on wasm32.

pub mod dim;
pub mod dmt;
pub mod drf;
pub mod form;
pub mod loc;
pub mod prof;
pub mod resolve;
pub mod zone;
