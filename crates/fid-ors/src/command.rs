//! `rdl.check_suite@1` (R1) and `rdl.list_suite@1` (R0): the suite store they read,
//! parameters, the one handler behind the CLI and MCP forms, the Activity Log record, exact
//! replay and the registry definitions (AC-1, AC-4, AC-5).
//!
//! Journaling: R0 and R1 calls by local principals go to the Activity Log, not the Record
//! Journal (REQ-CORE.JRN-016). Every call whose command and parameters parse appends one
//! [`ActivityEntry`] `{command, params, result_sha256, surface}`, failures included, where
//! `result_sha256` is SHA-256 of the canonical envelope bytes. [`replay`] reruns the entry
//! and compares that digest (`IDENTICAL` or `DIVERGENT`). Neither command changes state, so
//! no fold rebuilds anything from these records.

use crate::check::{check_suite, Finding};
use crate::suite::{bad_params, parse_handle, OrsError, Suite};
use fid_canon::{parse_with_limits, to_canonical_bytes, CValue};
use fid_gps::dmt::DmtTable;
use fid_registry::{CommandDef, DefError};
use fid_replay::ReplayOutcome;
use fid_surface::{Envelope, ErrorCode};
use fid_types::Digest;
use std::collections::BTreeMap;

/// Command ID of the checker.
pub const CHECK_SUITE: &str = "rdl.check_suite@1";
/// Command ID of the listing.
pub const LIST_SUITE: &str = "rdl.list_suite@1";
/// CLI subcommand of `fid rdl` for the checker.
pub const CLI_CHECK: &str = "check-suite";
/// CLI subcommand of `fid rdl` for the listing.
pub const CLI_LIST: &str = "list-suite";
/// MCP tool name of the checker.
pub const MCP_CHECK: &str = "rdl_check_suite";
/// MCP tool name of the listing.
pub const MCP_LIST: &str = "rdl_list_suite";
/// Largest MCP parameter payload, in bytes.
pub const MAX_PARAMS_BYTES: usize = 4096;
/// Deepest MCP parameter nesting.
pub const MAX_PARAMS_DEPTH: usize = 4;
/// Largest activity record accepted by [`ActivityEntry::from_json`], in bytes.
pub const MAX_ENTRY_BYTES: usize = 8192;
/// Most suites (installed plus staged) and DMT tables one store holds.
pub const MAX_SUITES: usize = 10_000;
/// Longest handle, in bytes.
const MAX_HANDLE: usize = 96;
const MAX_CLI_ARGS: usize = 8;
const LIFECYCLE_SINCE: &str = "2026-11-21";

fn obj<const N: usize>(pairs: [(&str, CValue); N]) -> CValue {
    CValue::Obj(pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
}

fn internal(reason: &'static str) -> OrsError {
    OrsError::Internal { reason }
}

// ------------------------------------------------------------------ store

/// The suites and DMT tables the commands see. Installed suites are released ones (listed,
/// usable as `previous`); staged suites are candidates (checked with `staged: true`).
///
/// A handle names one suite: a candidate cannot be staged under an installed handle, and a
/// suite cannot be installed under a handle staged with other content (its digest differs).
/// Installing the staged content itself (promotion) is allowed.
#[derive(Debug, Clone, Default)]
pub struct SuiteStore {
    installed: BTreeMap<String, Suite>,
    staged: BTreeMap<String, Suite>,
    dmts: BTreeMap<String, DmtTable>,
}

impl SuiteStore {
    /// An empty store.
    pub fn new() -> SuiteStore {
        SuiteStore::default()
    }

    fn full(&self) -> bool {
        self.installed.len() + self.staged.len() + self.dmts.len() >= MAX_SUITES
    }

    fn add(map: &mut BTreeMap<String, Suite>, full: bool, suite: Suite) -> Result<(), OrsError> {
        let h = suite.handle();
        if map.contains_key(&h) {
            return Err(OrsError::HandleConflict {
                field: "suite",
                reason: "a suite with this handle is already present",
            });
        }
        if full {
            return Err(crate::suite::invalid("manifest", "store is full"));
        }
        map.insert(h, suite);
        Ok(())
    }

    /// Adds a released suite. Refused (`handle_conflict`) if the handle is installed, or is
    /// staged with other content.
    pub fn install(&mut self, suite: Suite) -> Result<(), OrsError> {
        if self.staged.get(&suite.handle()).is_some_and(|s| s.digest() != suite.digest()) {
            return Err(OrsError::HandleConflict {
                field: "suite",
                reason: "a suite with other content is staged under this handle",
            });
        }
        let full = self.full();
        SuiteStore::add(&mut self.installed, full, suite)
    }

    /// Adds a candidate suite. Refused (`handle_conflict`) if the handle is already installed
    /// or already staged.
    pub fn stage(&mut self, suite: Suite) -> Result<(), OrsError> {
        if self.installed.contains_key(&suite.handle()) {
            return Err(OrsError::HandleConflict { field: "suite", reason: "a suite with this handle is installed" });
        }
        let full = self.full();
        SuiteStore::add(&mut self.staged, full, suite)
    }

    /// Withdraws a staged candidate, returning it.
    pub fn unstage(&mut self, handle: &str) -> Option<Suite> {
        self.staged.remove(handle)
    }

    /// The release the version rule compares `suite` against: the highest installed release
    /// of the same suite name with a lower version, if any (REQ-007).
    pub fn previous_of(&self, suite: &Suite) -> Option<&Suite> {
        let m = suite.manifest();
        self.installed
            .values()
            .filter(|s| s.manifest().name == m.name && s.manifest().version < m.version)
            .max_by_key(|s| s.manifest().version)
    }

    /// Adds a DMT table, found by its digest.
    pub fn add_dmt(&mut self, table: DmtTable) -> Result<(), OrsError> {
        if self.full() {
            return Err(crate::suite::invalid("dmt", "store is full"));
        }
        self.dmts.insert(table.digest().as_str().to_string(), table);
        Ok(())
    }

    /// Every installed suite, ascending by handle.
    pub fn installed(&self) -> impl Iterator<Item = &Suite> {
        self.installed.values()
    }
}

// ------------------------------------------------------------------ parameters

fn handle_param(v: Option<&CValue>, field: &'static str) -> Result<Option<String>, OrsError> {
    match v {
        None => Ok(None),
        Some(CValue::Str(s)) if s.len() <= MAX_HANDLE && parse_handle(s).is_some() => Ok(Some(s.clone())),
        Some(_) => Err(bad_params(field, "not ors-<suite>@<x.y.z>")),
    }
}

/// Parameters of `rdl.check_suite@1`: `suite` (handle), optional `previous` (handle of an
/// installed release; when given it must be the release [`SuiteStore::previous_of`]
/// resolves) and `staged` (check the staged candidate; default false).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckParams {
    pub suite: String,
    pub previous: Option<String>,
    pub staged: bool,
}

impl CheckParams {
    /// Reads the parameter object; unknown keys and malformed values are `bad_params`.
    pub fn from_value(v: &CValue) -> Result<CheckParams, OrsError> {
        let CValue::Obj(o) = v else { return Err(bad_params("params", "not an object")) };
        if o.keys().any(|k| !matches!(k.as_str(), "previous" | "staged" | "suite")) {
            return Err(bad_params("params", "unknown parameter"));
        }
        let suite = handle_param(o.get("suite"), "suite")?.ok_or_else(|| bad_params("suite", "required"))?;
        let previous = handle_param(o.get("previous"), "previous")?;
        let staged = match o.get("staged") {
            None => false,
            Some(CValue::Bool(b)) => *b,
            Some(_) => return Err(bad_params("staged", "not a boolean")),
        };
        Ok(CheckParams { suite, previous, staged })
    }

    /// The canonical parameter object (`staged` only when true; absent fields absent).
    pub fn to_value(&self) -> CValue {
        let mut m = BTreeMap::new();
        if let Some(p) = &self.previous {
            m.insert("previous".to_string(), CValue::Str(p.clone()));
        }
        if self.staged {
            m.insert("staged".to_string(), CValue::Bool(true));
        }
        m.insert("suite".to_string(), CValue::Str(self.suite.clone()));
        CValue::Obj(m)
    }
}

// ------------------------------------------------------------------ handlers

/// The check report of one suite (findings are data: a suite with findings is still a
/// `computed` result). The previous release is always resolved by the store
/// ([`SuiteStore::previous_of`]), so a caller cannot pick an older release to compare
/// against: a `previous` that is not installed is `suite_not_found{previous}`, and one that
/// is installed but is not the resolved release is `previous_mismatch`.
pub fn check(store: &SuiteStore, p: &CheckParams) -> Result<CValue, OrsError> {
    let map = if p.staged { &store.staged } else { &store.installed };
    let suite = map.get(&p.suite).ok_or(OrsError::SuiteNotFound { field: "suite" })?;
    let previous = store.previous_of(suite);
    if let Some(h) = &p.previous {
        if !store.installed.contains_key(h) {
            return Err(OrsError::SuiteNotFound { field: "previous" });
        }
        if previous.map(Suite::handle).as_ref() != Some(h) {
            return Err(OrsError::PreviousMismatch);
        }
    }
    let dmt = store.dmts.get(suite.manifest().dmt_digest.as_str());
    let mut report = check_suite(suite, dmt, previous)?;
    report.staged = p.staged;
    if below_every_release(store, suite) {
        report.findings.push(Finding { case_id: None, code: "version_not_bumped", field: "version".to_string() });
    }
    Ok(report.to_value())
}

/// Whether installed releases of `suite`'s name exist and every one has a higher version, so
/// no previous release resolves and the version rule could otherwise be skipped by checking
/// a candidate numbered below them (REQ-007; a manifest-level `version_not_bumped{version}`).
fn below_every_release(store: &SuiteStore, suite: &Suite) -> bool {
    let m = suite.manifest();
    let mut same = store.installed.values().filter(|s| s.manifest().name == m.name).peekable();
    same.peek().is_some() && same.all(|s| s.manifest().version > m.version)
}

/// The installed suites: `[{cases, dmt_digest, licence, name, version}]`, ascending by handle.
pub fn list(store: &SuiteStore) -> CValue {
    CValue::Arr(
        store
            .installed()
            .map(|s| {
                let m = s.manifest();
                obj([
                    ("cases", CValue::Num(m.cases.len() as f64)),
                    ("dmt_digest", CValue::Str(m.dmt_digest.as_str().to_string())),
                    ("licence", CValue::Str(m.licence.clone())),
                    ("name", CValue::Str(m.name.clone())),
                    ("version", CValue::Str(m.version.to_string())),
                ])
            })
            .collect(),
    )
}

fn error_code(e: &OrsError) -> ErrorCode {
    match e {
        OrsError::SuiteNotFound { .. } => ErrorCode::NotFound,
        OrsError::BadParams { .. } => ErrorCode::UsageError,
        OrsError::Internal { .. } => ErrorCode::InternalError,
        OrsError::SuiteInvalid { .. } => ErrorCode::ValidationError,
        OrsError::HandleConflict { .. } => ErrorCode::Conflict,
        OrsError::PreviousMismatch => ErrorCode::PreconditionFailed,
    }
}

/// A `failed` envelope holding only the error code, field path and static reason.
pub fn failed_envelope(command: &str, e: &OrsError) -> Envelope {
    let details =
        obj([("field", CValue::Str(e.field().to_string())), ("ors_error", CValue::Str(e.code().to_string()))]);
    Envelope::failed(command, error_code(e), &e.to_string(), Some(details))
}

/// Typed parameters of one call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Call {
    Check(CheckParams),
    List,
}

impl Call {
    /// The command ID.
    pub fn command(&self) -> &'static str {
        match self {
            Call::Check(_) => CHECK_SUITE,
            Call::List => LIST_SUITE,
        }
    }

    /// The canonical parameter object.
    pub fn params(&self) -> CValue {
        match self {
            Call::Check(p) => p.to_value(),
            Call::List => CValue::Obj(BTreeMap::new()),
        }
    }

    fn from_params(command: &str, params: &CValue) -> Result<Call, OrsError> {
        match command {
            CHECK_SUITE => Ok(Call::Check(CheckParams::from_value(params)?)),
            LIST_SUITE => match params {
                CValue::Obj(o) if o.is_empty() => Ok(Call::List),
                CValue::Obj(_) => Err(bad_params("params", "unknown parameter")),
                _ => Err(bad_params("params", "not an object")),
            },
            _ => Err(bad_params("command", "not an ORS suite command")),
        }
    }
}

/// The one handler behind both surfaces.
pub fn run(store: &SuiteStore, call: &Call) -> Envelope {
    let result = match call {
        Call::Check(p) => check(store, p),
        Call::List => Ok(list(store)),
    };
    match result {
        Ok(v) => Envelope::computed(call.command(), v),
        Err(e) => failed_envelope(call.command(), &e),
    }
}

fn encode(env: &Envelope) -> Result<Vec<u8>, OrsError> {
    env.encode().map_err(|_| internal("envelope encoding"))
}

fn envelope_digest(env: &Envelope) -> Result<Digest, OrsError> {
    Ok(Digest::from_bytes(fid_canon::digest_bytes(&encode(env)?).0))
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

/// One Activity Log record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActivityEntry {
    pub call: Call,
    pub surface: Surface,
    /// SHA-256 of the canonical envelope bytes the call returned.
    pub result_sha256: Digest,
}

impl ActivityEntry {
    /// `{command, params, result_sha256, surface}`.
    pub fn to_value(&self) -> CValue {
        obj([
            ("command", CValue::Str(self.call.command().to_string())),
            ("params", self.call.params()),
            ("result_sha256", CValue::Str(self.result_sha256.as_str().to_string())),
            ("surface", CValue::Str(self.surface.as_str().to_string())),
        ])
    }

    /// Canonical JSON bytes of [`ActivityEntry::to_value`].
    pub fn to_json(&self) -> Result<Vec<u8>, OrsError> {
        to_canonical_bytes(&self.to_value()).map_err(|_| internal("canonical encoding of the activity record"))
    }

    /// Parses a record written by [`ActivityEntry::to_json`] (bounded, strict).
    pub fn from_json(bytes: &[u8]) -> Result<ActivityEntry, OrsError> {
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

/// Runs one parsed call and appends its activity record. If the envelope cannot be encoded,
/// nothing is logged and an `internal_error` envelope is returned.
pub fn execute(store: &SuiteStore, log: &mut Vec<ActivityEntry>, call: Call, surface: Surface) -> Envelope {
    let env = run(store, &call);
    match envelope_digest(&env) {
        Ok(result_sha256) => {
            log.push(ActivityEntry { call, surface, result_sha256 });
            env
        }
        Err(e) => failed_envelope(call.command(), &e),
    }
}

/// Exact-mode replay of one activity record against `store`.
pub fn replay(entry: &ActivityEntry, store: &SuiteStore) -> Result<ReplayOutcome, OrsError> {
    Ok(if envelope_digest(&run(store, &entry.call))? == entry.result_sha256 {
        ReplayOutcome::Identical
    } else {
        ReplayOutcome::Divergent
    })
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
    let printed = if json {
        encode(&env)
    } else {
        match &env.result {
            Some(r) => to_canonical_bytes(r).map_err(|_| internal("canonical encoding of the result")),
            None => Ok(Vec::new()),
        }
    };
    match printed {
        Ok(stdout) => {
            let stderr = match (&env.error, json) {
                (Some(e), false) => format!("error: {}\n", e.message),
                _ => String::new(),
            };
            CliOutput { envelope: env, stdout, stderr }
        }
        Err(e) => {
            let env = failed_envelope(&env.command_id, &e);
            let stdout = if json { encode(&env).unwrap_or_default() } else { Vec::new() };
            CliOutput { envelope: env, stdout, stderr: format!("error: {e}\n") }
        }
    }
}

fn parse_cli(args: &[&str]) -> Result<(Call, bool), (&'static str, OrsError)> {
    if args.len() > MAX_CLI_ARGS {
        return Err(("rdl", bad_params("args", "too many arguments")));
    }
    let (sub, rest) = args.split_first().ok_or(("rdl", bad_params("args", "missing subcommand")))?;
    let command = match *sub {
        CLI_CHECK => CHECK_SUITE,
        CLI_LIST => LIST_SUITE,
        _ => return Err(("rdl", bad_params("args", "unknown subcommand"))),
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
            ("--staged", CHECK_SUITE) if !params.contains_key("staged") => {
                params.insert("staged".to_string(), CValue::Bool(true));
                continue;
            }
            ("--suite", CHECK_SUITE) => "suite",
            ("--previous", CHECK_SUITE) => "previous",
            _ => return Err((command, bad_params("args", "unknown or repeated option"))),
        };
        let value = it.next().ok_or((command, bad_params(key, "option needs a value")))?;
        if params.insert(key.to_string(), CValue::Str((*value).to_string())).is_some() {
            return Err((command, bad_params(key, "repeated option")));
        }
    }
    let call = Call::from_params(command, &CValue::Obj(params)).map_err(|e| (command, e))?;
    Ok((call, json))
}

/// Runs `fid rdl <args>`: `check-suite --suite <handle> [--previous <handle>] [--staged]
/// [--json]` or `list-suite [--json]`. Unknown or repeated options are `usage_error` and are
/// not logged; every parsed call is logged to `log`.
pub fn invoke_cli(store: &SuiteStore, log: &mut Vec<ActivityEntry>, args: &[&str]) -> CliOutput {
    let json = args.contains(&"--json");
    match parse_cli(args) {
        Ok((call, json)) => cli_output(execute(store, log, call, Surface::Cli), json),
        Err((command, e)) => {
            let id = if command == "rdl" { "cli.invoke@1" } else { command };
            cli_output(failed_envelope(id, &e), json)
        }
    }
}

/// Runs an MCP tool call (`rdl_check_suite` or `rdl_list_suite`) on raw JSON parameters.
pub fn invoke_mcp(store: &SuiteStore, log: &mut Vec<ActivityEntry>, tool: &str, params: &[u8]) -> Envelope {
    let command = match tool {
        MCP_CHECK => CHECK_SUITE,
        MCP_LIST => LIST_SUITE,
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

fn def(
    name: &str,
    title: &str,
    description: &str,
    side_effect: &str,
    params: CValue,
    result: CValue,
) -> Result<CommandDef, DefError> {
    CommandDef::from_value(obj([
        ("id", text(name)),
        ("major", CValue::Num(1.0)),
        ("title", text(title)),
        ("description", text(description)),
        ("params_schema", params),
        ("result_schema", result),
        ("side_effect", text(side_effect)),
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

fn prop(ty: &str, description: &str) -> CValue {
    obj([("type", text(ty)), ("description", text(description))])
}

/// The registry definitions of `rdl.check_suite@1` (R1) and `rdl.list_suite@1` (R0), in that
/// (ascending) order.
pub fn command_defs() -> Result<Vec<CommandDef>, DefError> {
    let check_params = obj([
        ("type", text("object")),
        ("additionalProperties", CValue::Bool(false)),
        ("required", strs(&["suite"])),
        (
            "properties",
            obj([
                (
                    "previous",
                    prop(
                        "string",
                        "Optional check: the handle of the highest installed release of this suite below its \
                         version, ors-<suite>@<x.y.z>. That release is always used; any other value is refused.",
                    ),
                ),
                (
                    "staged",
                    prop("boolean", "Check the staged candidate with this handle instead of the installed suite."),
                ),
                ("suite", prop("string", "Suite handle ors-<suite>@<x.y.z>.")),
            ]),
        ),
    ]);
    let finding = obj([
        ("type", text("object")),
        ("additionalProperties", CValue::Bool(false)),
        ("required", strs(&["case", "code", "field"])),
        (
            "properties",
            obj([
                ("case", obj([("type", strs(&["string", "null"]))])),
                ("code", obj([("type", text("string")), ("enum", strs(&crate::check::CODES))])),
                ("field", obj([("type", text("string"))])),
            ]),
        ),
    ]);
    let count = || obj([("type", text("integer")), ("minimum", CValue::Num(0.0))]);
    let check_result = obj([
        ("type", text("object")),
        ("additionalProperties", CValue::Bool(false)),
        ("required", strs(&["cases", "coverage", "findings", "passed", "previous", "staged", "suite", "suite_digest"])),
        (
            "properties",
            obj([
                ("cases", count()),
                (
                    "coverage",
                    obj([
                        ("type", text("object")),
                        ("additionalProperties", CValue::Bool(false)),
                        ("required", strs(&["closed_form_math", "dmt_row", "neither"])),
                        (
                            "properties",
                            obj([("closed_form_math", count()), ("dmt_row", count()), ("neither", count())]),
                        ),
                    ]),
                ),
                ("findings", obj([("type", text("array")), ("items", finding)])),
                ("passed", obj([("type", text("boolean"))])),
                ("previous", obj([("type", strs(&["string", "null"]))])),
                ("staged", obj([("type", text("boolean"))])),
                ("suite", obj([("type", text("string"))])),
                ("suite_digest", obj([("type", text("string")), ("pattern", text(Digest::PATTERN))])),
            ]),
        ),
    ]);
    let summary = obj([
        ("type", text("object")),
        ("additionalProperties", CValue::Bool(false)),
        ("required", strs(&["cases", "dmt_digest", "licence", "name", "version"])),
        (
            "properties",
            obj([
                ("cases", count()),
                ("dmt_digest", obj([("type", text("string"))])),
                ("licence", obj([("type", text("string"))])),
                ("name", obj([("type", text("string"))])),
                ("version", obj([("type", text("string"))])),
            ]),
        ),
    ]);
    Ok(vec![
        def(
            "rdl.check_suite",
            "Check ORS suite",
            "Checks an Open Reference Suite for citations, DMT rows, completeness, text length, \
             declared review, open licence and version bumps; findings are returned as data.",
            "R1",
            check_params,
            check_result,
        )?,
        def(
            "rdl.list_suite",
            "List ORS suites",
            "Lists the installed Open Reference Suites with version, case count, licence and pinned DMT digest.",
            "R0",
            obj([
                ("type", text("object")),
                ("additionalProperties", CValue::Bool(false)),
                ("properties", CValue::Obj(BTreeMap::new())),
            ]),
            obj([("type", text("array")), ("items", summary)]),
        )?,
    ])
}
