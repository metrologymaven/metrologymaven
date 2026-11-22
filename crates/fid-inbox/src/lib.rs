//! Proposal Inbox for Metrology Maven (spec node `VIZ.INBOX`).
//!
//! Modules land one spec leaf at a time:
//! - [`gates::plan`] (`VIZ.INBOX.GATES.PLAN`): pure governance-profile gate plan selection,
//!   the optional [`gates::plan::CeremonyProvider`] trait so Lean works with no signing crate
//!   linked, and the all-or-nothing [`gates::plan::GateRunner`].

pub mod gates;
