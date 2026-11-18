//! `fid-agent`: CI checks for the agent experience (spec domain `AGENT`). Tooling layer;
//! reads files only, registers no command. Modules land one spec leaf at a time.
//!
//! - [`catalog`]: the curated workflow catalog per MCP server profile and its CI check
//!   (`AGENT.SKILLS.CATALOG`), run as `cargo run -p fid-agent --bin agent-catalog-check`.
//! - [`style`]: the guide and recipe content standard linter (`AGENT.SKILLS.STYLE`), run as
//!   `cargo run -p fid-agent --bin agent-style-lint`; the standard itself is `agent/STYLE.md`.

pub mod catalog;
pub mod style;

pub use catalog::{
    catalog_version, check, parse_catalog, skill_dir_name, CatalogIndex, CatalogRow, Code, Finding, IndexRow, Inputs,
    CATALOG_FORMAT, CORE_PROFILE, MIN_APP_WORKFLOWS,
};
pub use style::{lint, LintFinding, LintReport, Rule, StyleInputs};
