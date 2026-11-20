// expect: `Plain` does not implement `FidSchema`
// REQ-CORE.REG.DEF.MACRO-004: the handler's parameter type has no schema.
use fid_registry::{fid_command, FidSchema};

pub struct Plain {
    pub count: u32,
}

#[derive(FidSchema)]
pub struct Out {}

#[fid_command(
    id = "demo.plain",
    major = 1,
    title = "Plain",
    description = "Takes a type without a schema.",
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
    web = Available,
)]
pub fn plain(_ctx: (), _p: Plain) -> Result<Out, String> {
    Ok(Out {})
}

fn main() {
    let _ = __fid_command_plain();
}
