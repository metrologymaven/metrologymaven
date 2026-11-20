// expect: expected `Declared`
// REQ-CORE.REG.DEF.MACRO-004: the attribute names a result type other than the handler's.
use fid_registry::{fid_command, FidSchema};

#[derive(FidSchema)]
pub struct Params {}

#[derive(FidSchema)]
pub struct Declared {
    pub count: u32,
}

#[derive(FidSchema)]
pub struct Returned {
    pub name: String,
}

#[fid_command(
    id = "demo.count",
    major = 1,
    title = "Count",
    description = "Counts.",
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
    params = Params,
    result = Declared,
)]
pub fn count(_ctx: (), _p: Params) -> Result<Returned, String> {
    Ok(Returned { name: String::new() })
}

fn main() {
    let _ = __fid_command_count();
}
