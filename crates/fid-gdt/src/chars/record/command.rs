//! The CLI and MCP entry points of `gdt.get_result@1` and its registry definition, in the
//! pattern of the R0 reads of `fid-align` (`align.get_alignment@1`): both surfaces build the
//! same parameters and call [`get_result`], so `fid gdt get-result --run R --char C --json`
//! and the MCP tool `gdt_get_result` return the same bytes, the stored canonical JSON.

use super::store::{get_result, ResultStore, GET_RESULT_COMMAND};
use super::RecordError;
use fid_canon::{CValue, CanonError};
use fid_io_reg::Principal;
use fid_registry::{CommandDef, DefError};
use std::collections::BTreeMap;

/// CLI subcommand of `fid gdt`.
pub const GET_RESULT_CLI: &str = "get-result";
/// MCP tool name.
pub const GET_RESULT_MCP: &str = "gdt_get_result";
/// Largest MCP parameter payload, in bytes.
pub const MAX_PARAMS_BYTES: usize = 4 * 1024;
/// Deepest MCP parameter nesting (the parameter object itself).
const MAX_PARAMS_DEPTH: usize = 1;
/// Most CLI arguments after `fid gdt`.
const MAX_CLI_ARGS: usize = 8;
const LIFECYCLE_SINCE: &str = "2026-11-19";
/// The run and characteristic ID grammar, for the parameter schema.
const ID_PATTERN: &str = "^[A-Za-z0-9][A-Za-z0-9._:-]{0,127}$";

fn bad(reason: &'static str) -> RecordError {
    RecordError::BadParams { reason }
}

/// Reads the two parameters from an object: exactly `run` and `char`, both strings.
fn params(o: &BTreeMap<String, CValue>) -> Result<(&str, &str), RecordError> {
    if o.keys().any(|k| k != "run" && k != "char") {
        return Err(bad("unknown parameter"));
    }
    let field = |k: &str| match o.get(k) {
        Some(CValue::Str(s)) => Ok(s.as_str()),
        Some(_) => Err(bad("parameter is not a string")),
        None => Err(bad("missing parameter")),
    };
    Ok((field("run")?, field("char")?))
}

/// Runs the MCP tool `gdt_get_result` on raw JSON parameters `{"run": .., "char": ..}`
/// (at most [`MAX_PARAMS_BYTES`] bytes, one level, duplicate keys refused) for `principal`
/// and returns the stored canonical bytes.
pub fn invoke_mcp(store: &ResultStore, principal: Principal, tool: &str, raw: &[u8]) -> Result<Vec<u8>, RecordError> {
    if tool != GET_RESULT_MCP {
        return Err(bad("unknown tool"));
    }
    let v = fid_canon::parse_with_limits(raw, MAX_PARAMS_BYTES, MAX_PARAMS_DEPTH).map_err(|e| match e {
        CanonError::InputLimit => bad("params too large or too deep"),
        CanonError::DuplicateKey(_) => bad("duplicate key"),
        _ => bad("params are not valid JSON"),
    })?;
    let CValue::Obj(o) = v else { return Err(bad("params is not an object")) };
    let (run, char) = params(&o)?;
    get_result(store, principal, run, char).map(<[u8]>::to_vec)
}

/// Runs `fid gdt <args>` for `principal`: `get-result --run <R> --char <C> [--json]`. Output
/// is the stored canonical JSON either way. Unknown subcommands, unknown, repeated or
/// value-less options and stray arguments are `bad_params`.
pub fn invoke_cli(store: &ResultStore, principal: Principal, args: &[&str]) -> Result<Vec<u8>, RecordError> {
    if args.len() > MAX_CLI_ARGS {
        return Err(bad("too many arguments"));
    }
    let (sub, rest) = args.split_first().ok_or(bad("missing subcommand"))?;
    if *sub != GET_RESULT_CLI {
        return Err(bad("unknown subcommand"));
    }
    let mut o = BTreeMap::new();
    let mut json = false;
    let mut it = rest.iter();
    while let Some(arg) = it.next() {
        let key = match *arg {
            "--json" if !json => {
                json = true;
                continue;
            }
            "--run" => "run",
            "--char" => "char",
            "--json" => return Err(bad("repeated option")),
            s if s.starts_with('-') => return Err(bad("unknown option")),
            _ => return Err(bad("unexpected argument")),
        };
        let value = it.next().ok_or(bad("option needs a value"))?;
        if o.insert(key.to_owned(), CValue::Str((*value).to_owned())).is_some() {
            return Err(bad("repeated option"));
        }
    }
    let (run, char) = params(&o)?;
    get_result(store, principal, run, char).map(<[u8]>::to_vec)
}

fn text(s: &str) -> CValue {
    CValue::Str(s.to_owned())
}

fn obj<const N: usize>(items: [(&str, CValue); N]) -> CValue {
    CValue::Obj(items.into_iter().map(|(k, v)| (k.to_owned(), v)).collect())
}

fn strs(items: &[&str]) -> CValue {
    CValue::Arr(items.iter().map(|s| text(s)).collect())
}

fn id_schema(description: &str) -> CValue {
    obj([("type", text("string")), ("pattern", text(ID_PATTERN)), ("description", text(description))])
}

/// The registry definition of `gdt.get_result@1` (R0, D0, CLI and MCP), built through
/// [`CommandDef::from_value`] so every field rule of the registry applies.
pub fn command_def() -> Result<CommandDef, DefError> {
    let name = GET_RESULT_COMMAND.split('@').next().unwrap_or(GET_RESULT_COMMAND);
    CommandDef::from_value(obj([
        ("id", text(name)),
        ("major", CValue::Num(1.0)),
        ("title", text("Get result")),
        ("description", text("The stored ResultRecord of one characteristic in one run, as canonical JSON.")),
        (
            "params_schema",
            obj([
                ("type", text("object")),
                ("additionalProperties", CValue::Bool(false)),
                ("required", strs(&["char", "run"])),
                ("properties", obj([("char", id_schema("The characteristic.")), ("run", id_schema("The run."))])),
            ]),
        ),
        ("result_schema", obj([("type", text("object"))])),
        ("side_effect", text("R0")),
        ("mcp_projection", text("exposed")),
        ("determinism", text("D0")),
        ("idempotency", text("natural")),
        ("fs_effect", text("none")),
        ("undo", text("not_undoable")),
        ("preconditions", CValue::Arr(Vec::new())),
        ("effects", CValue::Arr(Vec::new())),
        ("algorithms", CValue::Arr(Vec::new())),
        ("exposure", strs(&["cli", "mcp"])),
        ("lifecycle", obj([("state", text("active")), ("since", text(LIFECYCLE_SINCE))])),
        ("human_only", CValue::Bool(false)),
        ("network", text("none")),
        ("web", text("available")),
        ("cli_bare", CValue::Bool(false)),
    ]))
}
