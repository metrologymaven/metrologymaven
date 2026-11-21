//! The CLI and MCP entry points of `gdt.build_drf@1` and `gdt.get_drf@1` and their registry
//! definitions, in the pattern of `gdt.define_datum@1`: both surfaces build the same parameter
//! object and call one handler, so `fid gdt build-drf .. --json` and the MCP tool
//! `gdt_build_drf` return the same result bytes and the same journal event.
//!
//! CLI: `fid gdt build-drf --char <char> --datums A,B,C [--dry-run] [--json]` and
//! `fid gdt get-drf --id <sha256:..> [--json]`.
//!
//! MCP parameters: `{"char": "drf-1", "datums": ["A", "B", "C"], "dry_run": false}` (`dry_run`
//! optional) and `{"id": "sha256:.."}`.

use super::{build_drf, get_drf, BuildError, BuildInputs, BuildRequest, Built, DrfStore, BUILD_COMMAND, GET_COMMAND};
use fid_canon::{CValue, CanonError};
use fid_io_reg::Principal;
use fid_registry::{CommandDef, DefError};
use std::collections::BTreeMap;

/// CLI subcommand of `fid gdt` for the build.
pub const BUILD_DRF_CLI: &str = "build-drf";
/// CLI subcommand of `fid gdt` for the read.
pub const GET_DRF_CLI: &str = "get-drf";
/// MCP tool name of the build.
pub const BUILD_DRF_MCP: &str = "gdt_build_drf";
/// MCP tool name of the read.
pub const GET_DRF_MCP: &str = "gdt_get_drf";
/// Largest MCP parameter payload, in bytes.
pub const MAX_PARAMS_BYTES: usize = 4 * 1024;
/// Deepest MCP parameter nesting: the object and `datums`.
const MAX_PARAMS_DEPTH: usize = 2;
/// Most CLI arguments after `fid gdt`.
const MAX_CLI_ARGS: usize = 8;
const LIFECYCLE_SINCE: &str = "2026-11-20";
const ID_PATTERN: &str = "^[A-Za-z0-9][A-Za-z0-9._:-]{0,127}$";
/// A datum reference: a label, optionally `(M)` or `(L)` (refused by the command, P1a).
pub const DATUM_REF_PATTERN: &str = "^[A-Z]{1,3}(\\(M\\)|\\(L\\))?$";
const BUILD_PARAMS: [&str; 3] = ["char", "datums", "dry_run"];

/// What a call returns: a build result, or a stored DRF's canonical bytes.
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    /// `gdt.build_drf@1`.
    Built(Built),
    /// `gdt.get_drf@1`: the stored bytes, unchanged.
    Drf(Vec<u8>),
}

impl Outcome {
    /// The `--json` / MCP result bytes: [`Built::to_json_bytes`], or the stored DRF bytes
    /// exactly as stored (AC-5).
    pub fn to_json_bytes(&self) -> Result<Vec<u8>, BuildError> {
        match self {
            Outcome::Built(b) => b.to_json_bytes(),
            Outcome::Drf(bytes) => Ok(bytes.clone()),
        }
    }

    /// The build result, if this was a build.
    pub fn built(&self) -> Option<&Built> {
        match self {
            Outcome::Built(b) => Some(b),
            Outcome::Drf(_) => None,
        }
    }
}

fn bad(reason: &'static str) -> BuildError {
    BuildError::BadParams { reason }
}

fn run_build(
    store: &mut DrfStore,
    inp: &BuildInputs<'_>,
    principal: Principal,
    o: &BTreeMap<String, CValue>,
) -> Result<Outcome, BuildError> {
    if o.keys().any(|k| !BUILD_PARAMS.contains(&k.as_str())) {
        return Err(bad("unknown parameter"));
    }
    let char = match o.get("char") {
        Some(CValue::Str(c)) => c.as_str(),
        Some(_) => return Err(bad("parameter is not a string")),
        None => return Err(bad("missing parameter")),
    };
    let datums: Vec<&str> = match o.get("datums") {
        Some(CValue::Arr(a)) => a
            .iter()
            .map(|d| match d {
                CValue::Str(s) => Ok(s.as_str()),
                _ => Err(bad("datums is not a string array")),
            })
            .collect::<Result<_, _>>()?,
        Some(_) => return Err(bad("datums is not a string array")),
        None => return Err(bad("missing parameter")),
    };
    let dry_run = match o.get("dry_run") {
        None => false,
        Some(CValue::Bool(b)) => *b,
        Some(_) => return Err(bad("parameter is not a boolean")),
    };
    build_drf(store, inp, principal, &BuildRequest { char, datums: &datums }, dry_run).map(Outcome::Built)
}

fn run_get(store: &DrfStore, principal: Principal, o: &BTreeMap<String, CValue>) -> Result<Outcome, BuildError> {
    if o.len() != 1 {
        return Err(bad(if o.is_empty() { "missing parameter" } else { "unknown parameter" }));
    }
    match o.get("id") {
        Some(CValue::Str(id)) => get_drf(store, principal, id).map(|b| Outcome::Drf(b.to_vec())),
        Some(_) => Err(bad("parameter is not a string")),
        None => Err(bad("unknown parameter")),
    }
}

/// Runs the MCP tool `gdt_build_drf` or `gdt_get_drf` on raw JSON parameters (at most
/// [`MAX_PARAMS_BYTES`] bytes and two levels; duplicate keys refused) for `principal`.
pub fn invoke_mcp(
    store: &mut DrfStore,
    inp: &BuildInputs<'_>,
    principal: Principal,
    tool: &str,
    raw: &[u8],
) -> Result<Outcome, BuildError> {
    if tool != BUILD_DRF_MCP && tool != GET_DRF_MCP {
        return Err(bad("unknown tool"));
    }
    let v = fid_canon::parse_with_limits(raw, MAX_PARAMS_BYTES, MAX_PARAMS_DEPTH).map_err(|e| match e {
        CanonError::InputLimit => bad("params too large or too deep"),
        CanonError::DuplicateKey(_) => bad("duplicate key"),
        _ => bad("params are not valid JSON"),
    })?;
    let CValue::Obj(o) = v else { return Err(bad("params is not an object")) };
    if tool == BUILD_DRF_MCP {
        run_build(store, inp, principal, &o)
    } else {
        run_get(store, principal, &o)
    }
}

/// Runs `fid gdt <args>` for `principal`: `build-drf --char <C> --datums <A>[,<B>[,<C>]]
/// [--dry-run] [--json]` or `get-drf --id <ID> [--json]`. Output
/// ([`Outcome::to_json_bytes`]) is canonical JSON either way. Unknown subcommands, unknown,
/// repeated or value-less options and stray arguments are `bad_params`.
pub fn invoke_cli(
    store: &mut DrfStore,
    inp: &BuildInputs<'_>,
    principal: Principal,
    args: &[&str],
) -> Result<Outcome, BuildError> {
    if args.len() > MAX_CLI_ARGS {
        return Err(bad("too many arguments"));
    }
    let (sub, rest) = args.split_first().ok_or(bad("missing subcommand"))?;
    let build = match *sub {
        BUILD_DRF_CLI => true,
        GET_DRF_CLI => false,
        _ => return Err(bad("unknown subcommand")),
    };
    let mut o = BTreeMap::new();
    let mut it = rest.iter();
    let mut json = false;
    while let Some(arg) = it.next() {
        let key = match (*arg, build) {
            ("--json", _) if !json => {
                json = true;
                continue;
            }
            ("--json", _) => return Err(bad("repeated option")),
            ("--dry-run", true) => {
                if o.insert("dry_run".to_owned(), CValue::Bool(true)).is_some() {
                    return Err(bad("repeated option"));
                }
                continue;
            }
            ("--char", true) => "char",
            ("--datums", true) => "datums",
            ("--id", false) => "id",
            (s, _) if s.starts_with('-') => return Err(bad("unknown option")),
            _ => return Err(bad("unexpected argument")),
        };
        let value = it.next().ok_or(bad("option needs a value"))?;
        let v = if key == "datums" {
            CValue::Arr(value.split(',').map(|d| CValue::Str(d.to_owned())).collect())
        } else {
            CValue::Str((*value).to_owned())
        };
        if o.insert(key.to_owned(), v).is_some() {
            return Err(bad("repeated option"));
        }
    }
    if build {
        run_build(store, inp, principal, &o)
    } else {
        run_get(store, principal, &o)
    }
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

fn hex_pattern() -> CValue {
    pattern("^0x[0-9a-f]{16}$")
}

fn vec3() -> CValue {
    obj([
        ("type", text("array")),
        ("minItems", CValue::Num(3.0)),
        ("maxItems", CValue::Num(3.0)),
        ("items", hex_pattern()),
    ])
}

fn dof_list() -> CValue {
    obj([
        ("type", text("array")),
        ("maxItems", CValue::Num(6.0)),
        ("items", obj([("type", text("string")), ("enum", strs(&["tx", "ty", "tz", "rx", "ry", "rz"]))])),
    ])
}

fn closed(required: &[&str], properties: CValue) -> CValue {
    obj([
        ("type", text("object")),
        ("additionalProperties", CValue::Bool(false)),
        ("required", strs(required)),
        ("properties", properties),
    ])
}

/// The `fid.gdt-drf/1` document schema (closed at every level the command writes).
pub fn drf_schema() -> CValue {
    let digest = pattern(fid_types::Digest::PATTERN);
    let fit = closed(
        &[
            "candidates",
            "certificate",
            "contacts",
            "local_optimum_only",
            "max_violation",
            "objective",
            "optimality",
            "orientation",
            "output_digest",
        ],
        obj([
            ("candidates", typed("integer")),
            ("certificate", obj([("anyOf", CValue::Arr(vec![digest.clone(), typed("null")]))])),
            ("contacts", obj([("type", text("array")), ("items", typed("integer"))])),
            ("local_optimum_only", typed("boolean")),
            ("max_violation", hex_pattern()),
            ("objective", hex_pattern()),
            (
                "optimality",
                obj([
                    ("type", text("string")),
                    ("enum", strs(&["closed_form", "closed_form_enumerated", "strict_local"])),
                ]),
            ),
            ("orientation", obj([("type", text("string")), ("enum", strs(&["about_z", "fixed", "free"]))])),
            ("output_digest", digest.clone()),
        ]),
    );
    let datum = closed(
        &[
            "boundary",
            "constrained",
            "dmt_row",
            "feature",
            "fit",
            "kind",
            "label",
            "measurement",
            "nominal_outward",
            "plane",
            "precedence",
            "simulator",
        ],
        obj([
            ("boundary", obj([("type", text("string")), ("enum", strs(&["rmb"]))])),
            ("constrained", dof_list()),
            ("dmt_row", typed("string")),
            ("feature", pattern(ID_PATTERN)),
            ("fit", fit),
            ("kind", obj([("type", text("string")), ("enum", strs(&["plane"]))])),
            ("label", pattern("^[A-Z]{1,3}$")),
            (
                "measurement",
                closed(&["digest", "points"], obj([("digest", digest.clone()), ("points", typed("integer"))])),
            ),
            ("nominal_outward", vec3()),
            ("plane", closed(&["normal", "point"], obj([("normal", vec3()), ("point", vec3())]))),
            ("precedence", obj([("type", text("string")), ("enum", strs(&["primary", "secondary", "tertiary"]))])),
            ("simulator", typed("string")),
        ]),
    );
    let rule = closed(
        &["content_digest", "kind", "reason", "revision", "rule", "status"],
        obj([
            ("content_digest", digest.clone()),
            ("kind", obj([("type", text("string")), ("enum", strs(&["dmt_row", "profile"]))])),
            ("reason", obj([("type", strs(&["string", "null"]))])),
            ("revision", obj([("type", strs(&["integer", "null"]))])),
            ("rule", typed("string")),
            ("status", obj([("type", text("string")), ("enum", strs(&["provisional", "signed_off"]))])),
        ]),
    );
    closed(
        &[
            "axes",
            "chain",
            "char",
            "datums",
            "free",
            "interpretation",
            "interpretation_register",
            "interpretation_rules",
            "origin",
            "plan",
            "schema",
            "values",
        ],
        obj([
            ("axes", closed(&["x", "y", "z"], obj([("x", vec3()), ("y", vec3()), ("z", vec3())]))),
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
            (
                "datums",
                obj([
                    ("type", text("array")),
                    ("minItems", CValue::Num(1.0)),
                    ("maxItems", CValue::Num(3.0)),
                    ("items", datum),
                ]),
            ),
            ("free", dof_list()),
            ("interpretation", obj([("type", text("string")), ("enum", strs(&["provisional", "signed_off"]))])),
            ("interpretation_register", typed("string")),
            ("interpretation_rules", obj([("type", text("array")), ("items", rule)])),
            ("origin", vec3()),
            ("plan", typed("string")),
            ("schema", obj([("type", text("string")), ("enum", strs(&[super::DRF_SCHEMA]))])),
            ("values", obj([("type", text("string")), ("enum", strs(&["measured"]))])),
        ]),
    )
}

fn common(def: Vec<(&str, CValue)>) -> Result<CommandDef, DefError> {
    let mut m: BTreeMap<String, CValue> = def.into_iter().map(|(k, v)| (k.to_owned(), v)).collect();
    for (k, v) in [
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

fn name(id: &str) -> &str {
    id.split('@').next().unwrap_or(id)
}

/// The registry definition of `gdt.build_drf@1` (W2, D0, CLI and MCP; a dry run changes and
/// journals nothing), built through [`CommandDef::from_value`].
pub fn build_command_def() -> Result<CommandDef, DefError> {
    let datums = obj([
        ("type", text("array")),
        ("minItems", CValue::Num(1.0)),
        ("maxItems", CValue::Num(3.0)),
        ("uniqueItems", CValue::Bool(true)),
        ("items", pattern(DATUM_REF_PATTERN)),
        ("description", text("Datum references in precedence order (primary first).")),
    ]);
    common(vec![
        ("id", text(name(BUILD_COMMAND))),
        ("title", text("Build DRF")),
        (
            "description",
            text(
                "Builds a datum reference frame from one to three declared planar datums at RMB, in precedence \
                 order: each datum is simulated from its measured points by a certified tangential plane fit, and the \
                 frame records the DoFs each datum constrains, the free DoFs and the interpretation label.",
            ),
        ),
        (
            "params_schema",
            closed(
                &["char", "datums"],
                obj([
                    ("char", pattern(ID_PATTERN)),
                    ("datums", datums),
                    (
                        "dry_run",
                        obj([
                            ("type", text("boolean")),
                            ("description", text("Build and return the DRF; store and journal nothing.")),
                        ]),
                    ),
                ]),
            ),
        ),
        (
            "result_schema",
            closed(
                &["branch", "drf", "dry_run", "id", "label"],
                obj([
                    ("branch", obj([("type", text("string")), ("enum", strs(&["agent", "working"]))])),
                    ("drf", drf_schema()),
                    ("dry_run", typed("boolean")),
                    ("id", pattern(fid_types::Digest::PATTERN)),
                    (
                        "label",
                        obj([
                            ("type", text("string")),
                            (
                                "enum",
                                strs(&[
                                    crate::chars::interp::PROVISIONAL_LABEL_TEXT,
                                    crate::chars::interp::SIGNED_OFF_LINE,
                                ]),
                            ),
                        ]),
                    ),
                ]),
            ),
        ),
        ("side_effect", text("W2")),
        ("idempotency", text("none")),
        ("undo", text("head_move")),
        ("effects", strs(&[BUILD_COMMAND])),
    ])
}

/// The registry definition of `gdt.get_drf@1` (R0, D0, CLI and MCP).
pub fn get_command_def() -> Result<CommandDef, DefError> {
    common(vec![
        ("id", text(name(GET_COMMAND))),
        ("title", text("Get DRF")),
        ("description", text("A stored datum reference frame, as its canonical JSON, unchanged.")),
        ("params_schema", closed(&["id"], obj([("id", pattern(fid_types::Digest::PATTERN))]))),
        ("result_schema", drf_schema()),
        ("side_effect", text("R0")),
        ("idempotency", text("natural")),
        ("undo", text("not_undoable")),
        ("effects", CValue::Arr(Vec::new())),
    ])
}
