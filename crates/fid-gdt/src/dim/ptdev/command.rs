//! CLI, MCP and registry [`CommandDef`] bindings for `check.eval_deviation@1` (`GDT.DIM.PTDEV`).

use super::{
    eval_deviation, obj, DeviationRequest, DeviationTarget, EvalError, EvalInputs, Evaluated, EVAL_DEVIATION_COMMAND,
};
use crate::chars::record::ResultStore;
use fid_canon::{CValue, CanonError};
use fid_gps::dim::ptdev::{PTDEV_ALGORITHM_ID, SIGN_CONVENTION};
use fid_io_reg::Principal;
use fid_registry::def::{CommandDef, DefError};
use std::collections::BTreeMap;

/// CLI subcommand name for `check.eval_deviation@1` (`fid check eval-deviation`).
pub const EVAL_DEVIATION_CLI: &str = "eval-deviation";
/// MCP tool name for `check.eval_deviation@1`.
pub const EVAL_DEVIATION_MCP: &str = "check_eval_deviation";

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

fn dispatch(
    results: &ResultStore,
    inp: &EvalInputs<'_>,
    principal: Principal,
    o: &BTreeMap<String, CValue>,
) -> Result<Evaluated, EvalError> {
    for k in o.keys() {
        if !matches!(
            k.as_str(),
            "run" | "char" | "points" | "nominals" | "feature" | "nominal" | "frame" | "variant" | "dry_run"
        ) {
            return Err(bad("unknown parameter"));
        }
    }
    let run = req_string(o, "run")?;
    let char = req_string(o, "char")?;
    let frame = req_string(o, "frame")?;
    let variant = opt_string(o, "variant")?;
    let dry_run = opt_bool(o, "dry_run")?;

    let pts = opt_string(o, "points")?;
    let noms = opt_string(o, "nominals")?;
    let feat = opt_string(o, "feature")?;
    let nom = opt_string(o, "nominal")?;

    let target = match (pts, noms, feat, nom) {
        (Some(p), Some(n), None, None) => DeviationTarget::Points { points: p.to_owned(), nominals: n.to_owned() },
        (None, None, Some(f), Some(n)) => DeviationTarget::Feature { feature: f.to_owned(), nominal: n.to_owned() },
        _ => return Err(bad("specify either (points, nominals) or (feature, nominal)")),
    };
    let req = DeviationRequest { run, char, target, frame, variant };
    eval_deviation(results, inp, principal, &req, dry_run)
}

/// Runs MCP tool `check_eval_deviation` on raw JSON parameters.
pub fn invoke_mcp(
    results: &ResultStore,
    inp: &EvalInputs<'_>,
    principal: Principal,
    tool: &str,
    raw: &[u8],
) -> Result<Evaluated, EvalError> {
    if tool != EVAL_DEVIATION_MCP {
        return Err(bad("unknown tool"));
    }
    let v = fid_canon::parse_with_limits(raw, MAX_PARAMS_BYTES, MAX_PARAMS_DEPTH).map_err(|e| match e {
        CanonError::InputLimit => bad("params too large or too deep"),
        CanonError::DuplicateKey(_) => bad("duplicate key"),
        _ => bad("params are not valid JSON"),
    })?;
    let CValue::Obj(o) = v else { return Err(bad("params is not an object")) };
    dispatch(results, inp, principal, &o)
}

/// Runs CLI `fid check eval-deviation`.
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
    if *sub != EVAL_DEVIATION_CLI {
        return Err(bad("unknown subcommand"));
    }
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
            "--points" => "points",
            "--nominals" => "nominals",
            "--feature" => "feature",
            "--nominal" => "nominal",
            "--frame" => "frame",
            "--variant" => "variant",
            s if s.starts_with('-') => return Err(bad("unknown option")),
            _ => return Err(bad("unexpected argument")),
        };
        let value = it.next().ok_or(bad("option needs a value"))?;
        if o.insert(key.to_owned(), CValue::Str((*value).to_owned())).is_some() {
            return Err(bad("repeated option"));
        }
    }
    dispatch(results, inp, principal, &o)
}

/// Returns the validated [`CommandDef`] for `check.eval_deviation@1`.
pub fn deviation_command_def() -> Result<CommandDef, DefError> {
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
                ("feature", id_schema()),
                ("frame", id_schema()),
                ("nominal", id_schema()),
                ("nominals", id_schema()),
                ("points", id_schema()),
                ("run", id_schema()),
                ("variant", id_schema()),
            ]),
        ),
        ("required", strs(&["char", "frame", "run"])),
        ("type", text("object")),
    ]);
    let result_schema = obj([
        ("additionalProperties", CValue::Bool(false)),
        (
            "properties",
            obj([
                ("algorithm", obj([("type", text("string"))])),
                ("attachment", obj([("type", strs(&["object", "null"]))])),
                ("branch", obj([("enum", strs(&["agent", "working"])), ("type", text("string"))])),
                ("chain", obj([("type", text("object"))])),
                ("char", id_schema()),
                ("command", obj([("enum", strs(&[EVAL_DEVIATION_COMMAND])), ("type", text("string"))])),
                ("dry_run", obj([("type", text("boolean"))])),
                ("evaluation_basis", obj([("enum", strs(&["POINTS", "FEATURES"])), ("type", text("string"))])),
                ("feature", obj([("type", strs(&["object", "null"]))])),
                ("frame", id_schema()),
                ("interpretation", obj([("type", text("string"))])),
                ("items", obj([("type", strs(&["array", "null"]))])),
                ("label", obj([("type", text("string"))])),
                ("mode", obj([("enum", strs(&["points", "feature"])), ("type", text("string"))])),
                ("run", id_schema()),
                ("schema", obj([("type", text("string"))])),
                ("sign_convention", obj([("enum", strs(&[SIGN_CONVENTION])), ("type", text("string"))])),
                ("summary", obj([("type", strs(&["object", "null"]))])),
                ("unit", obj([("enum", strs(&["si:m"])), ("type", text("string"))])),
                ("value_si", obj([("pattern", text("^0x[0-9a-f]{16}$")), ("type", text("string"))])),
            ]),
        ),
        (
            "required",
            strs(&[
                "algorithm",
                "attachment",
                "branch",
                "chain",
                "char",
                "command",
                "dry_run",
                "evaluation_basis",
                "feature",
                "frame",
                "interpretation",
                "items",
                "label",
                "mode",
                "run",
                "schema",
                "sign_convention",
                "summary",
                "unit",
                "value_si",
            ]),
        ),
        ("type", text("object")),
    ]);
    CommandDef::from_value(obj([
        ("algorithms", strs(&[PTDEV_ALGORITHM_ID])),
        ("cli_bare", CValue::Bool(false)),
        (
            "description",
            text("Evaluates point deviations (dx, dy, dz in a named frame and signed normal deviation) or feature-versus-nominal deviations."),
        ),
        ("determinism", text("D0")),
        ("effects", strs(&[EVAL_DEVIATION_COMMAND])),
        ("exposure", strs(&["cli", "mcp"])),
        ("fs_effect", text("none")),
        ("human_only", CValue::Bool(false)),
        ("id", text("check.eval_deviation")),
        ("idempotency", text("none")),
        ("lifecycle", obj([("since", text(LIFECYCLE_SINCE)), ("state", text("active"))])),
        ("major", CValue::Num(1.0)),
        ("mcp_projection", text("exposed")),
        ("network", text("none")),
        ("params_schema", params_schema),
        ("preconditions", CValue::Arr(Vec::new())),
        ("result_schema", result_schema),
        ("side_effect", text("W2")),
        ("title", text("Evaluate point or feature-versus-nominal deviations")),
        ("undo", text("head_move")),
        ("web", text("available")),
    ]))
}
