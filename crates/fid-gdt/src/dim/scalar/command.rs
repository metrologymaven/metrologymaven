//! CLI, MCP and registry [`CommandDef`] bindings for `check.eval_scalar@1` and
//! `check.eval_minmax@1` (`GDT.DIM.SCALAR`).

use super::{
    eval_minmax, eval_scalar, obj, EvalError, EvalInputs, Evaluated, MinMaxRequest, ScalarRequest, EVAL_MINMAX_COMMAND,
    EVAL_SCALAR_COMMAND,
};
use crate::chars::record::ResultStore;
use fid_canon::{CValue, CanonError};
use fid_gps::dim::scalar::{MinMaxStat, MINMAX_ALGORITHM_ID, SCALAR_ALGORITHM_ID};
use fid_io_reg::Principal;
use fid_registry::def::{CommandDef, DefError};
use std::collections::BTreeMap;

/// CLI subcommand name for `check.eval_scalar@1` (`fid check eval-scalar`).
pub const EVAL_SCALAR_CLI: &str = "eval-scalar";
/// MCP tool name for `check.eval_scalar@1`.
pub const EVAL_SCALAR_MCP: &str = "check_eval_scalar";
/// CLI subcommand name for `check.eval_minmax@1` (`fid check eval-minmax`).
pub const EVAL_MINMAX_CLI: &str = "eval-minmax";
/// MCP tool name for `check.eval_minmax@1`.
pub const EVAL_MINMAX_MCP: &str = "check_eval_minmax";

const MAX_PARAMS_BYTES: usize = 64 * 1024;
const MAX_PARAMS_DEPTH: usize = 16;
const MAX_CLI_ARGS: usize = 32;
const LIFECYCLE_SINCE: &str = "0.1.0";
const ID_PATTERN: &str = "^[A-Za-z0-9][A-Za-z0-9._:-]{0,127}$";

fn bad(reason: &'static str) -> EvalError {
    EvalError::BadParams { reason }
}

fn req_string<'a>(o: &'a BTreeMap<String, CValue>, k: &str) -> Result<&'a str, EvalError> {
    match o.get(k) {
        Some(CValue::Str(s)) => Ok(s.as_str()),
        Some(_) => Err(bad("parameter is not a string")),
        None => Err(bad("missing required parameter")),
    }
}

fn opt_string<'a>(o: &'a BTreeMap<String, CValue>, k: &str) -> Result<Option<&'a str>, EvalError> {
    match o.get(k) {
        None | Some(CValue::Null) => Ok(None),
        Some(CValue::Str(s)) => Ok(Some(s.as_str())),
        Some(_) => Err(bad("parameter is not a string")),
    }
}

fn opt_bool(o: &BTreeMap<String, CValue>, k: &str) -> Result<bool, EvalError> {
    match o.get(k) {
        None => Ok(false),
        Some(CValue::Bool(b)) => Ok(*b),
        Some(_) => Err(bad("parameter is not a boolean")),
    }
}

fn dispatch_scalar(
    results: &ResultStore,
    inp: &EvalInputs<'_>,
    principal: Principal,
    o: &BTreeMap<String, CValue>,
) -> Result<Evaluated, EvalError> {
    for k in o.keys() {
        if !matches!(k.as_str(), "run" | "char" | "element" | "property" | "tolerance_unit" | "variant" | "dry_run") {
            return Err(bad("unknown parameter"));
        }
    }
    let run = req_string(o, "run")?;
    let char = req_string(o, "char")?;
    let element = req_string(o, "element")?;
    let property = req_string(o, "property")?;
    let tolerance_unit = opt_string(o, "tolerance_unit")?;
    let variant = opt_string(o, "variant")?;
    let dry_run = opt_bool(o, "dry_run")?;
    let req = ScalarRequest { run, char, element, property, tolerance_unit, variant };
    eval_scalar(results, inp, principal, &req, dry_run)
}

fn dispatch_minmax(
    results: &ResultStore,
    inp: &EvalInputs<'_>,
    principal: Principal,
    o: &BTreeMap<String, CValue>,
) -> Result<Evaluated, EvalError> {
    for k in o.keys() {
        if !matches!(k.as_str(), "run" | "char" | "values" | "statistic" | "variant" | "dry_run") {
            return Err(bad("unknown parameter"));
        }
    }
    let run = req_string(o, "run")?;
    let char = req_string(o, "char")?;
    let values = req_string(o, "values")?;
    let statistic = match opt_string(o, "statistic")? {
        None => MinMaxStat::Range,
        Some(s) => MinMaxStat::parse(s).ok_or(bad("invalid statistic"))?,
    };
    let variant = opt_string(o, "variant")?;
    let dry_run = opt_bool(o, "dry_run")?;
    let req = MinMaxRequest { run, char, values, statistic, variant };
    eval_minmax(results, inp, principal, &req, dry_run)
}

/// Runs MCP tool `check_eval_scalar` or `check_eval_minmax` on raw JSON parameters.
pub fn invoke_mcp(
    results: &ResultStore,
    inp: &EvalInputs<'_>,
    principal: Principal,
    tool: &str,
    raw: &[u8],
) -> Result<Evaluated, EvalError> {
    let v = fid_canon::parse_with_limits(raw, MAX_PARAMS_BYTES, MAX_PARAMS_DEPTH).map_err(|e| match e {
        CanonError::InputLimit => bad("params too large or too deep"),
        CanonError::DuplicateKey(_) => bad("duplicate key"),
        _ => bad("params are not valid JSON"),
    })?;
    let CValue::Obj(o) = v else { return Err(bad("params is not an object")) };
    match tool {
        EVAL_SCALAR_MCP => dispatch_scalar(results, inp, principal, &o),
        EVAL_MINMAX_MCP => dispatch_minmax(results, inp, principal, &o),
        _ => Err(bad("unknown tool")),
    }
}

/// Runs CLI `fid check eval-scalar` or `fid check eval-minmax`.
pub fn invoke_cli(
    results: &ResultStore,
    inp: &EvalInputs<'_>,
    principal: Principal,
    args: &[&str],
) -> Result<Evaluated, EvalError> {
    if args.len() > MAX_CLI_ARGS {
        return Err(bad("too many arguments"));
    }
    let (sub, rest) = args.split_first().ok_or(bad("missing subcommand"))?;
    match *sub {
        EVAL_SCALAR_CLI => invoke_scalar_cli(results, inp, principal, rest),
        EVAL_MINMAX_CLI => invoke_minmax_cli(results, inp, principal, rest),
        _ => Err(bad("unknown subcommand")),
    }
}

fn invoke_scalar_cli(
    results: &ResultStore,
    inp: &EvalInputs<'_>,
    principal: Principal,
    rest: &[&str],
) -> Result<Evaluated, EvalError> {
    let mut o = BTreeMap::new();
    let mut it = rest.iter();
    let mut json = false;
    while let Some(arg) = it.next() {
        let key = match *arg {
            "--json" if !json => {
                json = true;
                continue;
            }
            "--json" => return Err(bad("repeated option")),
            "--dry-run" => {
                if o.insert("dry_run".to_owned(), CValue::Bool(true)).is_some() {
                    return Err(bad("repeated option"));
                }
                continue;
            }
            "--run" => "run",
            "--char" => "char",
            "--element" => "element",
            "--property" => "property",
            "--tolerance-unit" | "--unit" => "tolerance_unit",
            "--variant" => "variant",
            s if s.starts_with('-') => return Err(bad("unknown option")),
            _ => return Err(bad("unexpected argument")),
        };
        let value = it.next().ok_or(bad("option needs a value"))?;
        if o.insert(key.to_owned(), CValue::Str((*value).to_owned())).is_some() {
            return Err(bad("repeated option"));
        }
    }
    dispatch_scalar(results, inp, principal, &o)
}

fn invoke_minmax_cli(
    results: &ResultStore,
    inp: &EvalInputs<'_>,
    principal: Principal,
    rest: &[&str],
) -> Result<Evaluated, EvalError> {
    let mut o = BTreeMap::new();
    let mut it = rest.iter();
    let mut json = false;
    while let Some(arg) = it.next() {
        let key = match *arg {
            "--json" if !json => {
                json = true;
                continue;
            }
            "--json" => return Err(bad("repeated option")),
            "--dry-run" => {
                if o.insert("dry_run".to_owned(), CValue::Bool(true)).is_some() {
                    return Err(bad("repeated option"));
                }
                continue;
            }
            "--run" => "run",
            "--char" => "char",
            "--values" => "values",
            "--statistic" | "--stat" => "statistic",
            "--variant" => "variant",
            s if s.starts_with('-') => return Err(bad("unknown option")),
            _ => return Err(bad("unexpected argument")),
        };
        let value = it.next().ok_or(bad("option needs a value"))?;
        if o.insert(key.to_owned(), CValue::Str((*value).to_owned())).is_some() {
            return Err(bad("repeated option"));
        }
    }
    dispatch_minmax(results, inp, principal, &o)
}

fn shared_result_schema(command_id: &str) -> CValue {
    let text = |s: &str| CValue::Str(s.to_owned());
    let strs = |items: &[&str]| CValue::Arr(items.iter().map(|s| text(s)).collect());
    let id_schema = || obj([("pattern", text(ID_PATTERN)), ("type", text("string"))]);
    obj([
        ("additionalProperties", CValue::Bool(false)),
        (
            "properties",
            obj([
                ("algorithm", obj([("type", text("string"))])),
                ("attained_at", obj([("type", strs(&["object", "null"]))])),
                ("branch", obj([("enum", strs(&["agent", "working"])), ("type", text("string"))])),
                ("chain", obj([("type", text("object"))])),
                ("char", id_schema()),
                ("command", obj([("enum", strs(&[command_id])), ("type", text("string"))])),
                ("dry_run", obj([("type", text("boolean"))])),
                ("evaluation_basis", obj([("enum", strs(&["POINTS", "FEATURES"])), ("type", text("string"))])),
                ("interpretation", obj([("type", text("string"))])),
                ("label", obj([("type", text("string"))])),
                ("minmax", obj([("type", strs(&["object", "null"]))])),
                (
                    "quantity_kind",
                    obj([("enum", strs(&["length", "angle", "dimensionless"])), ("type", text("string"))]),
                ),
                ("run", id_schema()),
                ("schema", obj([("type", text("string"))])),
                ("unit", obj([("enum", strs(&["si:m", "si:rad", "si:1"])), ("type", text("string"))])),
                ("value_si", obj([("pattern", text("^0x[0-9a-f]{16}$")), ("type", text("string"))])),
            ]),
        ),
        (
            "required",
            strs(&[
                "algorithm",
                "attained_at",
                "branch",
                "chain",
                "char",
                "command",
                "dry_run",
                "evaluation_basis",
                "interpretation",
                "label",
                "minmax",
                "quantity_kind",
                "run",
                "schema",
                "unit",
                "value_si",
            ]),
        ),
        ("type", text("object")),
    ])
}

/// Returns the validated [`CommandDef`] for `check.eval_scalar@1`.
pub fn scalar_command_def() -> Result<CommandDef, DefError> {
    let text = |s: &str| CValue::Str(s.to_owned());
    let strs = |items: &[&str]| CValue::Arr(items.iter().map(|s| text(s)).collect());
    let id_schema = || obj([("pattern", text(ID_PATTERN)), ("type", text("string"))]);
    let params_schema = obj([
        ("additionalProperties", CValue::Bool(false)),
        (
            "properties",
            obj([
                ("char", id_schema()),
                ("dry_run", obj([("type", text("boolean"))])),
                ("element", id_schema()),
                ("property", obj([("type", text("string"))])),
                ("run", id_schema()),
                ("tolerance_unit", obj([("type", text("string"))])),
                ("variant", id_schema()),
            ]),
        ),
        ("required", strs(&["char", "element", "property", "run"])),
        ("type", text("object")),
    ]);
    CommandDef::from_value(obj([
        ("algorithms", strs(&[SCALAR_ALGORITHM_ID])),
        ("cli_bare", CValue::Bool(false)),
        (
            "description",
            text("Evaluates a typed, registered numeric property of a geometric element and validates its quantity kind."),
        ),
        ("determinism", text("D0")),
        ("effects", strs(&[EVAL_SCALAR_COMMAND])),
        ("exposure", strs(&["cli", "mcp"])),
        ("fs_effect", text("none")),
        ("human_only", CValue::Bool(false)),
        ("id", text("check.eval_scalar")),
        ("idempotency", text("none")),
        ("lifecycle", obj([("since", text(LIFECYCLE_SINCE)), ("state", text("active"))])),
        ("major", CValue::Num(1.0)),
        ("mcp_projection", text("exposed")),
        ("network", text("none")),
        ("params_schema", params_schema),
        ("preconditions", CValue::Arr(Vec::new())),
        ("result_schema", shared_result_schema(EVAL_SCALAR_COMMAND)),
        ("side_effect", text("W2")),
        ("title", text("Evaluate registered scalar property of an element")),
        ("undo", text("head_move")),
        ("web", text("available")),
    ]))
}

/// Returns the validated [`CommandDef`] for `check.eval_minmax@1`.
pub fn minmax_command_def() -> Result<CommandDef, DefError> {
    let text = |s: &str| CValue::Str(s.to_owned());
    let strs = |items: &[&str]| CValue::Arr(items.iter().map(|s| text(s)).collect());
    let id_schema = || obj([("pattern", text(ID_PATTERN)), ("type", text("string"))]);
    let params_schema = obj([
        ("additionalProperties", CValue::Bool(false)),
        (
            "properties",
            obj([
                ("char", id_schema()),
                ("dry_run", obj([("type", text("boolean"))])),
                ("run", id_schema()),
                ("statistic", obj([("enum", strs(&["min", "max", "range"])), ("type", text("string"))])),
                ("values", id_schema()),
                ("variant", id_schema()),
            ]),
        ),
        ("required", strs(&["char", "run", "values"])),
        ("type", text("object")),
    ]);
    CommandDef::from_value(obj([
        ("algorithms", strs(&[MINMAX_ALGORITHM_ID])),
        ("cli_bare", CValue::Bool(false)),
        ("description", text("Evaluates minimum, maximum, and range over a value set with lowest-index tie breaking.")),
        ("determinism", text("D0")),
        ("effects", strs(&[EVAL_MINMAX_COMMAND])),
        ("exposure", strs(&["cli", "mcp"])),
        ("fs_effect", text("none")),
        ("human_only", CValue::Bool(false)),
        ("id", text("check.eval_minmax")),
        ("idempotency", text("none")),
        ("lifecycle", obj([("since", text(LIFECYCLE_SINCE)), ("state", text("active"))])),
        ("major", CValue::Num(1.0)),
        ("mcp_projection", text("exposed")),
        ("network", text("none")),
        ("params_schema", params_schema),
        ("preconditions", CValue::Arr(Vec::new())),
        ("result_schema", shared_result_schema(EVAL_MINMAX_COMMAND)),
        ("side_effect", text("W2")),
        ("title", text("Evaluate min, max, or range over a value set")),
        ("undo", text("head_move")),
        ("web", text("available")),
    ]))
}
