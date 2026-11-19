//! GD&T characteristics and the DMT workflow of Metrology Maven (spec domain `GDT`). This crate
//! grows one leaf at a time; today it holds:
//!
//! - [`dmt_flow::roster`] (`GDT.RES.ROWFLOW.ROSTER`): the `dmt-reviewers/v1` roster file,
//!   its loader, its digest and the reviewer role lookup;
//! - [`chars::dprof`] (`GDT.CHAR.DPROF`): defaults profiles as immutable, content-addressed,
//!   versioned objects, with the publish and re-point commands, their journal events and
//!   exact replay.
//!
//! Nothing here panics on any input: every outcome is a returned `Result`.

pub mod chars;
pub mod dmt_flow;
