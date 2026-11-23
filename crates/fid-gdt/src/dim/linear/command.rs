//! CLI, MCP and registry [`CommandDef`] bindings for `check.eval_distance@1` and
//! `check.eval_caliper@1` (`GDT.DIM.LINEAR`).

use super::{
    echo, eval_caliper, eval_distance, obj, CaliperRequest, DistanceMeasurandSpec, DistanceRequest, EvalError,
    EvalInputs, Evaluated, FrameAxis, EVAL_CALIPER_COMMAND, EVAL_DISTANCE_COMMAND,
};
use crate::chars::record::ResultStore;
use fid_canon::{CValue, CanonError};
use fid_gps::dim::linear::{LengthUnit, CALIPER_ALGORITHM_ID, DISTANCE_ALGORITHM_ID};
use fid_io_reg::Principal;
use fid_registry::def::{CommandDef, DefError};
use std::collections::BTreeMap;

/// CLI subcommand name for `check.eval_distance@1` (`fid check eval-distance`).
pub const EVAL_DISTANCE_CLI: &str = "eval-distance";
/// MCP tool name for `check.eval_distance@1`.
pub const EVAL_DISTANCE_MCP: &str = "check_eval_distance";
/// CLI subcommand name for `check.eval_caliper@1` (`fid check eval-caliper`).
pub const EVAL_CALIPER_CLI: &str = "eval-caliper";
/// MCP tool name for `check.eval_caliper@1`.
pub const EVAL_CALIPER_MCP: &str = "check_eval_caliper";

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

fn parse_measurand_param(v: Option<&CValue>) -> Result<Option<DistanceMeasurandSpec>, EvalError> {
    let Some(val) = v else {
        return Ok(None);
    };
    if matches!(val, CValue::Null) {
        return Ok(None);
    }
    let CValue::Obj(o) = val else {
        return Err(EvalError::MeasurandUnknown { detail: "measurand_not_object".to_owned() });
    };
    let Some(CValue::Str(kind)) = o.get("kind") else {
        return Err(EvalError::MeasurandMissing);
    };
    match kind.as_str() {
        "3d" => Ok(Some(DistanceMeasurandSpec::ThreeD)),
        "projected" => {
            let Some(CValue::Str(plane)) = o.get("plane") else {
                return Err(EvalError::MeasurandUnknown { detail: "projected_plane_missing".to_owned() });
            };
            Ok(Some(DistanceMeasurandSpec::Projected { plane: plane.clone() }))
        }
        "component" => {
            let Some(CValue::Str(frame)) = o.get("frame") else {
                return Err(EvalError::MeasurandUnknown { detail: "component_frame_missing".to_owned() });
            };
            let Some(CValue::Str(axis_str)) = o.get("axis") else {
                return Err(EvalError::MeasurandUnknown { detail: "component_axis_missing".to_owned() });
            };
            let Some(axis) = FrameAxis::parse(axis_str) else {
                return Err(EvalError::MeasurandUnknown { detail: format!("axis:{}", echo(axis_str)) });
            };
            Ok(Some(DistanceMeasurandSpec::Component { frame: frame.clone(), axis }))
        }
        other => Err(EvalError::MeasurandUnknown { detail: echo(other) }),
    }
}

fn parse_vec3_param(v: Option<&CValue>) -> Result<[f64; 3], EvalError> {
    let Some(CValue::Arr(a)) = v else {
        return Err(bad("direction must be a 3-element number array"));
    };
    if a.len() != 3 {
        return Err(bad("direction must have length 3"));
    }
    let mut out = [0.0; 3];
    for (i, item) in a.iter().enumerate() {
        match item {
            CValue::Num(n) => out[i] = *n,
            _ => return Err(bad("direction component is not a number")),
        }
    }
    Ok(out)
}

fn dispatch_distance(
    results: &ResultStore,
    inp: &EvalInputs<'_>,
    principal: Principal,
    o: &BTreeMap<String, CValue>,
) -> Result<Evaluated, EvalError> {
    for k in o.keys() {
        if !matches!(
            k.as_str(),
            "run" | "char" | "a" | "b" | "elements" | "measurand" | "authored_unit" | "variant" | "dry_run"
        ) {
            return Err(bad("unknown parameter"));
        }
    }
    let run = req_string(o, "run")?;
    let char = req_string(o, "char")?;
    let (a, b) = match (o.get("elements"), opt_string(o, "a")?, opt_string(o, "b")?) {
        (Some(CValue::Arr(arr)), None, None) => {
            if arr.len() != 2 {
                return Err(bad("elements must have length 2"));
            }
            match (&arr[0], &arr[1]) {
                (CValue::Str(ea), CValue::Str(eb)) => (ea.as_str(), eb.as_str()),
                _ => return Err(bad("elements must contain strings")),
            }
        }
        (None, Some(ea), Some(eb)) => (ea, eb),
        _ => return Err(bad("specify elements [a, b] or both a and b")),
    };
    let measurand = parse_measurand_param(o.get("measurand"))?;
    let authored_unit = match opt_string(o, "authored_unit")? {
        None => LengthUnit::Millimetre,
        Some(s) => LengthUnit::parse(s).ok_or(bad("invalid authored unit"))?,
    };
    let variant = opt_string(o, "variant")?;
    let dry_run = opt_bool(o, "dry_run")?;
    let req = DistanceRequest { run, char, a, b, measurand, authored_unit, variant };
    eval_distance(results, inp, principal, &req, dry_run)
}

fn dispatch_caliper(
    results: &ResultStore,
    inp: &EvalInputs<'_>,
    principal: Principal,
    o: &BTreeMap<String, CValue>,
) -> Result<Evaluated, EvalError> {
    for k in o.keys() {
        if !matches!(k.as_str(), "run" | "char" | "points" | "direction" | "authored_unit" | "variant" | "dry_run") {
            return Err(bad("unknown parameter"));
        }
    }
    let run = req_string(o, "run")?;
    let char = req_string(o, "char")?;
    let points = req_string(o, "points")?;
    let direction = parse_vec3_param(o.get("direction"))?;
    let authored_unit = match opt_string(o, "authored_unit")? {
        None => LengthUnit::Millimetre,
        Some(s) => LengthUnit::parse(s).ok_or(bad("invalid authored unit"))?,
    };
    let variant = opt_string(o, "variant")?;
    let dry_run = opt_bool(o, "dry_run")?;
    let req = CaliperRequest { run, char, points, direction, authored_unit, variant };
    eval_caliper(results, inp, principal, &req, dry_run)
}

/// Runs MCP tool `check_eval_distance` or `check_eval_caliper` on raw JSON parameters.
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
        EVAL_DISTANCE_MCP => dispatch_distance(results, inp, principal, &o),
        EVAL_CALIPER_MCP => dispatch_caliper(results, inp, principal, &o),
        _ => Err(bad("unknown tool")),
    }
}

/// Runs CLI `fid check eval-distance` or `fid check eval-caliper`.
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
        EVAL_DISTANCE_CLI => invoke_distance_cli(results, inp, principal, rest),
        EVAL_CALIPER_CLI => invoke_caliper_cli(results, inp, principal, rest),
        _ => Err(bad("unknown subcommand")),
    }
}

fn invoke_distance_cli(
    results: &ResultStore,
    inp: &EvalInputs<'_>,
    principal: Principal,
    rest: &[&str],
) -> Result<Evaluated, EvalError> {
    let mut o = BTreeMap::new();
    let mut it = rest.iter();
    let mut json = false;
    while let Some(arg) = it.next() {
        match *arg {
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
            "--measurand" => {
                let raw = it.next().ok_or(bad("option needs a value"))?;
                let val = parse_cli_measurand(raw)?;
                if o.insert("measurand".to_owned(), val).is_some() {
                    return Err(bad("repeated option"));
                }
                continue;
            }
            "--run" | "--char" | "--a" | "--b" | "--unit" | "--variant" => {
                let key = match *arg {
                    "--run" => "run",
                    "--char" => "char",
                    "--a" => "a",
                    "--b" => "b",
                    "--unit" => "authored_unit",
                    "--variant" => "variant",
                    _ => return Err(bad("unknown option")),
                };
                let value = it.next().ok_or(bad("option needs a value"))?;
                if o.insert(key.to_owned(), CValue::Str((*value).to_owned())).is_some() {
                    return Err(bad("repeated option"));
                }
            }
            s if s.starts_with('-') => return Err(bad("unknown option")),
            _ => return Err(bad("unexpected argument")),
        }
    }
    dispatch_distance(results, inp, principal, &o)
}

fn parse_cli_measurand(raw: &str) -> Result<CValue, EvalError> {
    if raw == "3d" {
        return Ok(obj([("kind", CValue::Str("3d".to_owned()))]));
    }
    if let Some(plane) = raw.strip_prefix("projected:") {
        return Ok(obj([("kind", CValue::Str("projected".to_owned())), ("plane", CValue::Str(plane.to_owned()))]));
    }
    if let Some(rest) = raw.strip_prefix("component:") {
        let mut parts = rest.split(':');
        let frame = parts.next().unwrap_or("");
        let axis = parts.next().unwrap_or("");
        if parts.next().is_some() || frame.is_empty() || axis.is_empty() {
            return Err(EvalError::MeasurandUnknown { detail: echo(raw) });
        }
        return Ok(obj([
            ("axis", CValue::Str(axis.to_owned())),
            ("frame", CValue::Str(frame.to_owned())),
            ("kind", CValue::Str("component".to_owned())),
        ]));
    }
    Err(EvalError::MeasurandUnknown { detail: echo(raw) })
}

fn invoke_caliper_cli(
    results: &ResultStore,
    inp: &EvalInputs<'_>,
    principal: Principal,
    rest: &[&str],
) -> Result<Evaluated, EvalError> {
    let mut o = BTreeMap::new();
    let mut it = rest.iter();
    let mut json = false;
    while let Some(arg) = it.next() {
        match *arg {
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
            "--dir" | "--direction" => {
                let raw = it.next().ok_or(bad("option needs a value"))?;
                let parts: Vec<&str> = raw.split(',').collect();
                if parts.len() != 3 {
                    return Err(bad("direction must be dx,dy,dz"));
                }
                let mut arr = Vec::with_capacity(3);
                for p in parts {
                    let n: f64 = p.trim().parse().map_err(|_| bad("direction component is not a number"))?;
                    arr.push(CValue::Num(n));
                }
                if o.insert("direction".to_owned(), CValue::Arr(arr)).is_some() {
                    return Err(bad("repeated option"));
                }
            }
            "--run" | "--char" | "--points" | "--unit" | "--variant" => {
                let key = match *arg {
                    "--run" => "run",
                    "--char" => "char",
                    "--points" => "points",
                    "--unit" => "authored_unit",
                    "--variant" => "variant",
                    _ => return Err(bad("unknown option")),
                };
                let value = it.next().ok_or(bad("option needs a value"))?;
                if o.insert(key.to_owned(), CValue::Str((*value).to_owned())).is_some() {
                    return Err(bad("repeated option"));
                }
            }
            s if s.starts_with('-') => return Err(bad("unknown option")),
            _ => return Err(bad("unexpected argument")),
        }
    }
    dispatch_caliper(results, inp, principal, &o)
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
                ("authored_decimal", obj([("type", text("string"))])),
                ("authored_unit", obj([("enum", strs(&["m", "mm", "um", "in"])), ("type", text("string"))])),
                ("authored_value", obj([("pattern", text("^0x[0-9a-f]{16}$")), ("type", text("string"))])),
                ("branch", obj([("enum", strs(&["agent", "working"])), ("type", text("string"))])),
                ("chain", obj([("type", text("object"))])),
                ("char", id_schema()),
                ("command", obj([("enum", strs(&[command_id])), ("type", text("string"))])),
                ("conversion", obj([("type", text("object"))])),
                ("dry_run", obj([("type", text("boolean"))])),
                ("evaluation_basis", obj([("enum", strs(&["POINTS", "FEATURES"])), ("type", text("string"))])),
                ("extreme_indices", obj([("type", strs(&["array", "null"]))])),
                ("flags", obj([("type", text("array"))])),
                ("interpretation", obj([("type", text("string"))])),
                ("label", obj([("type", text("string"))])),
                ("reference_segment", obj([("type", strs(&["object", "null"]))])),
                ("run", id_schema()),
                ("schema", obj([("type", text("string"))])),
                ("unit", obj([("enum", strs(&["si:m"])), ("type", text("string"))])),
                ("value_si", obj([("pattern", text("^0x[0-9a-f]{16}$")), ("type", text("string"))])),
            ]),
        ),
        (
            "required",
            strs(&[
                "algorithm",
                "authored_decimal",
                "authored_unit",
                "authored_value",
                "branch",
                "chain",
                "char",
                "command",
                "conversion",
                "dry_run",
                "evaluation_basis",
                "extreme_indices",
                "flags",
                "interpretation",
                "label",
                "reference_segment",
                "run",
                "schema",
                "unit",
                "value_si",
            ]),
        ),
        ("type", text("object")),
    ])
}

/// Returns the validated [`CommandDef`] for `check.eval_distance@1`.
pub fn distance_command_def() -> Result<CommandDef, DefError> {
    let text = |s: &str| CValue::Str(s.to_owned());
    let strs = |items: &[&str]| CValue::Arr(items.iter().map(|s| text(s)).collect());
    let id_schema = || obj([("pattern", text(ID_PATTERN)), ("type", text("string"))]);
    let params_schema = obj([
        ("additionalProperties", CValue::Bool(false)),
        (
            "properties",
            obj([
                ("a", id_schema()),
                ("authored_unit", obj([("enum", strs(&["m", "mm", "um", "in"])), ("type", text("string"))])),
                ("b", id_schema()),
                ("char", id_schema()),
                ("dry_run", obj([("type", text("boolean"))])),
                ("elements", obj([("type", text("array"))])),
                ("measurand", obj([("type", text("object"))])),
                ("run", id_schema()),
                ("variant", id_schema()),
            ]),
        ),
        ("required", strs(&["char", "measurand", "run"])),
        ("type", text("object")),
    ]);
    CommandDef::from_value(obj([
        ("algorithms", strs(&[DISTANCE_ALGORITHM_ID])),
        ("cli_bare", CValue::Bool(false)),
        (
            "description",
            text("Evaluates point-to-point or element-to-element distance (3D, projected onto a named plane, or axis component of a named frame)."),
        ),
        ("determinism", text("D0")),
        ("effects", strs(&[EVAL_DISTANCE_COMMAND])),
        ("exposure", strs(&["cli", "mcp"])),
        ("fs_effect", text("none")),
        ("human_only", CValue::Bool(false)),
        ("id", text("check.eval_distance")),
        ("idempotency", text("none")),
        ("lifecycle", obj([("since", text(LIFECYCLE_SINCE)), ("state", text("active"))])),
        ("major", CValue::Num(1.0)),
        ("mcp_projection", text("exposed")),
        ("network", text("none")),
        ("params_schema", params_schema),
        ("preconditions", CValue::Arr(Vec::new())),
        ("result_schema", shared_result_schema(EVAL_DISTANCE_COMMAND)),
        ("side_effect", text("W2")),
        ("title", text("Evaluate linear distance between two elements")),
        ("undo", text("head_move")),
        ("web", text("available")),
    ]))
}

/// Returns the validated [`CommandDef`] for `check.eval_caliper@1`.
pub fn caliper_command_def() -> Result<CommandDef, DefError> {
    let text = |s: &str| CValue::Str(s.to_owned());
    let strs = |items: &[&str]| CValue::Arr(items.iter().map(|s| text(s)).collect());
    let id_schema = || obj([("pattern", text(ID_PATTERN)), ("type", text("string"))]);
    let params_schema = obj([
        ("additionalProperties", CValue::Bool(false)),
        (
            "properties",
            obj([
                ("authored_unit", obj([("enum", strs(&["m", "mm", "um", "in"])), ("type", text("string"))])),
                ("char", id_schema()),
                ("direction", obj([("type", text("array"))])),
                ("dry_run", obj([("type", text("boolean"))])),
                ("points", id_schema()),
                ("run", id_schema()),
                ("variant", id_schema()),
            ]),
        ),
        ("required", strs(&["char", "direction", "points", "run"])),
        ("type", text("object")),
    ]);
    CommandDef::from_value(obj([
        ("algorithms", strs(&[CALIPER_ALGORITHM_ID])),
        ("cli_bare", CValue::Bool(false)),
        (
            "description",
            text("Evaluates caliper extent max(p·d) - min(p·d) over a point set along a direction, with extreme point indices."),
        ),
        ("determinism", text("D0")),
        ("effects", strs(&[EVAL_CALIPER_COMMAND])),
        ("exposure", strs(&["cli", "mcp"])),
        ("fs_effect", text("none")),
        ("human_only", CValue::Bool(false)),
        ("id", text("check.eval_caliper")),
        ("idempotency", text("none")),
        ("lifecycle", obj([("since", text(LIFECYCLE_SINCE)), ("state", text("active"))])),
        ("major", CValue::Num(1.0)),
        ("mcp_projection", text("exposed")),
        ("network", text("none")),
        ("params_schema", params_schema),
        ("preconditions", CValue::Arr(Vec::new())),
        ("result_schema", shared_result_schema(EVAL_CALIPER_COMMAND)),
        ("side_effect", text("W2")),
        ("title", text("Evaluate caliper extent along a direction")),
        ("undo", text("head_move")),
        ("web", text("available")),
    ]))
}
