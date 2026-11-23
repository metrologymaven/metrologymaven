//! CLI (`fid check eval-angle`), MCP (`check_eval_angle`) and registry [`CommandDef`] for
//! `check.eval_angle@1` (`GDT.DIM.ANGLE`).

use super::{eval_angle, obj, EvalError, EvalInputs, EvalRequest, Evaluated, TargetB, EVAL_ANGLE_COMMAND};
use crate::chars::record::ResultStore;
use fid_canon::{CValue, CanonError};
use fid_gps::dim::angle::{AngleMode, AuthoredUnit, FrameAxis};
use fid_io_reg::Principal;
use fid_registry::{CommandDef, DefError};
use std::collections::BTreeMap;

/// CLI subcommand of `fid check`.
pub const EVAL_ANGLE_CLI: &str = "eval-angle";
/// MCP tool name.
pub const EVAL_ANGLE_MCP: &str = "check_eval_angle";
/// Largest MCP parameter payload in bytes.
pub const MAX_PARAMS_BYTES: usize = 4 * 1024;
const MAX_PARAMS_DEPTH: usize = 3;
const MAX_CLI_ARGS: usize = 20;
const LIFECYCLE_SINCE: &str = "2026-11-22";
const ID_PATTERN: &str = "^[A-Za-z0-9][A-Za-z0-9._:-]{0,127}$";
const PARAMS: [&str; 10] = ["a", "authored_unit", "b", "b_axis", "char", "dry_run", "mode", "plane", "run", "variant"];

fn bad(reason: &'static str) -> EvalError {
    EvalError::BadParams { reason }
}

fn opt_string<'a>(o: &'a BTreeMap<String, CValue>, k: &str) -> Result<Option<&'a str>, EvalError> {
    match o.get(k) {
        None | Some(CValue::Null) => Ok(None),
        Some(CValue::Str(s)) => Ok(Some(s.as_str())),
        Some(_) => Err(bad("parameter is not a string")),
    }
}

fn req_string<'a>(o: &'a BTreeMap<String, CValue>, k: &str) -> Result<&'a str, EvalError> {
    opt_string(o, k)?.ok_or(bad("missing parameter"))
}

fn dispatch(
    store: &ResultStore,
    inp: &EvalInputs<'_>,
    principal: Principal,
    o: &BTreeMap<String, CValue>,
) -> Result<Evaluated, EvalError> {
    if o.keys().any(|k| !PARAMS.contains(&k.as_str())) {
        return Err(bad("unknown parameter"));
    }
    let run = req_string(o, "run")?;
    let char = req_string(o, "char")?;
    let a = req_string(o, "a")?;
    let b_el = opt_string(o, "b")?;
    let b_ax = opt_string(o, "b_axis")?;
    let b = match (b_el, b_ax) {
        (Some(el), None) => TargetB::Element(el.to_owned()),
        (None, Some(ax)) => TargetB::Axis(FrameAxis::parse(ax).ok_or(bad("invalid frame axis"))?),
        (Some(_), Some(_)) => return Err(bad("specify either b or b_axis, not both")),
        (None, None) => return Err(bad("missing b or b_axis")),
    };
    let mode = match opt_string(o, "mode")? {
        None => AngleMode::Acute,
        Some(s) => AngleMode::parse(s).ok_or(bad("invalid angle mode"))?,
    };
    let authored_unit = match opt_string(o, "authored_unit")? {
        None => AuthoredUnit::Rad,
        Some(s) => AuthoredUnit::parse(s).ok_or(bad("invalid authored unit"))?,
    };
    let plane = opt_string(o, "plane")?;
    let variant = opt_string(o, "variant")?;
    let dry_run = match o.get("dry_run") {
        None => false,
        Some(CValue::Bool(b)) => *b,
        Some(_) => return Err(bad("parameter is not a boolean")),
    };
    let req = EvalRequest { run, char, a, b, mode, plane, authored_unit, variant };
    eval_angle(store, inp, principal, &req, dry_run)
}

/// Runs the MCP tool `check_eval_angle` on raw JSON parameters.
pub fn invoke_mcp(
    store: &ResultStore,
    inp: &EvalInputs<'_>,
    principal: Principal,
    tool: &str,
    raw: &[u8],
) -> Result<Evaluated, EvalError> {
    if tool != EVAL_ANGLE_MCP {
        return Err(bad("unknown tool"));
    }
    let v = fid_canon::parse_with_limits(raw, MAX_PARAMS_BYTES, MAX_PARAMS_DEPTH).map_err(|e| match e {
        CanonError::InputLimit => bad("params too large or too deep"),
        CanonError::DuplicateKey(_) => bad("duplicate key"),
        _ => bad("params are not valid JSON"),
    })?;
    let CValue::Obj(o) = v else { return Err(bad("params is not an object")) };
    dispatch(store, inp, principal, &o)
}

/// Runs `fid check eval-angle --run <R> --char <C> --a <EL> (--b <EL> | --b-axis <AXIS>)
/// [--mode acute|full] [--plane <P>] [--unit rad|deg] [--variant <V>] [--dry-run] [--json]`.
pub fn invoke_cli(
    store: &ResultStore,
    inp: &EvalInputs<'_>,
    principal: Principal,
    args: &[&str],
) -> Result<Evaluated, EvalError> {
    if args.len() > MAX_CLI_ARGS {
        return Err(bad("too many arguments"));
    }
    let (sub, rest) = args.split_first().ok_or(bad("missing subcommand"))?;
    if *sub != EVAL_ANGLE_CLI {
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
            "--a" => "a",
            "--b" => "b",
            "--b-axis" => "b_axis",
            "--mode" => "mode",
            "--plane" => "plane",
            "--unit" => "authored_unit",
            "--variant" => "variant",
            s if s.starts_with('-') => return Err(bad("unknown option")),
            _ => return Err(bad("unexpected argument")),
        };
        let value = it.next().ok_or(bad("option needs a value"))?;
        if o.insert(key.to_owned(), CValue::Str((*value).to_owned())).is_some() {
            return Err(bad("repeated option"));
        }
    }
    dispatch(store, inp, principal, &o)
}

/// Returns the validated [`CommandDef`] for `check.eval_angle@1`.
pub fn angle_command_def() -> Result<CommandDef, DefError> {
    let text = |s: &str| CValue::Str(s.to_owned());
    let strs = |items: &[&str]| CValue::Arr(items.iter().map(|s| text(s)).collect());
    let id_schema = || obj([("pattern", text(ID_PATTERN)), ("type", text("string"))]);
    let params_schema = obj([
        ("additionalProperties", CValue::Bool(false)),
        (
            "properties",
            obj([
                ("a", id_schema()),
                ("authored_unit", obj([("enum", strs(&["rad", "deg"])), ("type", text("string"))])),
                ("b", id_schema()),
                ("b_axis", obj([("enum", strs(&["+X", "-X", "+Y", "-Y", "+Z", "-Z"])), ("type", text("string"))])),
                ("char", id_schema()),
                ("dry_run", obj([("type", text("boolean"))])),
                ("mode", obj([("enum", strs(&["acute", "full"])), ("type", text("string"))])),
                ("plane", id_schema()),
                ("run", id_schema()),
                ("variant", id_schema()),
            ]),
        ),
        ("required", strs(&["a", "char", "run"])),
        ("type", text("object")),
    ]);
    let result_schema = obj([
        ("additionalProperties", CValue::Bool(false)),
        (
            "properties",
            obj([
                ("a", id_schema()),
                ("algorithm", obj([("type", text("string"))])),
                ("authored_unit", obj([("enum", strs(&["rad", "deg"])), ("type", text("string"))])),
                ("authored_value", obj([("pattern", text("^0x[0-9a-f]{16}$")), ("type", text("string"))])),
                ("b", obj([("type", text("object"))])),
                ("branch", obj([("enum", strs(&["agent", "working"])), ("type", text("string"))])),
                ("chain", obj([("type", text("object"))])),
                ("char", id_schema()),
                ("command", obj([("enum", strs(&[EVAL_ANGLE_COMMAND])), ("type", text("string"))])),
                ("dry_run", obj([("type", text("boolean"))])),
                ("interpretation", obj([("type", text("string"))])),
                ("label", obj([("type", text("string"))])),
                ("mode", obj([("enum", strs(&["acute", "full"])), ("type", text("string"))])),
                ("plane", obj([("type", strs(&["string", "null"]))])),
                ("projected", obj([("type", text("boolean"))])),
                ("run", id_schema()),
                ("schema", obj([("type", text("string"))])),
                ("unit", obj([("enum", strs(&["si:rad"])), ("type", text("string"))])),
                ("value_rad", obj([("pattern", text("^0x[0-9a-f]{16}$")), ("type", text("string"))])),
            ]),
        ),
        (
            "required",
            strs(&[
                "a",
                "algorithm",
                "authored_unit",
                "authored_value",
                "b",
                "branch",
                "chain",
                "char",
                "command",
                "dry_run",
                "interpretation",
                "label",
                "mode",
                "plane",
                "projected",
                "run",
                "schema",
                "unit",
                "value_rad",
            ]),
        ),
        ("type", text("object")),
    ]);
    CommandDef::from_value(obj([
        ("algorithms", strs(&["eval.dim.angle@1.0.0"])),
        ("cli_bare", CValue::Bool(false)),
        (
            "description",
            text("Evaluates the angle between two directed elements or an element and a frame axis, optionally projected onto a named plane."),
        ),
        ("determinism", text("D0")),
        ("effects", strs(&[EVAL_ANGLE_COMMAND])),
        ("exposure", strs(&["cli", "mcp"])),
        ("fs_effect", text("none")),
        ("human_only", CValue::Bool(false)),
        ("id", text("check.eval_angle")),
        ("idempotency", text("none")),
        ("lifecycle", obj([("since", text(LIFECYCLE_SINCE)), ("state", text("active"))])),
        ("major", CValue::Num(1.0)),
        ("mcp_projection", text("exposed")),
        ("network", text("none")),
        ("params_schema", params_schema),
        ("preconditions", CValue::Arr(Vec::new())),
        ("result_schema", result_schema),
        ("side_effect", text("W2")),
        ("title", text("Evaluate angle between directed elements")),
        ("undo", text("head_move")),
        ("web", text("available")),
    ]))
}
