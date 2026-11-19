//! GD&T characteristics and the DMT workflow of Metrology Maven (spec domain `GDT`). This crate
//! grows one leaf at a time; today it holds:
//!
//! - [`dmt_flow::roster`] (`GDT.RES.ROWFLOW.ROSTER`): the `dmt-reviewers/v1` roster file,
//!   its loader, its digest and the reviewer role lookup;
//! - [`chars::dprof`] (`GDT.CHAR.DPROF`): defaults profiles as immutable, content-addressed,
//!   versioned objects, with the publish and re-point commands, their journal events and
//!   exact replay;
//! - [`chars::spec`] (`GDT.CHAR.SPEC`): the governing spec of a characteristic (dialect,
//!   standard designators with editions, defaults profile), `gdt.set_governing_spec@1`,
//!   creation-time inheritance from plan and tenant defaults, and the evaluation gate;
//! - [`resolve`] (`GDT.RES.RESOLVE`): the commands `gdt.resolve_chain@1` and
//!   `gdt.get_dmt_row@1` over the pure resolver of `fid-gps`, behind the governing-spec gate.
//!
//! Nothing here panics on any input: every outcome is a returned `Result`.

pub mod chars;
pub mod dmt_flow;
pub mod resolve;
