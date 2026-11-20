//! Fixture crate `demo_ok` of spec node CORE.REG.DEF.MACRO (AC-1, AC-3, AC-4): three commands
//! declared with the macros, one R0, one W2 with a length parameter in mm, one A0.
//!
//! `crates/fid-registry/tests/decl.rs` compiles this file as a module and compares the
//! generated schemas with `../golden/`; the compile-fail harness compiles it as a crate of its
//! own (`crates/fid-registry/tests/decl_compile_fail.rs`); `fid-registry-macros`' unit tests
//! expand it and search the generated code (AC-4). Licence: as the project (CC-BY-4.0 with the
//! spec).

use fid_registry::{fid_command, fid_commands, FidSchema};

/// Stand-in for the bus's handler context.
pub struct Ctx;

/// The demo handlers' error.
#[derive(Debug, PartialEq, Eq)]
pub struct DemoError;

/// No parameters.
#[derive(FidSchema)]
pub struct NoParams {}

/// What the demo store is doing.
#[derive(FidSchema, Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Idle,
    Measuring,
    #[fid(rename = "off_line")]
    Offline,
}

/// Result of `demo.status@1`.
#[derive(FidSchema)]
#[fid(description = "Status of the demo store.")]
pub struct Status {
    #[fid(description = "Number of features in the store.")]
    pub features: u32,
    pub mode: Mode,
    pub label: Option<String>,
}

/// Parameters of `demo.offset_plane@1`.
#[derive(FidSchema)]
pub struct OffsetParams {
    #[fid(description = "The plane feature to offset.", pattern = "^ftr_[0-9a-f]{8}$")]
    pub feature: String,
    #[fid(quantity = "length", unit = "mm", minimum = -1000, maximum = 1000)]
    pub distance: f64,
    #[fid(max_items = 8, max_length = 32)]
    pub tags: Vec<String>,
    #[fid(default = false)]
    pub dry_run: bool,
}

/// Result of `demo.offset_plane@1`.
#[derive(FidSchema)]
pub struct OffsetResult {
    pub revision: u32,
    #[fid(quantity = "length", unit = "mm")]
    pub applied: f64,
}

/// Telemetry policy values.
#[derive(FidSchema, Debug, Clone, Copy, PartialEq, Eq)]
pub enum Policy {
    Off,
    LocalOnly,
    Upload,
}

/// Parameters of `demo.set_telemetry@1`.
#[derive(FidSchema)]
pub struct TelemetryParams {
    pub policy: Policy,
}

#[fid_command(
    id = "demo.status",
    major = 1,
    title = "Demo status",
    description = "Reports the state of the demo store.",
    side_effect = R0,
    mcp_projection = Exposed,
    determinism = D0,
    idempotency = Natural,
    fs_effect = None,
    undo = NotUndoable,
    preconditions = [],
    effects = [],
    algorithms = [],
    exposure = [Cli, Mcp],
    lifecycle(state = "active", since = "2026-11-19"),
    web = Available,
    cli_bare = true,
)]
pub fn status(_ctx: &Ctx, _params: NoParams) -> Result<Status, DemoError> {
    Ok(Status { features: 3, mode: Mode::Idle, label: None })
}

#[fid_command(
    id = "demo.offset_plane",
    major = 1,
    title = "Offset plane",
    description = "Moves a plane feature along its normal.",
    side_effect = W2,
    mcp_projection = RequestOnly,
    determinism = D1,
    idempotency = Keyed,
    fs_effect = None,
    undo = HeadMove,
    preconditions = [(expr = "size(params.tags) <= 8", bound = 100, message_key = "demo.too_many_tags")],
    effects = ["demo.plane_offset@1"],
    algorithms = ["geo.plane.offset@1.0.0"],
    exposure = [Cli, Mcp, Python, Gui],
    lifecycle(state = "active", since = "2026-11-19"),
    web = Partial,
    web_note = "Preview only in the web build.",
    params = OffsetParams,
    result = OffsetResult,
)]
pub fn offset_plane(_ctx: &Ctx, params: OffsetParams) -> Result<OffsetResult, DemoError> {
    if params.dry_run {
        return Ok(OffsetResult { revision: 0, applied: 0.0 });
    }
    Ok(OffsetResult { revision: 1, applied: params.distance })
}

#[fid_command(
    id = "demo.set_telemetry",
    major = 1,
    title = "Set telemetry policy",
    description = "Sets the site-wide telemetry policy.",
    side_effect = A0,
    mcp_projection = Never,
    determinism = D0,
    idempotency = Natural,
    fs_effect = None,
    undo = NotUndoable,
    preconditions = [],
    effects = ["demo.telemetry_set@1"],
    algorithms = [],
    exposure = [Cli, Gui],
    lifecycle(state = "active", since = "2026-11-19"),
    human_only = true,
    web = Unavailable,
    web_note = "Site policy is set on the desktop app.",
)]
pub fn set_telemetry(_ctx: &Ctx, params: TelemetryParams) -> Result<NoParams, DemoError> {
    match params.policy {
        Policy::Off | Policy::LocalOnly | Policy::Upload => Ok(NoParams {}),
    }
}

fid_commands![status, offset_plane, set_telemetry];
