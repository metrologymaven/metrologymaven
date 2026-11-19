// expect: #[fid_command] is missing required field `web`
// REQ-CORE.REG.DEF.MACRO-005: web is required by CORE.REG.DEF.FIELD.
use fid_registry::{fid_command, FidSchema};

#[derive(FidSchema)]
pub struct Empty {}

#[fid_command(
    id = "demo.nothing",
    major = 1,
    title = "Nothing",
    description = "Does nothing.",
    side_effect = R0,
    mcp_projection = Exposed,
    determinism = D0,
    idempotency = Natural,
    fs_effect = None,
    undo = NotUndoable,
    preconditions = [],
    effects = [],
    algorithms = [],
    exposure = [Cli],
    lifecycle(state = "active", since = "2026-11-19"),
)]
pub fn nothing(_ctx: (), _p: Empty) -> Result<Empty, String> {
    Ok(Empty {})
}

fn main() {}
