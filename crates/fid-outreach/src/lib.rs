//! Metrology Maven outreach tooling (domain OUTR).
//!
//! - [`bench::recipe`]: benchmark recipe format `bench-recipe@1` and its validator, with the kind
//!   validator hook (spec node `OUTR.BENCH.RUN.RECIPE`).
//! - [`asset_check`]: outreach attribution and font-origin check (spec node
//!   `OUTR.BRAND.ASSET.ATTR`).
//! - [`metrics::def`]: adoption metric definitions, source list and no-telemetry guard (spec node
//!   `OUTR.COMM.METRIC.DEF`).
//! - [`metrics::snap`]: append-only metric snapshot store and quarterly values (spec node
//!   `OUTR.COMM.METRIC.SNAP`).
//! - [`copy_guard`]: copied-text policy guard, D-013 option C (spec node `OUTR.BRAND.COPY`).
//! - [`review_gate`]: human trade-dress review gate; its checklist and record parsers are shared
//!   with [`copy_guard`] (spec node `OUTR.BRAND.REVIEW`).
//! - [`launch_plan`]: the milestone criteria file (`launch/criteria.toml`), its parser and the
//!   `plan-check` rules (spec node `OUTR.LAUNCH.PLAN.FMT`). The gate and review checks reuse the
//!   parsed types.
//! - [`launch_gate`]: the `gate-check` milestone gate report over local spec-tree, parity, CI and
//!   sign-off data (spec node `OUTR.LAUNCH.GATE`).
//! - [`qa::a11y`]: static accessibility gate for built pages (spec node `OUTR.SITE.QA.A11Y`).
//! - [`doc_run`]: runs the `sh run` blocks of the contributor guide against a temporary copy of
//!   the revision it documents (spec node `OUTR.DOCS.CONTRIB`).
//! - [`docs_build`]: builds one static release edition of the docs portal with its shared page
//!   frame, release banner and `nav.toml` navigation tree (spec node `OUTR.DOCS.FRAME.NAV`).
//! - [`dress`]: trade-dress distinctness list check and Markdown renderer (spec node
//!   `OUTR.BRAND.GUIDE.DRESS`).
//! - [`guide_check`]: brand-guideline family-name token check and publish guard (spec node
//!   `OUTR.BRAND.GUIDE.NAME`).
//! - [`moderation`]: private moderation log check and quarterly counts (spec node
//!   `OUTR.COMM.MOD`).
//! - [`ref_pages`]: generated CLI, MCP and Python reference pages and their drift check (spec
//!   node `OUTR.DOCS.REF`).
//! - [`search_index`]: per-edition docs search index with the exact command-name table (spec node
//!   `OUTR.DOCS.SEARCH.INDEX`).
//! - [`site::import`]: generated-input lock file, fetch, digest check and build manifest of the
//!   website (spec node `OUTR.SITE.GEN.IMPORT`).

pub mod asset_check;
pub mod bench;
pub mod copy_guard;
pub mod doc_run;
pub mod docs_build;
pub mod dress;
pub mod guide_check;
pub mod launch_gate;
pub mod launch_plan;
pub mod metrics;
pub mod moderation;
pub mod qa;
pub mod ref_pages;
pub mod review_gate;
pub mod search_index;
pub mod site;
