//! `verify.package@1` command definition (R1, D0; `TRUST.VERIFY.CLI`).
//!
//! Shares [`crate::runner::verify_package`] with the standalone `fid-verify` CLI so
//! `verify.package@1` and `fid-verify --json` produce byte-identical `VerificationReport` JSON.

use crate::report::VerificationReport;
use crate::runner::{self, VerifyError, VerifyOptions};
use fid_pkg::reader::PackageOpener;
use fid_registry::{fid_command, fid_commands, CommandDef, FidSchema};

/// Command ID.
pub const COMMAND_ID: &str = "verify.package@1";
/// CLI group (`fid verify ...`).
pub const CLI_GROUP: &str = "verify";
/// CLI subcommand (`fid verify package`).
pub const CLI_SUBCOMMAND: &str = "package";
/// MCP tool name.
pub const MCP_TOOL: &str = "verify_package";

/// Parameters of `verify.package@1`.
#[derive(FidSchema, Debug, Clone, PartialEq, Eq)]
pub struct VerifyPackageParams {
    #[fid(
        description = "Handle of the Result Package to verify (a file path on the CLI, a host-issued handle on MCP).",
        min_length = 1,
        max_length = 4096
    )]
    pub package: String,
    #[fid(description = "Optional handle of the trust-root file (trusted keys).", min_length = 1, max_length = 4096)]
    pub trust_root: Option<String>,
    #[fid(description = "Optional handle of the revocation file.", min_length = 1, max_length = 4096)]
    pub revoked: Option<String>,
    #[fid(
        description = "Optional handle of a keyring of known but untrusted keys.",
        min_length = 1,
        max_length = 4096
    )]
    pub keys: Option<String>,
    #[fid(
        description = "Optional reference time supplied by the caller (RFC 3339 UTC; never reads a clock).",
        min_length = 20,
        max_length = 30
    )]
    pub as_of: Option<String>,
    #[fid(description = "Whether to run kernel replay verification for points-tier packages.", default = false)]
    pub replay: bool,
    #[fid(description = "Optional handle of a REP-1 equivalence table file.", min_length = 1, max_length = 4096)]
    pub rep1: Option<String>,
    #[fid(description = "Optional directory of cached WASM kernel artifacts.", min_length = 1, max_length = 4096)]
    pub kernel_cache: Option<String>,
    #[fid(description = "Stop after the first failing section and mark later sections skipped.", default = false)]
    pub fail_fast: bool,
    #[fid(
        description = "Optional replay memory limit (bytes or suffix, e.g. 64MiB).",
        min_length = 1,
        max_length = 32
    )]
    pub memory_limit: Option<String>,
    #[fid(description = "Optional replay time budget (e.g. 10s, 500ms, 0s).", min_length = 1, max_length = 32)]
    pub time_budget: Option<String>,
}

impl From<&VerifyPackageParams> for VerifyOptions {
    fn from(p: &VerifyPackageParams) -> Self {
        VerifyOptions {
            package: p.package.clone(),
            trust_root: p.trust_root.clone(),
            revoked: p.revoked.clone(),
            keys: p.keys.clone(),
            as_of: p.as_of.clone(),
            replay: p.replay,
            rep1: p.rep1.clone(),
            kernel_cache: p.kernel_cache.clone(),
            fail_fast: p.fail_fast,
            memory_limit: p.memory_limit.clone(),
            time_budget: p.time_budget.clone(),
        }
    }
}

/// Runs `verify.package@1` through the shared [`runner::verify_package`] entry point.
#[fid_command(
    id = "verify.package",
    major = 1,
    title = "Verify result package",
    description = "Verifies a Result Package's container, manifest digests, DSSE signatures, journal hash chain, checkpoints, ResultRecord linkage, and optional kernel replay, returning a composed VerificationReport.",
    side_effect = R1,
    mcp_projection = Exposed,
    determinism = D0,
    idempotency = Natural,
    fs_effect = ReadRoot,
    undo = NotUndoable,
    preconditions = [],
    effects = [],
    algorithms = [],
    exposure = [Cli, Mcp],
    lifecycle(state = "active", since = "2026-11-24"),
    web = Unavailable,
    web_note = "Packages and trust files are opened from the local file system, which the web build cannot reach.",
)]
pub fn verify_package(ctx: &dyn PackageOpener, params: VerifyPackageParams) -> Result<VerificationReport, VerifyError> {
    let opts = VerifyOptions::from(&params);
    runner::verify_package(ctx, &opts)
}

fid_commands![verify_package];

/// Registry command definitions exported by `fid-verify`.
pub fn command_defs() -> Vec<CommandDef> {
    declarations().into_iter().map(|d| d.def).collect()
}
