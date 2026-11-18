//! Metrology Maven outreach tooling (domain OUTR).
//!
//! - [`asset_check`]: outreach attribution and font-origin check (spec node
//!   `OUTR.BRAND.ASSET.ATTR`).
//! - [`metrics::def`]: adoption metric definitions, source list and no-telemetry guard (spec node
//!   `OUTR.COMM.METRIC.DEF`).
//! - [`copy_guard`]: copied-text policy guard, D-013 option C (spec node `OUTR.BRAND.COPY`).
//! - [`review_gate`]: human trade-dress review gate; its checklist and record parsers are shared
//!   with [`copy_guard`] (spec node `OUTR.BRAND.REVIEW`).
//! - [`launch_plan`]: the milestone criteria file (`launch/criteria.toml`), its parser and the
//!   `plan-check` rules (spec node `OUTR.LAUNCH.PLAN.FMT`). The gate and review checks reuse the
//!   parsed types.
//! - [`qa::a11y`]: static accessibility gate for built pages (spec node `OUTR.SITE.QA.A11Y`).
//! - [`doc_run`]: runs the `sh run` blocks of the contributor guide against a temporary copy of
//!   the revision it documents (spec node `OUTR.DOCS.CONTRIB`).
//! - [`guide_check`]: brand-guideline family-name token check and publish guard (spec node
//!   `OUTR.BRAND.GUIDE.NAME`).
//! - [`moderation`]: private moderation log check and quarterly counts (spec node
//!   `OUTR.COMM.MOD`).
//! - [`site::import`]: generated-input lock file, fetch, digest check and build manifest of the
//!   website (spec node `OUTR.SITE.GEN.IMPORT`).

pub mod asset_check;
pub mod copy_guard;
pub mod doc_run;
pub mod guide_check;
pub mod launch_plan;
pub mod metrics;
pub mod moderation;
pub mod qa;
pub mod review_gate;
pub mod site;
