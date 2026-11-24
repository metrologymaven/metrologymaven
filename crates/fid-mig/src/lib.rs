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
//!
//! [`cover::corpus`]: migration corpus manifests (`mig-corpus@1`), their dataset-catalogue,
//! visibility and source-register admission checks and the corpus digest, run by
//! `cargo xtask mig corpus-check` (spec node `MIG.COVER.CORPUS`).
//!
//! [`cover::gen`]: per-format coverage tables (`mig-coverage@1`) generated from the corpus,
//! the taxonomy and producer reports, with the drift check, as `migrate.generate_coverage@1`
//! (spec node `MIG.COVER.GEN`).
//!
//! [`explain::cat`]: closed factor catalogue (`mig-factors@1`) and typed probe candidates
//! (spec node `MIG.EXPLAIN.CAT`).

pub mod cover;
pub mod explain;
pub mod ladder;
pub mod src;

/// Every command this crate declares (for `fid-registry-build`): `migrate.list_sources@1`
/// (MIG.SRC.REG) and `migrate.generate_coverage@1` (MIG.COVER.GEN).
pub fn command_defs() -> Result<Vec<fid_registry::CommandDef>, fid_registry::DefError> {
    let mut defs = src::reg::command_defs()?;
    defs.push(cover::gen::command_def()?);
    Ok(defs)
}
