//! The CLI and MCP entry points of `check.eval_diameter@1` and `check.eval_radius@1` and their
//! registry definitions, in the pattern of `gdt.build_drf@1`: both surfaces build the same
//! parameter object and call one handler, so `fid check eval-diameter .. --json` and the MCP
//! tool `check_eval_diameter` return the same result bytes and the same journal event.
//!
//! CLI: `fid check eval-diameter --run <R> --char <C> --feature <F> [--variant <V>]
//! [--gauge-from-apex <metres>] [--dry-run] [--json]`, and `eval-radius` likewise. Output is
//! canonical JSON either way.
//!
//! MCP parameters: `{"run": "run-1", "char": "dia-1", "feature": "f-1", "variant": "ls_circle3",
//! "gauge": {"from_apex": 0.01}, "dry_run": false}` (`variant`, `gauge` and `dry_run`
//! optional).

use super::{eval_radial, EvalError, EvalInputs, EvalRequest, Evaluated, EVAL_DIAMETER_COMMAND, EVAL_RADIUS_COMMAND};
use crate::chars::record::ResultStore;
use fid_canon::{CValue, CanonError};
use fid_gps::dim::radial::{Gauge, Measure};
use fid_io_reg::Principal;
use fid_registry::{CommandDef, DefError};
use std::collections::BTreeMap;

/// CLI subcommand of `fid check` for the diameter.
pub const EVAL_DIAMETER_CLI: &str = "eval-diameter";
/// CLI subcommand of `fid check` for the radius.
pub const EVAL_RADIUS_CLI: &str = "eval-radius";
/// MCP tool name of the diameter.
pub const EVAL_DIAMETER_MCP: &str = "check_eval_diameter";
/// MCP tool name of the radius.
pub const EVAL_RADIUS_MCP: &str = "check_eval_radius";
/// Largest MCP parameter payload, in bytes.
pub const MAX_PARAMS_BYTES: usize = 4 * 1024;
/// Deepest MCP parameter nesting: the object and `gauge`.
const MAX_PARAMS_DEPTH: usize = 2;
/// Most CLI arguments after `fid check`.
const MAX_CLI_ARGS: usize = 14;
const LIFECYCLE_SINCE: &str = "2026-11-21";
const ID_PATTERN: &str = "^[A-Za-z0-9][A-Za-z0-9._:-]{0,127}$";
const PARAMS: [&str; 6] = ["char", "dry_run", "feature", "gauge", "run", "variant"];

fn bad(reason: &'static str) -> EvalError {
    EvalError::BadParams { reason }
}

fn string<'a>(o: &'a BTreeMap<String, CValue>, k: &str) -> Result<Option<&'a str>, EvalError> {
    match o.get(k) {
        None => Ok(None),
        Some(CValue::Str(v)) => Ok(Some(v.as_str())),
        Some(_) => Err(bad("parameter is not a string")),
    }
}

fn required<'a>(o: &'a BTreeMap<String, CValue>, k: &str) -> Result<&'a str, EvalError> {
    string(o, k)?.ok_or(bad("missing parameter"))
}

fn run(
    store: &ResultStore,
    inp: &EvalInputs<'_>,
    principal: Principal,
    measure: Measure,
    o: &BTreeMap<String, CValue>,
) -> Result<Evaluated, EvalError> {
    if o.keys().any(|k| !PARAMS.contains(&k.as_str())) {
        return Err(bad("unknown parameter"));
    }
    let run_id = required(o, "run")?;
    let char = required(o, "char")?;
    let feature = required(o, "feature")?;
    let variant = string(o, "variant")?;
    let gauge = match o.get("gauge") {
        None => None,
        Some(CValue::Obj(g)) if g.len() == 1 => match g.get("from_apex") {
            Some(CValue::Num(d)) => Some(Gauge::FromApex(*d)),
            _ => return Err(bad("gauge is not {from_apex: number}")),
        },
        Some(_) => return Err(bad("gauge is not {from_apex: number}")),
    };
    let dry_run = match o.get("dry_run") {
        None => false,
        Some(CValue::Bool(b)) => *b,
        Some(_) => return Err(bad("parameter is not a boolean")),
    };
    let req = EvalRequest { run: run_id, char, feature, variant, gauge };
    eval_radial(store, inp, principal, measure, &req, dry_run)
}

/// Runs the MCP tool `check_eval_diameter` or `check_eval_radius` on raw JSON parameters (at
/// most [`MAX_PARAMS_BYTES`] bytes and two levels; duplicate keys refused) for `principal`.
pub fn invoke_mcp(
    store: &ResultStore,
    inp: &EvalInputs<'_>,
    principal: Principal,
    tool: &str,
    raw: &[u8],
) -> Result<Evaluated, EvalError> {
    let measure = match tool {
        EVAL_DIAMETER_MCP => Measure::Diameter,
        EVAL_RADIUS_MCP => Measure::Radius,
        _ => return Err(bad("unknown tool")),
    };
    let v = fid_canon::parse_with_limits(raw, MAX_PARAMS_BYTES, MAX_PARAMS_DEPTH).map_err(|e| match e {
        CanonError::InputLimit => bad("params too large or too deep"),
        CanonError::DuplicateKey(_) => bad("duplicate key"),
        _ => bad("params are not valid JSON"),
    })?;
    let CValue::Obj(o) = v else { return Err(bad("params is not an object")) };
    run(store, inp, principal, measure, &o)
}

/// Runs `fid check <args>` for `principal`: `eval-diameter` or `eval-radius` with `--run <R>
/// --char <C> --feature <F> [--variant <V>] [--gauge-from-apex <metres>] [--dry-run]
/// [--json]`. Unknown subcommands, unknown, repeated or value-less options, a gauge that is not
/// a number and stray arguments are `bad_params`.
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
    let measure = match *sub {
        EVAL_DIAMETER_CLI => Measure::Diameter,
        EVAL_RADIUS_CLI => Measure::Radius,
        _ => return Err(bad("unknown subcommand")),
    };
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
            "--feature" => "feature",
            "--variant" => "variant",
            "--gauge-from-apex" => "gauge",
            s if s.starts_with('-') => return Err(bad("unknown option")),
            _ => return Err(bad("unexpected argument")),
        };
        let value = it.next().ok_or(bad("option needs a value"))?;
        let v = if key == "gauge" {
            let d: f64 = value.parse().map_err(|_| bad("gauge is not a number"))?;
            let mut g = BTreeMap::new();
            g.insert("from_apex".to_owned(), CValue::Num(d));
            CValue::Obj(g)
        } else {
            CValue::Str((*value).to_owned())
        };
        if o.insert(key.to_owned(), v).is_some() {
            return Err(bad("repeated option"));
        }
    }
    run(store, inp, principal, measure, &o)
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

fn typed(t: &str) -> CValue {
    obj([("type", text(t))])
}

fn pattern(p: &str) -> CValue {
    obj([("type", text("string")), ("pattern", text(p))])
}

fn one_of(items: &[&str]) -> CValue {
    obj([("type", text("string")), ("enum", strs(items))])
}

fn hex_pattern() -> CValue {
    pattern("^0x[0-9a-f]{16}$")
}

fn closed(required: &[&str], properties: CValue) -> CValue {
    obj([
        ("type", text("object")),
        ("additionalProperties", CValue::Bool(false)),
        ("required", strs(required)),
        ("properties", properties),
    ])
}

/// The `fid.check-radial-result/1` result schema of `measure`'s command.
pub fn result_schema(measure: Measure) -> CValue {
    let digest = pattern(fid_types::Digest::PATTERN);
    let gauge =
        obj([("anyOf", CValue::Arr(vec![closed(&["from_apex"], obj([("from_apex", hex_pattern())])), typed("null")]))]);
    closed(
        &[
            "branch",
            "chain",
            "char",
            "command",
            "dry_run",
            "feature",
            "fit",
            "gauge",
            "interpretation",
            "label",
            "measure",
            "points",
            "run",
            "schema",
            "unit",
            "value",
        ],
        obj([
            ("branch", one_of(&["agent", "working"])),
            (
                "chain",
                closed(
                    &["chain_digest", "dmt_digest", "row", "variant"],
                    obj([
                        ("chain_digest", digest.clone()),
                        ("dmt_digest", digest.clone()),
                        ("row", typed("string")),
                        ("variant", typed("string")),
                    ]),
                ),
            ),
            ("char", pattern(ID_PATTERN)),
            ("command", one_of(&[super::command_of(measure)])),
            ("dry_run", typed("boolean")),
            ("feature", pattern(ID_PATTERN)),
            (
                "fit",
                closed(
                    &["algorithm", "output_digest"],
                    obj([
                        (
                            "algorithm",
                            one_of(&[
                                "fit.circle2.ls@1.0.0",
                                "fit.circle3.ls@1.0.0",
                                "fit.cylinder.ls@1.0.0",
                                "fit.sphere.ls@1.0.0",
                            ]),
                        ),
                        ("output_digest", digest),
                    ]),
                ),
            ),
            ("gauge", gauge),
            ("interpretation", one_of(&["provisional", "signed_off"])),
            ("label", one_of(&[crate::chars::interp::PROVISIONAL_LABEL_TEXT, crate::chars::interp::SIGNED_OFF_LINE])),
            ("measure", one_of(&[measure.as_str()])),
            ("points", typed("integer")),
            ("run", typed("string")),
            ("schema", one_of(&[super::RESULT_SCHEMA])),
            ("unit", one_of(&[super::UNIT])),
            ("value", hex_pattern()),
        ]),
    )
}

fn params_schema() -> CValue {
    closed(
        &["char", "feature", "run"],
        obj([
            ("char", pattern(ID_PATTERN)),
            (
                "dry_run",
                obj([
                    ("type", text("boolean")),
                    ("description", text("Evaluate and return the value; journal and commit nothing.")),
                ]),
            ),
            (
                "feature",
                obj([
                    ("type", text("string")),
                    ("pattern", text(ID_PATTERN)),
                    ("description", text("The measured feature.")),
                ]),
            ),
            (
                "gauge",
                obj([
                    ("type", text("object")),
                    ("additionalProperties", CValue::Bool(false)),
                    ("required", strs(&["from_apex"])),
                    (
                        "properties",
                        obj([(
                            "from_apex",
                            obj([
                                ("type", text("number")),
                                ("exclusiveMinimum", CValue::Num(0.0)),
                                ("description", text("Distance from the cone's apex along its axis, in metres.")),
                            ]),
                        )]),
                    ),
                    ("description", text("Gauge position of a cone (required for a cone, refused otherwise).")),
                ]),
            ),
            (
                "run",
                obj([("type", text("string")), ("minLength", CValue::Num(1.0)), ("maxLength", CValue::Num(128.0))]),
            ),
            (
                "variant",
                obj([
                    ("type", text("string")),
                    ("pattern", text(ID_PATTERN)),
                    ("description", text("DMT variant to use instead of the profile's or the row's default.")),
                ]),
            ),
        ]),
    )
}

fn def(measure: Measure) -> Result<CommandDef, DefError> {
    let (id, title, what) = match measure {
        Measure::Diameter => (EVAL_DIAMETER_COMMAND, "Evaluate diameter", "twice the fitted radius"),
        Measure::Radius => (EVAL_RADIUS_COMMAND, "Evaluate radius", "the fitted radius"),
    };
    let description = format!(
        "Reports {what} of a measured circle, cylinder or sphere, fitted by the least-squares fit the feature is \
         defined with, after checking that fit against the association of the resolved operator chain. The \
         result becomes a ResultRecord with the fit's algorithm ID and the interpretation label."
    );
    let mut m: BTreeMap<String, CValue> = BTreeMap::new();
    for (k, v) in [
        ("id", text(id.split('@').next().unwrap_or(id))),
        ("title", text(title)),
        ("description", text(&description)),
        ("params_schema", params_schema()),
        ("result_schema", result_schema(measure)),
        ("side_effect", text("W2")),
        ("idempotency", text("none")),
        ("undo", text("head_move")),
        ("effects", strs(&[id])),
        ("major", CValue::Num(1.0)),
        ("mcp_projection", text("exposed")),
        ("determinism", text("D0")),
        ("fs_effect", text("none")),
        ("preconditions", CValue::Arr(Vec::new())),
        ("algorithms", CValue::Arr(Vec::new())),
        ("exposure", strs(&["cli", "mcp"])),
        ("lifecycle", obj([("state", text("active")), ("since", text(LIFECYCLE_SINCE))])),
        ("human_only", CValue::Bool(false)),
        ("network", text("none")),
        ("web", text("available")),
        ("cli_bare", CValue::Bool(false)),
    ] {
        m.insert(k.to_owned(), v);
    }
    CommandDef::from_value(CValue::Obj(m))
}

/// The registry definition of `check.eval_diameter@1` (W2, D0, CLI and MCP; a dry run
/// journals and commits nothing), built through [`CommandDef::from_value`].
pub fn diameter_command_def() -> Result<CommandDef, DefError> {
    def(Measure::Diameter)
}

/// The registry definition of `check.eval_radius@1` (W2, D0, CLI and MCP).
pub fn radius_command_def() -> Result<CommandDef, DefError> {
    def(Measure::Radius)
}
