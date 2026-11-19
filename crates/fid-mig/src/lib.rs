//! Migration from incumbent software (spec domain `MIG`).
//!
//! [`ladder::def`]: the migration ladder as data (`mig-ladder@1`), its loader
//! [`ladder::def::MigrationLadder`] and the build-time stage-independence check run by
//! `cargo xtask mig ladder-check` (spec node `MIG.LADDER.DEF`). Deterministic (D0): BTreeMap
//! and BTreeSet only, no clock, no environment.

pub mod ladder;
