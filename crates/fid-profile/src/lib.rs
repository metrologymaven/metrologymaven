//! Governance profiles for Metrology Maven (spec node `TRUST.PROFILE`).
//!
//! - [`defs`] (`TRUST.PROFILE.DEFS`): the three built-in profiles as data, the tenant
//!   parameters a site may set on them, and the canonical profile document whose SHA-256
//!   digest is recorded in every Result Package and journal session. Nothing here enforces
//!   a profile.
//!
//! This crate links no CORE crate, so the verifier can read profile documents.

pub mod defs;

pub use defs::{
    instantiate, AgentWrites, CanonicalProfile, MeaningCode, Params, ProfileDoc, ProfileError, ProfileName,
    ProgramReleaseByOther, TenantMeaning, TenantParams,
};
