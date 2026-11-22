//! Governance-profile gates on promotion (spec node `VIZ.INBOX.GATES`).
//!
//! - [`plan`] (`VIZ.INBOX.GATES.PLAN`): pure gate plan selection, optional `CeremonyProvider`
//!   trait, and all-or-nothing `GateRunner`.

pub mod plan;
