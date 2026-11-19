//! `rdl.list_dataset@1` and `rdl.show_dataset@1` (R0, D0; REQ-UNC.RDL.FORMAT-006): the
//! record store they read, parameters, results, registry definitions, the CLI and MCP entry
//! points, the activity record and exact-mode replay (AC-3, AC-5, AC-6).
//!
//! Both surfaces build the same typed parameters and call one handler, which returns a
//! result envelope (AUTO.SURF.ENV), so `fid rdl list-dataset --json` and the MCP tool
//! `rdl_list_dataset` print the same canonical bytes. Results only ever hold public views
//! ([`crate::public_view`]); no code path here returns a full record.
//!
//! Journaling: R0 commands are not written to the Record Journal; they go to the Activity
//! Log (CONSTITUTION §3, D-031). Every call whose command and parameters parse appends one
//! [`ActivityEntry`] holding the command ID, the surface, the canonical parameters and the
//! SHA-256 of the canonical envelope bytes, failures (`dataset_not_found`) included.
//! [`replay`] reruns the entry against a store and compares that digest.

use crate::error::{bad_params, invalid, RdlError};
use crate::record::{aim_ok, obj, parse_id, RdlDataset, Tier};
use crate::view::public_view;
use fid_canon::{parse_with_limits, to_canonical_bytes, CValue};
use fid_registry::{CommandDef, DefError};
use fid_replay::ReplayOutcome;
use fid_surface::{Envelope, ErrorCode};
use fid_types::Digest;
use std::collections::BTreeMap;

/// Command ID of the listing.
pub const LIST_DATASET: &str = "rdl.list_dataset@1";
/// Command ID of the single-record query.
pub const SHOW_DATASET: &str = "rdl.show_dataset@1";
/// CLI subcommand of `fid rdl` for the listing.
pub const CLI_LIST: &str = "list-dataset";
/// CLI subcommand of `fid rdl` for the single-record query.
pub const CLI_SHOW: &str = "show-dataset";
/// MCP tool name of the listing.
pub const MCP_LIST: &str = "rdl_list_dataset";
/// MCP tool name of the single-record query.
pub const MCP_SHOW: &str = "rdl_show_dataset";
/// Largest MCP parameter payload, in bytes.
pub const MAX_PARAMS_BYTES: usize = 4096;
/// Deepest MCP parameter nesting.
pub const MAX_PARAMS_DEPTH: usize = 4;
/// Largest activity record accepted by [`ActivityEntry::from_json`], in bytes.
pub const MAX_ENTRY_BYTES: usize = 8192;
/// Most records one store holds.
pub const MAX_RECORDS: usize = 100_000;
/// Most CLI arguments after `fid rdl`.
const MAX_CLI_ARGS: usize = 8;
const LIFECYCLE_SINCE: &str = "2026-11-19";

// ------------------------------------------------------------------ store

/// The validated records the read commands see, keyed and ordered by ID (byte order; IDs
/// are ASCII).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RdlStore {
    records: BTreeMap<String, RdlDataset>,
}

impl RdlStore {
    /// An empty store.
    pub fn new() -> RdlStore {
        RdlStore::default()
    }

    /// Adds a validated record. A second record with the same ID, or more than
    /// [`MAX_RECORDS`] records, is refused.
    pub fn insert(&mut self, rec: RdlDataset) -> Result<(), RdlError> {
        if self.records.contains_key(rec.id()) {
            return Err(invalid("id", "duplicate dataset id"));
        }
        if self.records.len() >= MAX_RECORDS {
            return Err(invalid("record", "store is full"));
        }
        self.records.insert(rec.id().to_string(), rec);
        Ok(())
    }

    /// Parses, validates and adds one record ([`RdlDataset::from_json`]).
    pub fn load_json(&mut self, bytes: &[u8]) -> Result<(), RdlError> {
        self.insert(RdlDataset::from_json(bytes)?)
    }

    /// The record with this ID.
    pub fn get(&self, id: &str) -> Option<&RdlDataset> {
        self.records.get(id)
    }

    /// Every record, ascending by ID.
    pub fn iter(&self) -> impl Iterator<Item = &RdlDataset> {
        self.records.values()
    }

    /// Number of records.
    pub fn len(&self) -> usize {
        self.records.len()
    }

    /// Whether the store is empty.
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }
}

// ------------------------------------------------------------------ parameters

/// Parameters of `rdl.list_dataset@1`: optional `tier` and `aim`; both filters must hold.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ListParams {
    pub tier: Option<Tier>,
    pub aim: Option<String>,
}

impl ListParams {
    /// Reads the parameter object; unknown keys and malformed values are `bad_params`.
    pub fn from_value(v: &CValue) -> Result<ListParams, RdlError> {
        let CValue::Obj(o) = v else { return Err(bad_params("params", "not an object")) };
        if o.keys().any(|k| k != "tier" && k != "aim") {
            return Err(bad_params("params", "unknown parameter"));
        }
        let tier = match o.get("tier") {
            None => None,
            Some(CValue::Str(s)) => Some(Tier::parse(s).ok_or_else(|| bad_params("tier", "not X, G, O or B"))?),
            Some(_) => return Err(bad_params("tier", "not a string")),
        };
        let aim = match o.get("aim") {
            None => None,
            Some(CValue::Str(s)) if aim_ok(s) => Some(s.clone()),
            Some(_) => return Err(bad_params("aim", "not module.feature.method")),
        };
        Ok(ListParams { tier, aim })
    }

    /// The canonical parameter object (absent filters are absent).
    pub fn to_value(&self) -> CValue {
        let mut m = BTreeMap::new();
        if let Some(a) = &self.aim {
            m.insert("aim".to_string(), CValue::Str(a.clone()));
        }
        if let Some(t) = self.tier {
            m.insert("tier".to_string(), CValue::Str(t.as_str().to_string()));
        }
        CValue::Obj(m)
    }
}

/// Parameters of `rdl.show_dataset@1`: `id`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShowParams {
    pub id: String,
}

impl ShowParams {
    /// Reads the parameter object; the ID must match the ID grammar.
    pub fn from_value(v: &CValue) -> Result<ShowParams, RdlError> {
        let CValue::Obj(o) = v else { return Err(bad_params("params", "not an object")) };
        if o.keys().any(|k| k != "id") {
            return Err(bad_params("params", "unknown parameter"));
        }
        match o.get("id") {
            Some(CValue::Str(s)) if parse_id(s).is_some() => Ok(ShowParams { id: s.clone() }),
            Some(_) => Err(bad_params("id", "not rdl:<tier>/<aim>/<case>/<nnnn>")),
            None => Err(bad_params("id", "required")),
        }
    }

    /// The canonical parameter object.
    pub fn to_value(&self) -> CValue {
        obj([("id", CValue::Str(self.id.clone()))])
    }
}

// ------------------------------------------------------------------ handlers

/// The public views of the matching records, ascending by ID.
pub fn list_dataset(store: &RdlStore, params: &ListParams) -> CValue {
    CValue::Arr(
        store
            .iter()
            .filter(|r| params.tier.map_or(true, |t| r.tier() == t))
            .filter(|r| params.aim.as_deref().map_or(true, |a| r.aim() == a))
            .map(public_view)
            .collect(),
    )
}

/// The public view of one record; `dataset_not_found` if there is none.
pub fn show_dataset(store: &RdlStore, params: &ShowParams) -> Result<CValue, RdlError> {
    store.get(&params.id).map(public_view).ok_or(RdlError::DatasetNotFound)
}

/// The envelope error code of an error.
fn error_code(e: &RdlError) -> ErrorCode {
    match e {
        RdlError::DatasetNotFound => ErrorCode::NotFound,
        RdlError::BadParams { .. } => ErrorCode::UsageError,
        RdlError::BadUnit { .. } => ErrorCode::UnitError,
        _ => ErrorCode::ValidationError,
    }
}

/// A `failed` envelope. The message and details hold only the error code, the field path
/// and static text (see [`RdlError`]), never record content.
pub fn failed_envelope(command: &str, e: &RdlError) -> Envelope {
    let details =
        obj([("field", CValue::Str(e.field().to_string())), ("rdl_error", CValue::Str(e.code().to_string()))]);
    Envelope::failed(command, error_code(e), &e.to_string(), Some(details))
}

/// Typed parameters of one call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Call {
    List(ListParams),
    Show(ShowParams),
}

impl Call {
    /// The command ID.
    pub fn command(&self) -> &'static str {
        match self {
            Call::List(_) => LIST_DATASET,
            Call::Show(_) => SHOW_DATASET,
        }
    }

    /// The canonical parameter object.
    pub fn params(&self) -> CValue {
        match self {
            Call::List(p) => p.to_value(),
            Call::Show(p) => p.to_value(),
        }
    }

    fn from_params(command: &str, params: &CValue) -> Result<Call, RdlError> {
        match command {
            LIST_DATASET => Ok(Call::List(ListParams::from_value(params)?)),
            SHOW_DATASET => Ok(Call::Show(ShowParams::from_value(params)?)),
            _ => Err(bad_params("command", "not an rdl read command")),
        }
    }
}

/// The one handler behind both surfaces: a `computed` or `failed` envelope.
pub fn run(store: &RdlStore, call: &Call) -> Envelope {
    let result = match call {
        Call::List(p) => Ok(list_dataset(store, p)),
        Call::Show(p) => show_dataset(store, p),
    };
    match result {
        Ok(v) => Envelope::computed(call.command(), v),
        Err(e) => failed_envelope(call.command(), &e),
    }
}

fn envelope_digest(env: &Envelope) -> Digest {
    // Encoding only fails for an envelope that breaks its own invariants; hash the error
    // marker then, so replay reports DIVERGENT instead of matching by accident.
    let bytes = env.encode().unwrap_or_else(|_| b"encode_failed".to_vec());
    Digest::from_bytes(fid_canon::digest_bytes(&bytes).0)
}

// ------------------------------------------------------------------ activity log

/// The surface a call came through.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Surface {
    Cli,
    Mcp,
}

impl Surface {
    /// Wire form.
    pub fn as_str(self) -> &'static str {
        match self {
            Surface::Cli => "cli",
            Surface::Mcp => "mcp",
        }
    }
}

/// One Activity Log record of an R0 call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActivityEntry {
    pub call: Call,
    pub surface: Surface,
    /// SHA-256 of the canonical envelope bytes the call returned.
    pub result_sha256: Digest,
}

impl ActivityEntry {
    /// The record as JSON: `{command, params, result_sha256, surface}`.
    pub fn to_value(&self) -> CValue {
        obj([
            ("command", CValue::Str(self.call.command().to_string())),
            ("params", self.call.params()),
            ("result_sha256", CValue::Str(self.result_sha256.as_str().to_string())),
            ("surface", CValue::Str(self.surface.as_str().to_string())),
        ])
    }

    /// Canonical JSON bytes of [`ActivityEntry::to_value`].
    pub fn to_json(&self) -> Vec<u8> {
        to_canonical_bytes(&self.to_value()).unwrap_or_default()
    }

    /// Parses a record written by [`ActivityEntry::to_json`]. Strict: bounded input, no
    /// duplicate or unknown keys, an rdl read command, its parameters, a valid digest.
    pub fn from_json(bytes: &[u8]) -> Result<ActivityEntry, RdlError> {
        let v = parse_with_limits(bytes, MAX_ENTRY_BYTES, MAX_PARAMS_DEPTH)
            .map_err(|_| bad_params("entry", "not acceptable JSON"))?;
        let CValue::Obj(o) = v else { return Err(bad_params("entry", "not an object")) };
        if o.keys().any(|k| !matches!(k.as_str(), "command" | "params" | "result_sha256" | "surface")) {
            return Err(bad_params("entry", "unknown field"));
        }
        let command = match o.get("command") {
            Some(CValue::Str(s)) => s.as_str(),
            _ => return Err(bad_params("command", "not a string")),
        };
        let call = Call::from_params(command, o.get("params").unwrap_or(&CValue::Null))?;
        let surface = match o.get("surface") {
            Some(CValue::Str(s)) if s == "cli" => Surface::Cli,
            Some(CValue::Str(s)) if s == "mcp" => Surface::Mcp,
            _ => return Err(bad_params("surface", "not cli or mcp")),
        };
        let result_sha256 = match o.get("result_sha256") {
            Some(CValue::Str(s)) => Digest::parse(s).map_err(|_| bad_params("result_sha256", "not a digest"))?,
            _ => return Err(bad_params("result_sha256", "not a string")),
        };
        Ok(ActivityEntry { call, surface, result_sha256 })
    }
}

/// Runs one parsed call and appends its activity record.
pub fn execute(store: &RdlStore, log: &mut Vec<ActivityEntry>, call: Call, surface: Surface) -> Envelope {
    let env = run(store, &call);
    log.push(ActivityEntry { call, surface, result_sha256: envelope_digest(&env) });
    env
}

/// Exact-mode replay of one activity record against `store`: `IDENTICAL` when the rerun
/// envelope has the recorded digest, `DIVERGENT` otherwise. D0, so no tolerance applies.
pub fn replay(entry: &ActivityEntry, store: &RdlStore) -> ReplayOutcome {
    if envelope_digest(&run(store, &entry.call)) == entry.result_sha256 {
        ReplayOutcome::Identical
    } else {
        ReplayOutcome::Divergent
    }
}

// ------------------------------------------------------------------ surfaces

/// What a CLI call prints.
#[derive(Debug, Clone, PartialEq)]
pub struct CliOutput {
    /// The final envelope.
    pub envelope: Envelope,
    /// With `--json`, the canonical envelope bytes; otherwise the canonical result bytes on
    /// success and nothing on failure.
    pub stdout: Vec<u8>,
    /// Without `--json`, one `error: <code>: <field>: <reason>` line on failure.
    pub stderr: String,
}

fn cli_output(env: Envelope, json: bool) -> CliOutput {
    if json {
        let stdout = env.encode().unwrap_or_default();
        return CliOutput { envelope: env, stdout, stderr: String::new() };
    }
    let (stdout, stderr) = match (&env.result, &env.error) {
        (Some(r), _) => (to_canonical_bytes(r).unwrap_or_default(), String::new()),
        (None, Some(e)) => (Vec::new(), format!("error: {}\n", e.message)),
        (None, None) => (Vec::new(), String::new()),
    };
    CliOutput { envelope: env, stdout, stderr }
}

fn parse_cli(args: &[&str]) -> Result<(Call, bool), (&'static str, RdlError)> {
    let usage = |e| ("rdl", e);
    if args.len() > MAX_CLI_ARGS {
        return Err(usage(bad_params("args", "too many arguments")));
    }
    let (sub, rest) = args.split_first().ok_or_else(|| usage(bad_params("args", "missing subcommand")))?;
    let command = match *sub {
        CLI_LIST => LIST_DATASET,
        CLI_SHOW => SHOW_DATASET,
        _ => return Err(usage(bad_params("args", "unknown subcommand"))),
    };
    let mut params = BTreeMap::new();
    let mut json = false;
    let mut it = rest.iter();
    while let Some(arg) = it.next() {
        let key = match (*arg, command) {
            ("--json", _) if !json => {
                json = true;
                continue;
            }
            ("--tier", LIST_DATASET) => "tier",
            ("--aim", LIST_DATASET) => "aim",
            ("--id", SHOW_DATASET) => "id",
            _ => return Err((command, bad_params("args", "unknown or repeated option"))),
        };
        let value = it.next().ok_or_else(|| (command, bad_params(key, "option needs a value")))?;
        if params.insert(key.to_string(), CValue::Str((*value).to_string())).is_some() {
            return Err((command, bad_params(key, "repeated option")));
        }
    }
    let call = Call::from_params(command, &CValue::Obj(params)).map_err(|e| (command, e))?;
    Ok((call, json))
}

/// Runs `fid rdl <args>`: `list-dataset [--tier <X|G|O|B>] [--aim <aim>] [--json]` or
/// `show-dataset --id <id> [--json]`. Unknown or repeated options are refused with
/// `usage_error` and are not logged; every parsed call is logged to `log`.
pub fn invoke_cli(store: &RdlStore, log: &mut Vec<ActivityEntry>, args: &[&str]) -> CliOutput {
    let json = args.contains(&"--json");
    match parse_cli(args) {
        Ok((call, json)) => cli_output(execute(store, log, call, Surface::Cli), json),
        Err((command, e)) => {
            let id = if command == "rdl" { "cli.invoke@1" } else { command };
            cli_output(failed_envelope(id, &e), json)
        }
    }
}

/// Runs an MCP tool call (`rdl_list_dataset` or `rdl_show_dataset`) on raw JSON parameters
/// (at most [`MAX_PARAMS_BYTES`] bytes and [`MAX_PARAMS_DEPTH`] levels; duplicate keys
/// refused) and returns the envelope. Every parsed call is logged to `log`.
pub fn invoke_mcp(store: &RdlStore, log: &mut Vec<ActivityEntry>, tool: &str, params: &[u8]) -> Envelope {
    let command = match tool {
        MCP_LIST => LIST_DATASET,
        MCP_SHOW => SHOW_DATASET,
        _ => return failed_envelope("mcp.invoke@1", &bad_params("tool", "unknown tool")),
    };
    let parsed = parse_with_limits(params, MAX_PARAMS_BYTES, MAX_PARAMS_DEPTH)
        .map_err(|_| bad_params("params", "not acceptable JSON"))
        .and_then(|v| Call::from_params(command, &v));
    match parsed {
        Ok(call) => execute(store, log, call, Surface::Mcp),
        Err(e) => failed_envelope(command, &e),
    }
}

// ------------------------------------------------------------------ registry definitions

fn text(s: &str) -> CValue {
    CValue::Str(s.to_string())
}

fn strs(items: &[&str]) -> CValue {
    CValue::Arr(items.iter().map(|s| text(s)).collect())
}

fn def(name: &str, title: &str, description: &str, params: CValue, result: CValue) -> Result<CommandDef, DefError> {
    CommandDef::from_value(obj([
        ("id", text(name)),
        ("major", CValue::Num(1.0)),
        ("title", text(title)),
        ("description", text(description)),
        ("params_schema", params),
        ("result_schema", result),
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

/// The registry definitions of `rdl.list_dataset@1` and `rdl.show_dataset@1`, in that
/// (ascending) order, built through [`CommandDef::from_value`].
pub fn command_defs() -> Result<Vec<CommandDef>, DefError> {
    let tiers: Vec<&str> = Tier::ALL.iter().map(|t| t.as_str()).collect();
    let list_params = obj([
        ("type", text("object")),
        ("additionalProperties", CValue::Bool(false)),
        ("required", CValue::Arr(Vec::new())),
        (
            "properties",
            obj([
                ("aim", obj([("type", text("string")), ("description", text("Only records with this kernel aim."))])),
                (
                    "tier",
                    obj([
                        ("type", text("string")),
                        ("enum", strs(&tiers)),
                        ("description", text("Only records of this tier.")),
                    ]),
                ),
            ]),
        ),
    ]);
    let show_params = obj([
        ("type", text("object")),
        ("additionalProperties", CValue::Bool(false)),
        ("required", strs(&["id"])),
        (
            "properties",
            obj([(
                "id",
                obj([("type", text("string")), ("description", text("Dataset ID rdl:<tier>/<aim>/<case>/<nnnn>."))]),
            )]),
        ),
    ]);
    Ok(vec![
        def(
            "rdl.list_dataset",
            "List RDL datasets",
            "Lists the public views of RDL dataset records sorted by ID, optionally filtered by tier and aim.",
            list_params,
            obj([("type", text("array")), ("items", obj([("type", text("object"))]))]),
        )?,
        def(
            "rdl.show_dataset",
            "Show RDL dataset",
            "Returns the public view of one RDL dataset record.",
            show_params,
            obj([("type", text("object"))]),
        )?,
    ])
}
