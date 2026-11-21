//! Open Reference Suites: the case format, the traceability checker and suite versioning.
//! Spec node `UNC.ORS.TRACE` (`spec/unc/ors/trace/NODE.md`); every choice is pinned in its
//! "Pinned at build" section.
//!
//! - [`suite`]: the manifest `ors-suite/v1`, the case document `ors-case/v1` (an
//!   `rdl-dataset/v1` record of tier O plus the ORS fields) and bounded suite loading.
//! - [`check`]: [`check_suite`], findings in a fixed order (REQ-001 to REQ-007) and the
//!   coverage count.
//! - [`command`]: `rdl.check_suite@1` (R1) and `rdl.list_suite@1` (R0), one handler behind
//!   the CLI and MCP forms, an Activity Log record per call and exact replay.
//!
//! Nothing here panics. No error or finding holds text taken from the input. The checker
//! never computes, alters or claims any uncertainty value, and the review check reads the
//! declared review fields only: it never reports anything as signed off (D-009, D-045).

pub mod check;
pub mod command;
pub mod suite;

pub use check::{check_suite, CheckReport, Coverage, Finding, CODES};
pub use command::{
    command_defs, execute, failed_envelope, invoke_cli, invoke_mcp, replay, run, ActivityEntry, Call, CheckParams,
    CliOutput, SuiteStore, Surface, CHECK_SUITE, CLI_CHECK, CLI_LIST, LIST_SUITE, MCP_CHECK, MCP_LIST,
};
pub use suite::{
    case_id_ok, parse_handle, suite_name_ok, ChangelogEntry, OrsError, Suite, SuiteManifest, CASE_SCHEMA,
    DEFAULT_LICENCE, MAX_TEXT_CHARS, SUITE_SCHEMA,
};
