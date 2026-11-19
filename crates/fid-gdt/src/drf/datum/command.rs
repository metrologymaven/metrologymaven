//! The CLI and MCP entry points of `gdt.define_datum@1` and its registry definition, in the
//! pattern of `fid-align` and `GDT.CHAR.RECORD`: both surfaces build the same parameter
//! object and call [`DatumModel::define_datum`] through one handler, so
//! `fid gdt define-datum .. --json` and the MCP tool `gdt_define_datum` return the same
//! result bytes and the same journal event.
//!
//! CLI: `fid gdt define-datum --label A --feature f1 --faces face:12[,face:13]
//! --type plane [--target A1,point,pt:1].. [--replace] [--dry-run] [--json]`.
//!
//! MCP parameters: `{"label": "A", "feature": "f1", "faces": ["face:12"], "type": "plane",
//! "targets": [{"id": "A1", "kind": "point", "geometry": "pt:1"}], "replace": false,
//! "dry_run": false}`; `targets`, `replace` and `dry_run` are optional.

use super::model::{DatumModel, DefineRequest, Defined, TargetRequest};
use super::types::{LabelPattern, MAX_DATUM_FACES, MAX_TARGETS};
use super::{DatumError, DEFINE_COMMAND};
use fid_canon::{CValue, CanonError};
use fid_io_reg::Principal;
use fid_registry::{CommandDef, DefError};
use std::collections::BTreeMap;

/// CLI subcommand of `fid gdt`.
pub const DEFINE_DATUM_CLI: &str = "define-datum";
/// MCP tool name.
pub const DEFINE_DATUM_MCP: &str = "gdt_define_datum";
/// Largest MCP parameter payload, in bytes.
pub const MAX_PARAMS_BYTES: usize = 64 * 1024;
/// Deepest MCP parameter nesting: the object, `targets`, one target.
const MAX_PARAMS_DEPTH: usize = 3;
/// Most CLI arguments after `fid gdt`.
const MAX_CLI_ARGS: usize = 2 * MAX_TARGETS + 16;
const LIFECYCLE_SINCE: &str = "2026-11-19";
const ID_PATTERN: &str = "^[A-Za-z0-9][A-Za-z0-9._:-]{0,127}$";
const PARAMS: [&str; 7] = ["dry_run", "faces", "feature", "label", "replace", "targets", "type"];

fn bad(reason: &'static str) -> DatumError {
    DatumError::BadParams { reason }
}

fn string<'a>(o: &'a BTreeMap<String, CValue>, k: &str) -> Result<&'a str, DatumError> {
    match o.get(k) {
        Some(CValue::Str(s)) => Ok(s),
        Some(_) => Err(bad("parameter is not a string")),
        None => Err(bad("missing parameter")),
    }
}

fn flag(o: &BTreeMap<String, CValue>, k: &str) -> Result<bool, DatumError> {
    match o.get(k) {
        None => Ok(false),
        Some(CValue::Bool(b)) => Ok(*b),
        Some(_) => Err(bad("parameter is not a boolean")),
    }
}

/// The one handler behind both surfaces: reads the parameter object (no unknown key; the
/// four required strings; `faces` a non-empty string array; `targets` an array of
/// `{id, kind, geometry}` strings; `replace` and `dry_run` booleans) and runs the command.
fn run(
    model: &mut DatumModel,
    principal: Principal,
    pattern: &LabelPattern,
    o: &BTreeMap<String, CValue>,
) -> Result<Defined, DatumError> {
    if o.keys().any(|k| !PARAMS.contains(&k.as_str())) {
        return Err(bad("unknown parameter"));
    }
    let faces: Vec<&str> = match o.get("faces") {
        Some(CValue::Arr(a)) if a.len() > MAX_DATUM_FACES => return Err(DatumError::invalid("faces", "too_many")),
        Some(CValue::Arr(a)) => a
            .iter()
            .map(|f| match f {
                CValue::Str(s) => Ok(s.as_str()),
                _ => Err(bad("faces is not a string array")),
            })
            .collect::<Result<_, _>>()?,
        Some(_) => return Err(bad("faces is not a string array")),
        None => return Err(bad("missing parameter")),
    };
    let targets: Vec<TargetRequest<'_>> = match o.get("targets") {
        None => Vec::new(),
        Some(CValue::Arr(a)) if a.len() > MAX_TARGETS => return Err(DatumError::invalid("targets", "too_many")),
        Some(CValue::Arr(a)) => a
            .iter()
            .map(|t| match t {
                CValue::Obj(t) if t.len() == 3 => Ok(TargetRequest {
                    id: string(t, "id")?,
                    kind: string(t, "kind")?,
                    geometry: string(t, "geometry")?,
                }),
                _ => Err(bad("target is not {id, kind, geometry}")),
            })
            .collect::<Result<_, _>>()?,
        Some(_) => return Err(bad("targets is not an array")),
    };
    let req = DefineRequest {
        label: string(o, "label")?,
        feature: string(o, "feature")?,
        faces: &faces,
        feature_type: string(o, "type")?,
        targets: &targets,
        replace: flag(o, "replace")?,
    };
    let dry_run = flag(o, "dry_run")?;
    model.define_datum(principal, pattern, &req, dry_run)
}

/// Runs the MCP tool `gdt_define_datum` on raw JSON parameters (at most
/// [`MAX_PARAMS_BYTES`] bytes and three levels; duplicate keys refused) for `principal`,
/// whose branch the command writes. `pattern` is the label pattern of the plan's active
/// defaults profile ([`super::active_label_pattern`]).
pub fn invoke_mcp(
    model: &mut DatumModel,
    principal: Principal,
    pattern: &LabelPattern,
    tool: &str,
    raw: &[u8],
) -> Result<Defined, DatumError> {
    if tool != DEFINE_DATUM_MCP {
        return Err(bad("unknown tool"));
    }
    let v = fid_canon::parse_with_limits(raw, MAX_PARAMS_BYTES, MAX_PARAMS_DEPTH).map_err(|e| match e {
        CanonError::InputLimit => bad("params too large or too deep"),
        CanonError::DuplicateKey(_) => bad("duplicate key"),
        _ => bad("params are not valid JSON"),
    })?;
    let CValue::Obj(o) = v else { return Err(bad("params is not an object")) };
    run(model, principal, pattern, &o)
}

/// Runs `fid gdt <args>` for `principal`: `define-datum --label <L> --feature <F>
/// --faces <F1>[,<F2>..] --type <T> [--target <ID>,<kind>,<geometry>].. [--replace]
/// [--dry-run] [--json]`. Output ([`Defined::to_json_bytes`]) is canonical JSON either way.
/// Unknown subcommands, unknown, repeated or value-less options, malformed `--target`
/// values and stray arguments are `bad_params`.
pub fn invoke_cli(
    model: &mut DatumModel,
    principal: Principal,
    pattern: &LabelPattern,
    args: &[&str],
) -> Result<Defined, DatumError> {
    if args.len() > MAX_CLI_ARGS {
        return Err(bad("too many arguments"));
    }
    let (sub, rest) = args.split_first().ok_or(bad("missing subcommand"))?;
    if *sub != DEFINE_DATUM_CLI {
        return Err(bad("unknown subcommand"));
    }
    let mut o = BTreeMap::new();
    let mut targets = Vec::new();
    let mut it = rest.iter();
    let mut json = false;
    while let Some(arg) = it.next() {
        let key = match *arg {
            "--json" if !json => {
                json = true;
                continue;
            }
            "--json" => return Err(bad("repeated option")),
            "--replace" | "--dry-run" => {
                let k = if *arg == "--replace" { "replace" } else { "dry_run" };
                if o.insert(k.to_owned(), CValue::Bool(true)).is_some() {
                    return Err(bad("repeated option"));
                }
                continue;
            }
            "--target" => {
                let value = it.next().ok_or(bad("option needs a value"))?;
                let parts: Vec<&str> = value.split(',').collect();
                let [id, kind, geometry] = parts[..] else { return Err(bad("target is not <id>,<kind>,<geometry>")) };
                let t = [("id", id), ("kind", kind), ("geometry", geometry)]
                    .into_iter()
                    .map(|(k, v)| (k.to_owned(), CValue::Str(v.to_owned())))
                    .collect();
                targets.push(CValue::Obj(t));
                continue;
            }
            "--label" => "label",
            "--feature" => "feature",
            "--faces" => "faces",
            "--type" => "type",
            s if s.starts_with('-') => return Err(bad("unknown option")),
            _ => return Err(bad("unexpected argument")),
        };
        let value = it.next().ok_or(bad("option needs a value"))?;
        let v = if key == "faces" {
            CValue::Arr(value.split(',').map(|f| CValue::Str(f.to_owned())).collect())
        } else {
            CValue::Str((*value).to_owned())
        };
        if o.insert(key.to_owned(), v).is_some() {
            return Err(bad("repeated option"));
        }
    }
    if !targets.is_empty() {
        o.insert("targets".to_owned(), CValue::Arr(targets));
    }
    run(model, principal, pattern, &o)
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

fn flag_schema(description: &str) -> CValue {
    obj([("type", text("boolean")), ("description", text(description))])
}

fn definition_schema() -> CValue {
    let target = obj([
        ("type", text("object")),
        ("additionalProperties", CValue::Bool(false)),
        ("required", strs(&["geometry", "id", "kind"])),
        (
            "properties",
            obj([
                ("id", obj([("type", text("string")), ("pattern", text("^[A-Z]{1,3}[1-9][0-9]?$"))])),
                ("kind", obj([("type", text("string")), ("enum", strs(&["area", "line", "point"]))])),
                ("geometry", id_schema("The plan geometry the target refers to.")),
            ]),
        ),
    ]);
    let faces = obj([
        ("type", text("array")),
        ("minItems", CValue::Num(1.0)),
        ("maxItems", CValue::Num(MAX_DATUM_FACES as f64)),
        ("uniqueItems", CValue::Bool(true)),
        ("items", id_schema("A nominal CAD face of the feature.")),
    ]);
    let targets = obj([("type", text("array")), ("maxItems", CValue::Num(MAX_TARGETS as f64)), ("items", target)]);
    obj([
        ("label", obj([("type", text("string")), ("pattern", text("^[A-Z]{1,3}$"))])),
        ("feature", id_schema("The measured feature the datum is bound to.")),
        ("faces", faces),
        ("type", obj([("type", text("string")), ("enum", strs(&["cylinder", "plane"]))])),
        ("targets", targets),
    ])
}

/// The registry definition of `gdt.define_datum@1` (W2, D0, CLI and MCP), built through
/// [`CommandDef::from_value`] so every field rule of the registry applies.
pub fn command_def() -> Result<CommandDef, DefError> {
    let name = DEFINE_COMMAND.split('@').next().unwrap_or(DEFINE_COMMAND);
    let CValue::Obj(mut params) = definition_schema() else {
        return Err(DefError::BadType { field: "params_schema".into(), expected: "an object" });
    };
    params.insert("replace".to_owned(), flag_schema("Replace an existing datum with this label."));
    params.insert("dry_run".to_owned(), flag_schema("Validate only; change and journal nothing."));
    let def = obj([
        ("type", text("object")),
        ("additionalProperties", CValue::Bool(false)),
        ("required", strs(&["faces", "feature", "label", "type"])),
        ("properties", definition_schema()),
    ]);
    CommandDef::from_value(obj([
        ("id", text(name)),
        ("major", CValue::Num(1.0)),
        ("title", text("Define datum")),
        (
            "description",
            text(
                "Declares a datum in the plan: its label, the measured feature and nominal faces it is bound to, the \
                 feature type and optional datum targets. A DRF can only be built from declared datums.",
            ),
        ),
        (
            "params_schema",
            obj([
                ("type", text("object")),
                ("additionalProperties", CValue::Bool(false)),
                ("required", strs(&["faces", "feature", "label", "type"])),
                ("properties", CValue::Obj(params)),
            ]),
        ),
        (
            "result_schema",
            obj([
                ("type", text("object")),
                ("additionalProperties", CValue::Bool(false)),
                ("required", strs(&["branch", "datum", "dry_run", "plan", "replaced"])),
                (
                    "properties",
                    obj([
                        ("branch", obj([("type", text("string")), ("enum", strs(&["agent", "working"]))])),
                        ("datum", def.clone()),
                        ("dry_run", obj([("type", text("boolean"))])),
                        ("plan", id_schema("The plan.")),
                        ("replaced", obj([("anyOf", CValue::Arr(vec![obj([("type", text("null"))]), def]))])),
                    ]),
                ),
            ]),
        ),
        ("side_effect", text("W2")),
        ("mcp_projection", text("exposed")),
        ("determinism", text("D0")),
        ("idempotency", text("none")),
        ("fs_effect", text("none")),
        ("undo", text("head_move")),
        ("preconditions", CValue::Arr(Vec::new())),
        ("effects", strs(&[DEFINE_COMMAND])),
        ("algorithms", CValue::Arr(Vec::new())),
        ("exposure", strs(&["cli", "mcp"])),
        ("lifecycle", obj([("state", text("active")), ("since", text(LIFECYCLE_SINCE))])),
        ("human_only", CValue::Bool(false)),
        ("network", text("none")),
        ("web", text("available")),
        ("cli_bare", CValue::Bool(false)),
    ]))
}
