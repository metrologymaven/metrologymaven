//! GD&T characteristics and the DMT workflow of Metrology Maven (spec domain `GDT`). This crate
//! grows one leaf at a time; today it holds [`dmt_flow::roster`] (`GDT.RES.ROWFLOW.ROSTER`):
//! the `dmt-reviewers/v1` roster file, its loader, its digest and the reviewer role lookup.
//!
//! Nothing here panics on any input: every outcome is a returned `Result`.

pub mod dmt_flow;
