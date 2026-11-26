//! `fid-agent`: agent experience CI checks and L3 proposal brief command (spec domain `AGENT`).
//! Modules land one spec leaf at a time.
//!
//! - [`catalog`]: the curated workflow catalog per MCP server profile and its CI check
//!   (`AGENT.SKILLS.CATALOG`), run as `cargo run -p fid-agent --bin agent-catalog-check`.
//! - [`style`]: the guide and recipe content standard linter (`AGENT.SKILLS.STYLE`), run as
//!   `cargo run -p fid-agent --bin agent-style-lint`; the standard itself is `agent/STYLE.md`.
//! - [`propose`]: [`ProposalBrief`] schema (`x-untrusted` free-text fields) and
//!   `agent.attach_brief@1` (`AGENT.PROPOSE.BRIEF`).
//! - [`impact`]: [`ProposalImpact`] and `agent.compute_impact@1`, computed from the branch
//!   diff and the stored ResultRecords (`AGENT.PROPOSE.IMPACT`).

pub mod catalog;
pub mod impact;
pub mod propose;
pub mod style;

pub use catalog::{
    catalog_version, check, parse_catalog, skill_dir_name, CatalogIndex, CatalogRow, Code, Finding, IndexRow, Inputs,
    CATALOG_FORMAT, CORE_PROFILE, MIN_APP_WORKFLOWS,
};
pub use impact::{
    class_table, compute_impact, compute_impact_command_def, invoke_compute_impact_cli, invoke_compute_impact_mcp,
    Change, CommandClasses, Evidence, ImpactError, ImpactProject, ImpactRow, Label, MemoryObjects, ObjectRead,
    ObjectSource, Outcome, ProposalImpact, RecordFacts, StoredValue, Uec, CLI_COMPUTE_IMPACT, COMPUTE_IMPACT,
    IMPACT_SCHEMA, MCP_COMPUTE_IMPACT,
};
pub use propose::{
    attach_brief, attach_brief_command_def, brief_schema, command_defs, invoke_attach_brief_cli,
    invoke_attach_brief_mcp, replay_brief_entries, AttachBriefResult, BranchHeads, BriefEntry, BriefError,
    BriefJournal, Confidence, FieldError, ProposalBrief, RootHandle, SkillRef, ATTACH_BRIEF, CLI_ATTACH_BRIEF,
    MAX_ASSUMPTIONS, MAX_ASSUMPTION_CHARS, MAX_BRIEF_BYTES, MAX_BRIEF_DEPTH, MAX_CLAIMED_CHARACTERISTICS,
    MAX_CLIENT_MODEL_CHARS, MAX_CONFIDENCE_REASONS, MAX_CONFIDENCE_REASON_CHARS, MAX_GOAL_CHARS, MAX_RATIONALE_STEPS,
    MAX_RATIONALE_STEP_CHARS, MAX_SKILLS_USED, MAX_SKILL_REF_CHARS, MCP_ATTACH_BRIEF, MIN_CLIENT_MODEL_CHARS,
    MIN_CONFIDENCE_REASONS, MIN_GOAL_CHARS, MIN_RATIONALE_STEPS, MIN_RATIONALE_STEP_CHARS,
};
pub use style::{lint, LintFinding, LintReport, Rule, StyleInputs};
