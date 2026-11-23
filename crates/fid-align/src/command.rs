//! `align.list_alignments@1` and `align.get_alignment@1` (R0, D0; ALN.MODEL.ELEM REQ-008)
//! and `align.get_residuals@1` (R0, D0; ALN.QA.REPORT REQ-005, in [`crate::residuals`]) and
//! `align.get_diagnostics@1` (R0, D0; ALN.QA.COND REQ-005, in [`crate::diagnostics`]):
//! parameters, results, registry definitions, and the CLI and MCP entry points.
//!
//! Both surfaces build the same typed parameters and call the same handler, which returns
//! RFC 8785 canonical bytes, so `fid align list-alignments --json` and the MCP tool
//! `align_list_alignments` return identical bytes (and likewise for every command). Every call is recorded in the caller's
//! Activity Log; R0 calls never reach the Record Journal (REQ-CORE.JRN-016).

use crate::diagnostics::{self, DiagnosticsParams, CLI_DIAGNOSTICS, GET_DIAGNOSTICS, MCP_DIAGNOSTICS};
use crate::element::{obj, parse_id, AlignmentStatus};
use crate::error::{kept, AlignError};
use crate::model::AlignmentModel;
use crate::residuals::{self, ResidualStore, ResidualsParams, CLI_RESIDUALS, GET_RESIDUALS, MAX_PAGE, MCP_RESIDUALS};
use fid_canon::{parse_with_limits, to_canonical_bytes, CValue, CanonError};
use fid_registry::{CommandDef, DefError};
use fid_uuid::Uuid;

/// Command ID of the list query.
pub const LIST_ALIGNMENTS: &str = "align.list_alignments@1";
/// Command ID of the get query.
pub const GET_ALIGNMENT: &str = "align.get_alignment@1";
/// CLI subcommand of `fid align` for the list query.
pub const CLI_LIST: &str = "list-alignments";
/// CLI subcommand of `fid align` for the get query.
pub const CLI_GET: &str = "get-alignment";
/// MCP tool name of the list query.
pub const MCP_LIST: &str = "align_list_alignments";
/// MCP tool name of the get query.
pub const MCP_GET: &str = "align_get_alignment";
/// Largest MCP parameter payload, in bytes.
pub const MAX_PARAMS_BYTES: usize = 64 * 1024;
/// Deepest MCP parameter nesting.
pub const MAX_PARAMS_DEPTH: usize = 8;
/// Most CLI arguments after `fid align`.
const MAX_CLI_ARGS: usize = 16;
const LIFECYCLE_SINCE: &str = "2026-11-19";
pub(crate) const ID_PATTERN: &str = "^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$";

/// The surface a call came through.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Surface {
    Cli,
    Mcp,
}

/// One Activity Log record of an R0 call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActivityEntry {
    pub command: &'static str,
    pub surface: Surface,
}

fn id_param(v: &CValue, what: &'static str) -> Result<Uuid, AlignError> {
    match v {
        CValue::Str(s) => parse_id(s).ok_or(AlignError::BadParams(what)),
        _ => Err(AlignError::BadParams(what)),
    }
}

/// Parameters of `align.list_alignments@1`: optional `parent` (an alignment ID) and optional
/// `status`; both filters must hold.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ListParams {
    pub parent: Option<Uuid>,
    pub status: Option<AlignmentStatus>,
}

impl ListParams {
    /// Reads the parameter object. Refuses an unknown key or a malformed value
    /// (`bad_params`) and an unknown status (`unknown_kind`).
    pub fn from_value(v: &CValue) -> Result<ListParams, AlignError> {
        let CValue::Obj(o) = v else { return Err(AlignError::BadParams("params is not an object")) };
        if o.keys().any(|k| k != "parent" && k != "status") {
            return Err(AlignError::BadParams("unknown parameter"));
        }
        let parent = o.get("parent").map(|p| id_param(p, "parent is not a canonical id")).transpose()?;
        let status = match o.get("status") {
            None => None,
            Some(CValue::Str(s)) => Some(AlignmentStatus::parse(s)?),
            Some(_) => return Err(AlignError::BadParams("status is not a string")),
        };
        Ok(ListParams { parent, status })
    }
}

/// Parameters of `align.get_alignment@1`: `id`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GetParams {
    pub id: Uuid,
}

impl GetParams {
    /// Reads the parameter object; fails closed like [`ListParams::from_value`].
    pub fn from_value(v: &CValue) -> Result<GetParams, AlignError> {
        let CValue::Obj(o) = v else { return Err(AlignError::BadParams("params is not an object")) };
        if o.keys().any(|k| k != "id") {
            return Err(AlignError::BadParams("unknown parameter"));
        }
        let id = o.get("id").ok_or(AlignError::BadParams("missing id"))?;
        Ok(GetParams { id: id_param(id, "id is not a canonical id")? })
    }
}

/// The summaries of the matching alignments, ascending by ID.
pub fn list_alignments(model: &AlignmentModel, params: &ListParams) -> CValue {
    CValue::Arr(
        model
            .iter()
            .filter(|a| params.parent.map_or(true, |p| a.parent().map(|r| r.id) == Some(p)))
            .filter(|a| params.status.map_or(true, |s| a.status() == s))
            .map(|a| a.summary_value())
            .collect(),
    )
}

/// The full element form of one alignment; `not_found` if there is none.
pub fn get_alignment(model: &AlignmentModel, params: &GetParams) -> Result<CValue, AlignError> {
    model.get(&params.id).map(|a| a.to_value()).ok_or(AlignError::NotFound(params.id))
}

/// The one handler behind both surfaces.
fn run(
    model: &AlignmentModel,
    reports: &ResidualStore,
    command: &'static str,
    params: &CValue,
) -> Result<Vec<u8>, AlignError> {
    let result = if command == LIST_ALIGNMENTS {
        list_alignments(model, &ListParams::from_value(params)?)
    } else if command == GET_RESIDUALS {
        residuals::get_residuals(model, reports, &ResidualsParams::from_value(params)?)?
    } else if command == GET_DIAGNOSTICS {
        diagnostics::get_diagnostics(model, reports, &DiagnosticsParams::from_value(params)?)?
    } else {
        get_alignment(model, &GetParams::from_value(params)?)?
    };
    to_canonical_bytes(&result).map_err(|_| AlignError::Internal("canonical json"))
}

/// [`invoke_mcp_with`] on a project without residual reports (`align_get_residuals` then
/// fails `not_found`).
pub fn invoke_mcp(
    model: &AlignmentModel,
    log: &mut Vec<ActivityEntry>,
    tool: &str,
    params: &[u8],
) -> Result<Vec<u8>, AlignError> {
    invoke_mcp_with(model, &ResidualStore::new(), log, tool, params)
}

/// Runs an MCP tool call (`align_list_alignments`, `align_get_alignment`,
/// `align_get_residuals` or `align_get_diagnostics`) on raw JSON parameters (at most [`MAX_PARAMS_BYTES`] bytes and
/// [`MAX_PARAMS_DEPTH`] levels; duplicate keys refused) and returns the canonical result
/// bytes. The call is logged to `log`.
pub fn invoke_mcp_with(
    model: &AlignmentModel,
    reports: &ResidualStore,
    log: &mut Vec<ActivityEntry>,
    tool: &str,
    params: &[u8],
) -> Result<Vec<u8>, AlignError> {
    let command = match tool {
        MCP_LIST => LIST_ALIGNMENTS,
        MCP_GET => GET_ALIGNMENT,
        MCP_RESIDUALS => GET_RESIDUALS,
        MCP_DIAGNOSTICS => GET_DIAGNOSTICS,
        other => return Err(AlignError::UnknownKind(kept(other))),
    };
    log.push(ActivityEntry { command, surface: Surface::Mcp });
    let value = parse_with_limits(params, MAX_PARAMS_BYTES, MAX_PARAMS_DEPTH).map_err(|e| match e {
        CanonError::InputLimit => AlignError::InputLimit("params"),
        CanonError::DuplicateKey(_) => AlignError::BadParams("duplicate key"),
        _ => AlignError::BadParams("params are not valid JSON"),
    })?;
    run(model, reports, command, &value)
}

/// [`invoke_cli_with`] on a project without residual reports.
pub fn invoke_cli(model: &AlignmentModel, log: &mut Vec<ActivityEntry>, args: &[&str]) -> Result<Vec<u8>, AlignError> {
    invoke_cli_with(model, &ResidualStore::new(), log, args)
}

/// Runs `fid align <args>`: `list-alignments [--parent <id>] [--status <status>] [--json]`,
/// `get-alignment <id> [--json]` or `get-residuals <alignment> [--summary-only]
/// [--offset N] [--limit M] [--digest D] [--json]` or `get-diagnostics <alignment> [--json]`.
/// Output is canonical JSON either way.
/// Unknown or repeated options are refused. The call is logged to `log`.
pub fn invoke_cli_with(
    model: &AlignmentModel,
    reports: &ResidualStore,
    log: &mut Vec<ActivityEntry>,
    args: &[&str],
) -> Result<Vec<u8>, AlignError> {
    if args.len() > MAX_CLI_ARGS {
        return Err(AlignError::InputLimit("args"));
    }
    let (sub, rest) = args.split_first().ok_or(AlignError::BadParams("missing subcommand"))?;
    let command = match *sub {
        CLI_LIST => LIST_ALIGNMENTS,
        CLI_GET => GET_ALIGNMENT,
        CLI_RESIDUALS => GET_RESIDUALS,
        CLI_DIAGNOSTICS => GET_DIAGNOSTICS,
        other => return Err(AlignError::UnknownKind(kept(other))),
    };
    log.push(ActivityEntry { command, surface: Surface::Cli });
    if command == GET_RESIDUALS {
        return run(model, reports, command, &residuals::cli_params(rest)?);
    }
    if command == GET_DIAGNOSTICS {
        return run(model, reports, command, &diagnostics::cli_params(rest)?);
    }
    let mut params = std::collections::BTreeMap::new();
    let mut json = false;
    let mut it = rest.iter();
    while let Some(arg) = it.next() {
        let key = match *arg {
            "--json" if !json => {
                json = true;
                continue;
            }
            "--json" => return Err(AlignError::BadParams("repeated option")),
            "--parent" if command == LIST_ALIGNMENTS => "parent",
            "--status" if command == LIST_ALIGNMENTS => "status",
            s if s.starts_with("--") => return Err(AlignError::BadParams("unknown option")),
            _ if command == GET_ALIGNMENT => "id",
            _ => return Err(AlignError::BadParams("unexpected argument")),
        };
        let value = if key == "id" { *arg } else { *it.next().ok_or(AlignError::BadParams("option needs a value"))? };
        if params.insert(key.to_string(), CValue::Str(value.to_string())).is_some() {
            return Err(AlignError::BadParams("repeated option"));
        }
    }
    run(model, reports, command, &CValue::Obj(params))
}

// ------------------------------------------------------------------ registry definitions

fn text(s: &str) -> CValue {
    CValue::Str(s.to_string())
}

fn strs(items: &[&str]) -> CValue {
    CValue::Arr(items.iter().map(|s| text(s)).collect())
}

fn id_schema(description: &str) -> CValue {
    obj([
        ("type", text("string")),
        ("x-ref", text("alignment")),
        ("pattern", text(ID_PATTERN)),
        ("description", text(description)),
    ])
}

fn status_schema(description: &str) -> CValue {
    let names: Vec<&str> = AlignmentStatus::ALL.iter().map(|s| s.as_str()).collect();
    obj([("type", text("string")), ("enum", strs(&names)), ("description", text(description))])
}

fn summary_schema() -> CValue {
    obj([
        ("type", text("object")),
        ("additionalProperties", CValue::Bool(false)),
        ("required", strs(&["algorithm", "digest", "id", "parent", "status"])),
        (
            "properties",
            obj([
                ("algorithm", obj([("type", text("string"))])),
                ("digest", obj([("type", text("string")), ("pattern", text("^sha256:[0-9a-f]{64}$"))])),
                ("id", obj([("type", text("string")), ("pattern", text(ID_PATTERN))])),
                ("parent", obj([("type", strs(&["string", "null"]))])),
                ("status", status_schema("Alignment status.")),
            ]),
        ),
    ])
}

fn def(
    name: &str,
    title: &str,
    description: &str,
    params_schema: CValue,
    result_schema: CValue,
) -> Result<CommandDef, DefError> {
    CommandDef::from_value(obj([
        ("id", text(name)),
        ("major", CValue::Num(1.0)),
        ("title", text(title)),
        ("description", text(description)),
        ("params_schema", params_schema),
        ("result_schema", result_schema),
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

/// The registry definitions of `align.get_alignment@1`, `align.get_diagnostics@1`,
/// `align.get_residuals@1`, `align.list_alignments@1` and `align.set_parent@1`, in that
/// (ascending) order, built through [`CommandDef::from_value`].
pub fn command_defs() -> Result<Vec<CommandDef>, DefError> {
    let get_params = obj([
        ("type", text("object")),
        ("additionalProperties", CValue::Bool(false)),
        ("required", strs(&["id"])),
        ("properties", obj([("id", id_schema("The alignment to return."))])),
    ]);
    let list_params = obj([
        ("type", text("object")),
        ("additionalProperties", CValue::Bool(false)),
        ("required", CValue::Arr(Vec::new())),
        (
            "properties",
            obj([
                ("parent", id_schema("Only alignments whose parent is this alignment.")),
                ("status", status_schema("Only alignments with this status.")),
            ]),
        ),
    ]);
    let residuals_params = obj([
        ("type", text("object")),
        ("additionalProperties", CValue::Bool(false)),
        ("required", strs(&["alignment"])),
        (
            "properties",
            obj([
                ("alignment", id_schema("The alignment whose residual report to read.")),
                (
                    "digest",
                    obj([
                        ("type", text("string")),
                        ("pattern", text("^sha256:[0-9a-f]{64}$")),
                        ("description", text("The report digest paging started from; report_changed if it moved.")),
                    ]),
                ),
                (
                    "limit",
                    obj([
                        ("type", text("integer")),
                        ("minimum", CValue::Num(1.0)),
                        ("maximum", CValue::Num(MAX_PAGE as f64)),
                        ("description", text("Entries per page (default 1000).")),
                    ]),
                ),
                (
                    "offset",
                    obj([
                        ("type", text("integer")),
                        ("minimum", CValue::Num(0.0)),
                        (
                            "description",
                            text("Index of the first entry of the page (default 0); above 0 needs digest."),
                        ),
                    ]),
                ),
                (
                    "summary_only",
                    obj([("type", text("boolean")), ("description", text("Return only the summary with its digest."))]),
                ),
            ]),
        ),
    ]);
    let diagnostics_params = obj([
        ("type", text("object")),
        ("additionalProperties", CValue::Bool(false)),
        ("required", strs(&["alignment"])),
        ("properties", obj([("alignment", id_schema("The alignment whose diagnostics to read."))])),
    ]);
    let list_result = obj([("type", text("array")), ("items", summary_schema())]);
    let get_result = obj([("type", text("object"))]);
    Ok(vec![
        def(
            "align.get_alignment",
            "Get alignment",
            "Returns one alignment element with every stored field and its content digest.",
            get_params,
            get_result,
        )?,
        def(
            "align.get_diagnostics",
            "Get diagnostics",
            "Returns an alignment's conditioning diagnostics (length-scaled singular values, condition number, \
             weakest degree of freedom, ill_conditioned and unobservable_dof warnings with the threshold and its \
             basis) and the convergence history of an iterative method (null for closed-form methods).",
            diagnostics_params,
            diagnostics::result_schema(),
        )?,
        def(
            "align.get_residuals",
            "Get residuals",
            "Returns an alignment's residual report: the summary (RMS, weighted RMS, maximum absolute residual, \
             used and rejected counts, all with units, and the report digest) and optionally one page of entries.",
            residuals_params,
            residuals::result_schema(),
        )?,
        def(
            "align.list_alignments",
            "List alignments",
            "Lists alignment summaries sorted by alignment ID, optionally filtered by parent and status.",
            list_params,
            list_result,
        )?,
        crate::hier::set_parent_command_def()?,
    ])
}
