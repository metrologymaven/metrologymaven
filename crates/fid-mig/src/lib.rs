//! Migration from incumbent software (spec domain `MIG`).
//!
//! [`ladder::def`]: the migration ladder as data (`mig-ladder@1`), its loader
//! [`ladder::def::MigrationLadder`] and the build-time stage-independence check run by
//! `cargo xtask mig ladder-check` (spec node `MIG.LADDER.DEF`). Deterministic (D0): BTreeMap
//! and BTreeSet only, no clock, no environment.
//!
//! [`src::reg`]: the incumbent source register (`mig-sources@1`) with its review records
//! (`mig-review@1`), the sources check run by `cargo xtask mig sources-check`, and the
//! listing `migrate.list_sources@1` with the generated sources page (spec node `MIG.SRC.REG`).

//! [`cover::taxo`]: per-format construct taxonomies (`mig-taxonomy@1`) and the completeness,
//! closure, attestation and breaking-change checks run by `cargo xtask mig taxonomy-check`
//! (spec node `MIG.COVER.TAXO`).

pub mod cover;
pub mod ladder;
pub mod src;
