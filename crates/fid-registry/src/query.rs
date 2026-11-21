//! Registry query commands (spec node `CORE.REG.QRY`): `registry.list_commands@1`,
//! `registry.describe_command@1` and `registry.diff_manifests@1`, all R0, D0, writing only to
//! the Activity Log (REQ-007).
//!
//! Pinned rules (mirrored by the independent reference `fixtures/core/reg/qry/make_cases.py`):
//!
//! - **Audience** ([`Audience`]). [`Audience::Agent`] is any MCP channel or `agent` principal;
//!   [`invoke_mcp`] always uses it. To it a command whose MCP projection ([`mcp_projection`])
//!   is `never` does not exist: it is never listed (not even with `include_hidden`), describing
//!   it is `not_found`, and it is never suggested (D-035, CORE.BUS.POL.PROJ). A `human_only`
//!   command an agent can see is shown as `request_only`, which is exactly what the bus does
//!   with it. [`Audience::Person`] (a human or script outside MCP) sees every command.
//! - **MCP projection**: the stricter of the declared projection (`human_only` clamps it to
//!   `request_only`, D-031) and the ceiling of the class (R0 R1 I1 W1 W2 `exposed`, W3 X1 D1
//!   `request_only`, M1 S1 A0 `never`), and `never` when `mcp` is not in the exposure. This is
//!   `fid_bus::policy::projection::mcp_projection`; `fid-registry-build`'s tests check that the
//!   two agree on every workspace command.
//! - **list_commands**: summaries `{id, major, title, lifecycle, side_effect,
//!   mcp_projection}` sorted by ID bytes, then major (REQ-001). `domain` is the ID's text
//!   before the first dot; all given filters must match; `hidden` majors only with
//!   `include_hidden` (REQ-002). A lifecycle that does not parse counts as `hidden`.
//! - **describe_command**: the definition in manifest normal form plus its
//!   `definition_digest` (REQ-003). Without `major`, the highest `active` major; none is
//!   `not_found`. Unknown ID or major: `not_found` with up to five distinct IDs the audience
//!   can see, closest first by Levenshtein distance over Unicode scalar values, ties by ID bytes
//!   (REQ-004).
//! - **diff_manifests**: each side is read through a file reader the caller supplies (the bus
//!   resolves file handles, CORE.BUS.POL.FS), at most [`MAX_MANIFEST_BYTES`] bytes
//!   (`input_too_large`). It must parse (duplicate keys refused), match `manifest.schema.json`
//!   and carry a correct `definition_digest` on every definition, else `manifest_invalid`
//!   (REQ-006). The quantity-catalogue digest is not compared with the catalogue in force: a
//!   manifest of an older release may carry another one. One [`Change`] per changed (ID,
//!   major), from [`crate::compat::diff`] (REQ-005): `added` (additive), `removed` (breaking),
//!   `lifecycle` (breaking only when the new state is `removed`) or `changed` (breaking when
//!   any classified change is); `fields` lists the changed JSON pointers. For an agent each
//!   side must be a `root:` handle (defence in depth; the bus refuses raw paths first).
//! - **Untrusted input**: MCP parameters at most [`MAX_PARAMS_BYTES`] bytes and
//!   [`MAX_PARAMS_DEPTH`] levels, unknown keys and wrong types refused (`bad_params`), `domain`
//!   at most [`MAX_DOMAIN_BYTES`], `id` at most [`MAX_ID_BYTES`], file arguments at most
//!   [`MAX_FILE_ARG_BYTES`]. Error messages never echo input. All maps are B-trees, so
//!   output order never depends on hashing.

use crate::compat::{self, CommandRef, Kind};
use crate::def::{CommandDef, DefError, Projection, Surface as Exposure};
use crate::lifecycle::{Lifecycle, State};
use crate::manifest::{ActivityEntry, ManifestInForce, Surface, MAX_MANIFEST_BYTES};
use crate::validate::max_projection;
use fid_canon::{parse_with_limits, to_canonical_bytes, CValue};
use fid_types::{Digest, SideEffect};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

/// Command ID of the list command.
pub const LIST_COMMANDS: &str = "registry.list_commands@1";
/// Command ID of the describe command.
pub const DESCRIBE_COMMAND: &str = "registry.describe_command@1";
/// Command ID of the diff command.
pub const DIFF_MANIFESTS: &str = "registry.diff_manifests@1";
/// CLI forms after `fid registry`.
pub const CLI_LIST: &str = "list-commands";
pub const CLI_DESCRIBE: &str = "describe-command";
pub const CLI_DIFF: &str = "diff-manifests";
/// MCP tool names.
pub const MCP_LIST: &str = "registry_list_commands";
pub const MCP_DESCRIBE: &str = "registry_describe_command";
pub const MCP_DIFF: &str = "registry_diff_manifests";
/// Largest MCP parameter payload, in bytes.
pub const MAX_PARAMS_BYTES: usize = 8 * 1024;
/// Deepest MCP parameter nesting.
pub const MAX_PARAMS_DEPTH: usize = 4;
/// Longest `domain` filter, in bytes.
pub const MAX_DOMAIN_BYTES: usize = 64;
/// Longest `id` to describe, in bytes.
pub const MAX_ID_BYTES: usize = 256;
/// Longest file handle or path argument, in bytes.
pub const MAX_FILE_ARG_BYTES: usize = 2048;
/// Most suggestions of a `not_found`.
pub const MAX_SUGGESTIONS: usize = 5;
/// Most CLI arguments after `fid registry`.
const MAX_CLI_ARGS: usize = 12;
const HANDLE_PREFIX: &str = "root:";
const LIFECYCLE_SINCE: &str = "2026-11-21";
const DIGEST_PATTERN: &str = "^sha256:[0-9a-f]{64}$";
const MANIFEST_FIELDS: [&str; 3] = ["commands", "format", "quantity_catalogue_digest"];
/// Fields `manifest.schema.json` requires of a command beyond `command-def.schema.json`.
const NORMAL_FIELDS: [&str; 4] = ["cli_bare", "definition_digest", "human_only", "network"];

/// Why a query failed. Messages never echo input text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QueryError {
    /// No such command for this audience; up to five IDs it can see (`not_found`).
    NotFound(Vec<String>),
    /// A supplied manifest is not a valid manifest (`manifest_invalid`).
    ManifestInvalid(String),
    /// A supplied manifest is larger than [`MAX_MANIFEST_BYTES`] (`input_too_large`).
    InputTooLarge(usize),
    /// Parameters or CLI arguments are malformed (`bad_params`).
    BadParams(&'static str),
    /// An unknown CLI subcommand or MCP tool (`unknown_command`).
    UnknownCommand,
    /// The manifest in force could not be read (`internal`).
    Internal(&'static str),
}

impl QueryError {
    /// Stable snake_case code.
    pub fn code(&self) -> &'static str {
        match self {
            QueryError::NotFound(_) => "not_found",
            QueryError::ManifestInvalid(_) => "manifest_invalid",
            QueryError::InputTooLarge(_) => "input_too_large",
            QueryError::BadParams(_) => "bad_params",
            QueryError::UnknownCommand => "unknown_command",
            QueryError::Internal(_) => "internal",
        }
    }

    /// JSON form `{"code", "message"}`, plus `"suggestions"` for `not_found`.
    pub fn to_value(&self) -> CValue {
        let mut o = BTreeMap::new();
        o.insert("code".to_string(), CValue::Str(self.code().to_string()));
        o.insert("message".to_string(), CValue::Str(self.to_string()));
        if let QueryError::NotFound(s) = self {
            o.insert("suggestions".to_string(), CValue::Arr(s.iter().map(|x| CValue::Str(x.clone())).collect()));
        }
        CValue::Obj(o)
    }
}

impl fmt::Display for QueryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let code = self.code();
        match self {
            QueryError::NotFound(s) => write!(f, "{code}: no such command; {} suggestion(s)", s.len()),
            QueryError::ManifestInvalid(m) => write!(f, "{code}: {m}"),
            QueryError::InputTooLarge(n) => write!(f, "{code}: {n} bytes, at most {MAX_MANIFEST_BYTES}"),
            QueryError::BadParams(m) | QueryError::Internal(m) => write!(f, "{code}: {m}"),
            QueryError::UnknownCommand => write!(f, "{code}"),
        }
    }
}

impl std::error::Error for QueryError {}

/// Who is asking.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Audience {
    /// A human or script outside MCP: every command.
    Person,
    /// An MCP channel or an `agent` principal: only commands whose MCP projection is not `never`.
    Agent,
}

/// The projection MCP callers get for `def` (see the module docs).
pub fn mcp_projection(def: &CommandDef) -> Projection {
    let declared = def.effective_projection();
    let ceiling = max_projection(def.side_effect);
    let p = if declared < ceiling { declared } else { ceiling };
    if def.exposure.contains(&Exposure::Mcp) {
        p
    } else {
        Projection::Never
    }
}

fn sees(audience: Audience, def: &CommandDef) -> bool {
    audience == Audience::Person || mcp_projection(def) != Projection::Never
}

fn state_of(def: &CommandDef) -> State {
    Lifecycle::from_map(&def.lifecycle).map(|l| l.state).unwrap_or(State::Hidden)
}

// ------------------------------------------------------------------ the registry

struct Entry {
    def: CommandDef,
    digest: Digest,
    state: State,
}

/// The commands of the manifest in force, indexed by (ID, major).
pub struct Registry {
    entries: BTreeMap<(String, u32), Entry>,
}

impl fmt::Debug for Registry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Registry").field("commands", &self.entries.len()).finish()
    }
}

impl Registry {
    /// Reads the exported manifest in force. Its definitions and digests were checked at export,
    /// so a failure here is `internal`.
    pub fn new(m: &ManifestInForce) -> Result<Registry, QueryError> {
        let manifest = compat::Manifest::from_json(m.bytes())
            .map_err(|_| QueryError::Internal("the manifest in force is unreadable"))?;
        let mut entries = BTreeMap::new();
        for (key, def) in manifest.commands {
            let k = (key.id, key.major);
            let digest = *m.export().definitions.get(&k).ok_or(QueryError::Internal("a definition has no digest"))?;
            let state = state_of(&def);
            entries.insert(k, Entry { def, digest, state });
        }
        Ok(Registry { entries })
    }

    /// Number of commands, all audiences.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// True when the manifest holds no command.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    fn visible(&self, audience: Audience) -> impl Iterator<Item = (&(String, u32), &Entry)> {
        self.entries.iter().filter(move |(_, e)| sees(audience, &e.def))
    }
}

// ------------------------------------------------------------------ parameters

type Obj = BTreeMap<String, CValue>;

fn object<'v>(v: &'v CValue, allowed: &[&str]) -> Result<&'v Obj, QueryError> {
    let CValue::Obj(o) = v else { return Err(QueryError::BadParams("params is not an object")) };
    if o.keys().any(|k| !allowed.contains(&k.as_str())) {
        return Err(QueryError::BadParams("unknown parameter"));
    }
    Ok(o)
}

fn opt_str<'v>(o: &'v Obj, key: &str, max: usize, msg: &'static str) -> Result<Option<&'v str>, QueryError> {
    match o.get(key) {
        None => Ok(None),
        Some(CValue::Str(s)) if !s.is_empty() && s.len() <= max => Ok(Some(s.as_str())),
        Some(_) => Err(QueryError::BadParams(msg)),
    }
}

/// Parameters of `registry.list_commands@1`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ListParams {
    pub domain: Option<String>,
    pub side_effect: Option<SideEffect>,
    pub lifecycle: Option<State>,
    pub include_hidden: bool,
}

impl ListParams {
    /// Reads the parameter object; unknown keys, wrong types and bad values are `bad_params`.
    pub fn from_value(v: &CValue) -> Result<ListParams, QueryError> {
        let o = object(v, &["domain", "include_hidden", "lifecycle", "side_effect"])?;
        let domain = opt_str(o, "domain", MAX_DOMAIN_BYTES, "domain must be a string of 1 to 64 bytes")?;
        let side_effect = opt_str(o, "side_effect", 8, "side_effect is not a side-effect class")?
            .map(|s| SideEffect::parse(s).map_err(|_| QueryError::BadParams("side_effect is not a side-effect class")))
            .transpose()?;
        let lifecycle = opt_str(o, "lifecycle", 16, "lifecycle is not a lifecycle state")?
            .map(|s| State::parse(s).map_err(|_| QueryError::BadParams("lifecycle is not a lifecycle state")))
            .transpose()?;
        let include_hidden = match o.get("include_hidden") {
            None => false,
            Some(CValue::Bool(b)) => *b,
            Some(_) => return Err(QueryError::BadParams("include_hidden is not a boolean")),
        };
        Ok(ListParams { domain: domain.map(str::to_string), side_effect, lifecycle, include_hidden })
    }
}

/// Parameters of `registry.describe_command@1`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DescribeParams {
    pub id: String,
    pub major: Option<u32>,
}

impl DescribeParams {
    /// Reads the parameter object; unknown keys, wrong types and bad values are `bad_params`.
    pub fn from_value(v: &CValue) -> Result<DescribeParams, QueryError> {
        let o = object(v, &["id", "major"])?;
        let id = opt_str(o, "id", MAX_ID_BYTES, "id must be a string of 1 to 256 bytes")?
            .ok_or(QueryError::BadParams("id is required"))?;
        let major = match o.get("major") {
            None => None,
            Some(CValue::Num(n)) if n.fract() == 0.0 && (0.0..=f64::from(u32::MAX)).contains(n) => Some(*n as u32),
            Some(_) => return Err(QueryError::BadParams("major must be an integer 0..2^32")),
        };
        Ok(DescribeParams { id: id.to_string(), major })
    }
}

/// Parameters of `registry.diff_manifests@1`: the two file arguments, as given.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffParams {
    pub a: String,
    pub b: String,
}

impl DiffParams {
    /// Reads the parameter object; unknown keys, wrong types and over-long values are
    /// `bad_params`.
    pub fn from_value(v: &CValue) -> Result<DiffParams, QueryError> {
        let o = object(v, &["a", "b"])?;
        let msg = "a and b must be strings of 1 to 2048 bytes";
        let a = opt_str(o, "a", MAX_FILE_ARG_BYTES, msg)?.ok_or(QueryError::BadParams("a is required"))?;
        let b = opt_str(o, "b", MAX_FILE_ARG_BYTES, msg)?.ok_or(QueryError::BadParams("b is required"))?;
        Ok(DiffParams { a: a.to_string(), b: b.to_string() })
    }
}

// ------------------------------------------------------------------ list

fn num(n: u32) -> CValue {
    CValue::Num(f64::from(n))
}

fn text(s: &str) -> CValue {
    CValue::Str(s.to_string())
}

fn summary(e: &Entry) -> CValue {
    let mut o = BTreeMap::new();
    o.insert("id".to_string(), text(&e.def.id));
    o.insert("major".to_string(), num(e.def.major));
    o.insert("title".to_string(), text(&e.def.title));
    o.insert("lifecycle".to_string(), text(e.state.as_str()));
    o.insert("side_effect".to_string(), text(e.def.side_effect.as_str()));
    o.insert("mcp_projection".to_string(), text(mcp_projection(&e.def).as_str()));
    CValue::Obj(o)
}

fn domain_of(id: &str) -> &str {
    id.split_once('.').map_or(id, |(d, _)| d)
}

/// `registry.list_commands@1`: `{commands: [summary]}` (REQ-001, REQ-002).
pub fn list_commands(r: &Registry, audience: Audience, p: &ListParams) -> CValue {
    let commands = r
        .visible(audience)
        .filter(|(_, e)| p.include_hidden || e.state != State::Hidden)
        .filter(|(k, _)| p.domain.as_deref().is_none_or(|d| domain_of(&k.0) == d))
        .filter(|(_, e)| p.side_effect.is_none_or(|c| e.def.side_effect == c))
        .filter(|(_, e)| p.lifecycle.is_none_or(|s| e.state == s))
        .map(|(_, e)| summary(e))
        .collect();
    let mut o = BTreeMap::new();
    o.insert("commands".to_string(), CValue::Arr(commands));
    CValue::Obj(o)
}

// ------------------------------------------------------------------ describe

/// Levenshtein distance over Unicode scalar values (two rows, no allocation per cell).
pub fn levenshtein(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut cur = vec![0; b.len() + 1];
    for (i, ca) in a.chars().enumerate() {
        cur[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let sub = prev[j] + usize::from(ca != *cb);
            let del = prev[j + 1] + 1;
            let ins = cur[j] + 1;
            cur[j + 1] = sub.min(del).min(ins);
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[b.len()]
}

fn suggestions(r: &Registry, audience: Audience, id: &str) -> Vec<String> {
    let ids: BTreeSet<&str> = r.visible(audience).map(|(k, _)| k.0.as_str()).collect();
    let mut ranked: Vec<(usize, &str)> = ids.into_iter().map(|s| (levenshtein(id, s), s)).collect();
    ranked.sort();
    ranked.into_iter().take(MAX_SUGGESTIONS).map(|(_, s)| s.to_string()).collect()
}

/// `registry.describe_command@1`: `{definition, definition_digest}` (REQ-003, REQ-004).
pub fn describe_command(r: &Registry, audience: Audience, p: &DescribeParams) -> Result<CValue, QueryError> {
    let lo = (p.id.clone(), 0);
    let hi = (p.id.clone(), u32::MAX);
    let mut majors = r.entries.range(lo..=hi).filter(|(_, e)| sees(audience, &e.def));
    let hit = match p.major {
        Some(m) => majors.find(|(k, _)| k.1 == m),
        None => majors.rev().find(|(_, e)| e.state == State::Active),
    };
    let Some((_, e)) = hit else { return Err(QueryError::NotFound(suggestions(r, audience, &p.id))) };
    let mut o = BTreeMap::new();
    o.insert("definition".to_string(), e.def.to_value());
    o.insert("definition_digest".to_string(), CValue::Str(e.digest.to_string()));
    Ok(CValue::Obj(o))
}

// ------------------------------------------------------------------ diff

/// One entry of a manifest diff (REQ-005).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Change {
    pub id: String,
    pub major: u32,
    pub kind: ChangeKind,
    pub compat: Kind,
    /// Changed JSON pointers, sorted; empty for added and removed.
    pub fields: BTreeSet<String>,
}

/// What happened to one (ID, major).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ChangeKind {
    Added,
    Removed,
    Lifecycle,
    Changed,
}

impl ChangeKind {
    /// The wire string.
    pub fn as_str(self) -> &'static str {
        match self {
            ChangeKind::Added => "added",
            ChangeKind::Removed => "removed",
            ChangeKind::Lifecycle => "lifecycle",
            ChangeKind::Changed => "changed",
        }
    }
}

impl Change {
    /// `{id, major, kind, compat, fields}`.
    pub fn to_value(&self) -> CValue {
        let mut o = BTreeMap::new();
        o.insert("id".to_string(), text(&self.id));
        o.insert("major".to_string(), num(self.major));
        o.insert("kind".to_string(), text(self.kind.as_str()));
        o.insert("compat".to_string(), text(self.compat.as_str()));
        o.insert("fields".to_string(), CValue::Arr(self.fields.iter().map(|f| text(f)).collect()));
        CValue::Obj(o)
    }
}

/// Checks a supplied manifest (see the module docs) and reads it for diffing.
pub fn read_manifest(bytes: &[u8]) -> Result<compat::Manifest, QueryError> {
    let bad = |m: String| QueryError::ManifestInvalid(m);
    if bytes.len() > MAX_MANIFEST_BYTES {
        return Err(QueryError::InputTooLarge(bytes.len()));
    }
    let value = parse_with_limits(bytes, MAX_MANIFEST_BYTES, fid_canon::MAX_DEPTH)
        .map_err(|e| bad(format!("json: {}", e.code())))?;
    let CValue::Obj(top) = &value else { return Err(bad("root must be an object".into())) };
    if top.len() != MANIFEST_FIELDS.len() || !MANIFEST_FIELDS.iter().all(|k| top.contains_key(*k)) {
        return Err(bad("root must have exactly commands, format and quantity_catalogue_digest".into()));
    }
    if top.get("format") != Some(&CValue::Num(f64::from(crate::manifest::MANIFEST_FORMAT))) {
        return Err(bad("format must be 1".into()));
    }
    match top.get("quantity_catalogue_digest") {
        Some(CValue::Str(s)) if Digest::is_valid(s) => {}
        _ => return Err(bad("quantity_catalogue_digest must be a sha256 digest".into())),
    }
    let Some(CValue::Arr(items)) = top.get("commands") else { return Err(bad("commands must be an array".into())) };
    if items.len() > crate::manifest::MAX_COMMANDS {
        return Err(bad("too many commands".into()));
    }
    for (i, item) in items.iter().enumerate() {
        let CValue::Obj(def) = item else { return Err(bad(format!("commands[{i}] is not an object"))) };
        if !NORMAL_FIELDS.iter().all(|k| def.contains_key(*k)) {
            return Err(bad(format!("commands[{i}] lacks a field manifest.schema.json requires")));
        }
        match def.get("definition_digest") {
            Some(CValue::Str(s)) if Digest::is_valid(s) => {}
            _ => return Err(bad(format!("commands[{i}]: definition_digest malformed"))),
        }
    }
    // The definition rules (command-def.schema.json), every definition_digest recomputed and
    // compared, and repeated (ID, major) refused.
    compat::Manifest::from_value(value).map_err(|e| bad(e.0))
}

fn lifecycle_compat(new_state: Option<&str>) -> Kind {
    if new_state == Some(State::Removed.as_str()) {
        Kind::Breaking
    } else {
        Kind::Additive
    }
}

fn slot<'m>(acc: &'m mut BTreeMap<CommandRef, Change>, c: &CommandRef, kind: ChangeKind) -> &'m mut Change {
    acc.entry(c.clone()).or_insert_with(|| Change {
        id: c.id.clone(),
        major: c.major,
        kind,
        compat: Kind::Additive,
        fields: BTreeSet::new(),
    })
}

/// The changes from manifest `a` to manifest `b`, one per changed (ID, major), sorted by ID,
/// then major (REQ-005).
pub fn changes(a: &compat::Manifest, b: &compat::Manifest) -> Vec<Change> {
    let d = compat::diff(a, b);
    let mut acc: BTreeMap<CommandRef, Change> = BTreeMap::new();
    for c in &d.added {
        slot(&mut acc, c, ChangeKind::Added);
    }
    for c in &d.removed {
        slot(&mut acc, c, ChangeKind::Removed).compat = Kind::Breaking;
    }
    for l in &d.lifecycle_changed {
        let e = slot(&mut acc, &l.command, ChangeKind::Lifecycle);
        e.fields.insert("/lifecycle".to_string());
        e.compat = e.compat.max(lifecycle_compat(l.new_state.as_deref()));
    }
    for c in &d.changed {
        let e = slot(&mut acc, &c.command, ChangeKind::Changed);
        e.kind = ChangeKind::Changed;
        e.fields.insert(c.pointer.clone());
        e.compat = e.compat.max(c.kind);
    }
    acc.into_values().collect()
}

/// `registry.diff_manifests@1` over the bytes of two manifests: `{changes: [Change]}`.
pub fn diff_manifests(a: &[u8], b: &[u8]) -> Result<CValue, QueryError> {
    let (a, b) = (read_manifest(a)?, read_manifest(b)?);
    let mut o = BTreeMap::new();
    o.insert("changes".to_string(), CValue::Arr(changes(&a, &b).iter().map(Change::to_value).collect()));
    Ok(CValue::Obj(o))
}

// ------------------------------------------------------------------ surfaces

/// Reads a file argument (a `root:` handle the bus has resolved, or a person's path); `None`
/// when it cannot be read. The caller bounds the read; more than [`MAX_MANIFEST_BYTES`] is
/// refused here too.
pub type ReadFile<'a> = &'a dyn Fn(&str) -> Option<Vec<u8>>;

fn load(audience: Audience, arg: &str, read: ReadFile<'_>) -> Result<Vec<u8>, QueryError> {
    if audience == Audience::Agent && !arg.starts_with(HANDLE_PREFIX) {
        return Err(QueryError::BadParams("an agent must pass root: file handles"));
    }
    let bytes = read(arg).ok_or(QueryError::BadParams("a manifest file cannot be read"))?;
    if bytes.len() > MAX_MANIFEST_BYTES {
        return Err(QueryError::InputTooLarge(bytes.len()));
    }
    Ok(bytes)
}

/// Which query a call names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Query {
    List,
    Describe,
    Diff,
}

impl Query {
    fn id(self) -> &'static str {
        match self {
            Query::List => LIST_COMMANDS,
            Query::Describe => DESCRIBE_COMMAND,
            Query::Diff => DIFF_MANIFESTS,
        }
    }
}

fn run(r: &Registry, audience: Audience, q: Query, params: &CValue, read: ReadFile<'_>) -> Result<Vec<u8>, QueryError> {
    let result = match q {
        Query::List => list_commands(r, audience, &ListParams::from_value(params)?),
        Query::Describe => describe_command(r, audience, &DescribeParams::from_value(params)?)?,
        Query::Diff => {
            let p = DiffParams::from_value(params)?;
            let a = load(audience, &p.a, read)?;
            let b = load(audience, &p.b, read)?;
            diff_manifests(&a, &b)?
        }
    };
    to_canonical_bytes(&result).map_err(|_| QueryError::Internal("result is not canonical JSON"))
}

/// Runs one query on an already parsed parameter object for `audience`; the bus adapter's entry
/// point. The call is logged to `log` with `surface`.
pub fn call(
    r: &Registry,
    log: &mut Vec<ActivityEntry>,
    surface: Surface,
    audience: Audience,
    command: &str,
    params: &CValue,
    read: ReadFile<'_>,
) -> Result<Vec<u8>, QueryError> {
    let q = match command {
        LIST_COMMANDS => Query::List,
        DESCRIBE_COMMAND => Query::Describe,
        DIFF_MANIFESTS => Query::Diff,
        _ => return Err(QueryError::UnknownCommand),
    };
    log.push(ActivityEntry { command: q.id(), surface });
    run(r, audience, q, params, read)
}

/// Runs an MCP tool on raw JSON parameters (at most [`MAX_PARAMS_BYTES`] bytes,
/// [`MAX_PARAMS_DEPTH`] levels, duplicate keys refused) as [`Audience::Agent`] and returns the
/// canonical result bytes. The call is logged to `log`.
pub fn invoke_mcp(
    r: &Registry,
    log: &mut Vec<ActivityEntry>,
    tool: &str,
    params: &[u8],
    read: ReadFile<'_>,
) -> Result<Vec<u8>, QueryError> {
    let q = match tool {
        MCP_LIST => Query::List,
        MCP_DESCRIBE => Query::Describe,
        MCP_DIFF => Query::Diff,
        _ => return Err(QueryError::UnknownCommand),
    };
    log.push(ActivityEntry { command: q.id(), surface: Surface::Mcp });
    let value = parse_with_limits(params, MAX_PARAMS_BYTES, MAX_PARAMS_DEPTH)
        .map_err(|_| QueryError::BadParams("params are not valid JSON within the limits"))?;
    run(r, Audience::Agent, q, &value, read)
}

fn cli_option<'a>(it: &mut std::slice::Iter<'_, &'a str>, seen: bool) -> Result<&'a str, QueryError> {
    if seen {
        return Err(QueryError::BadParams("repeated option"));
    }
    it.next().copied().ok_or(QueryError::BadParams("an option needs a value"))
}

fn cli_list(rest: &[&str]) -> Result<CValue, QueryError> {
    let mut o = BTreeMap::new();
    let (mut json, mut hidden) = (false, false);
    let mut it = rest.iter();
    while let Some(arg) = it.next() {
        let key = match *arg {
            "--json" if !json => {
                json = true;
                continue;
            }
            "--include-hidden" if !hidden => {
                hidden = true;
                o.insert("include_hidden".to_string(), CValue::Bool(true));
                continue;
            }
            "--domain" => "domain",
            "--side-effect" => "side_effect",
            "--lifecycle" => "lifecycle",
            "--json" | "--include-hidden" => return Err(QueryError::BadParams("repeated option")),
            _ => return Err(QueryError::BadParams("unknown argument")),
        };
        let v = cli_option(&mut it, o.contains_key(key))?;
        o.insert(key.to_string(), text(v));
    }
    Ok(CValue::Obj(o))
}

fn positional<'a>(rest: &[&'a str], n: usize) -> Result<Vec<&'a str>, QueryError> {
    let mut json = false;
    let mut out = Vec::new();
    for arg in rest {
        match *arg {
            "--json" if !json => json = true,
            "--json" => return Err(QueryError::BadParams("repeated option")),
            a if a.starts_with("--") => return Err(QueryError::BadParams("unknown argument")),
            a => out.push(a),
        }
    }
    if out.len() != n {
        return Err(QueryError::BadParams("wrong number of arguments"));
    }
    Ok(out)
}

/// `<id>[@<major>]`; the major is 1 to 10 ASCII digits.
fn cli_describe(rest: &[&str]) -> Result<CValue, QueryError> {
    let args = positional(rest, 1)?;
    let arg = args.first().copied().unwrap_or_default();
    let mut o = BTreeMap::new();
    let (id, major) = match arg.rsplit_once('@') {
        Some((id, m)) => {
            if m.is_empty() || m.len() > 10 || !m.bytes().all(|b| b.is_ascii_digit()) {
                return Err(QueryError::BadParams("the major after @ must be an integer 0..2^32"));
            }
            let m: u64 =
                m.parse().map_err(|_| QueryError::BadParams("the major after @ must be an integer 0..2^32"))?;
            let m =
                u32::try_from(m).map_err(|_| QueryError::BadParams("the major after @ must be an integer 0..2^32"))?;
            (id, Some(m))
        }
        None => (arg, None),
    };
    o.insert("id".to_string(), text(id));
    if let Some(m) = major {
        o.insert("major".to_string(), num(m));
    }
    Ok(CValue::Obj(o))
}

fn cli_diff(rest: &[&str]) -> Result<CValue, QueryError> {
    let args = positional(rest, 2)?;
    let mut o = BTreeMap::new();
    for (k, v) in ["a", "b"].into_iter().zip(args) {
        o.insert(k.to_string(), text(v));
    }
    Ok(CValue::Obj(o))
}

/// Runs `fid registry list-commands [--domain D] [--side-effect C] [--lifecycle L]
/// [--include-hidden] [--json]`, `fid registry describe-command <id>[@<major>] [--json]` or
/// `fid registry diff-manifests <a> <b> [--json]` for `audience`. Output is canonical JSON
/// either way. Unknown or repeated options are refused. The call is logged to `log`.
pub fn invoke_cli(
    r: &Registry,
    log: &mut Vec<ActivityEntry>,
    audience: Audience,
    args: &[&str],
    read: ReadFile<'_>,
) -> Result<Vec<u8>, QueryError> {
    if args.len() > MAX_CLI_ARGS {
        return Err(QueryError::BadParams("too many arguments"));
    }
    let (sub, rest) = args.split_first().ok_or(QueryError::BadParams("missing subcommand"))?;
    let q = match *sub {
        CLI_LIST => Query::List,
        CLI_DESCRIBE => Query::Describe,
        CLI_DIFF => Query::Diff,
        _ => return Err(QueryError::UnknownCommand),
    };
    log.push(ActivityEntry { command: q.id(), surface: Surface::Cli });
    let params = match q {
        Query::List => cli_list(rest)?,
        Query::Describe => cli_describe(rest)?,
        Query::Diff => cli_diff(rest)?,
    };
    run(r, audience, q, &params, read)
}

// ------------------------------------------------------------------ definitions

fn obj<const N: usize>(pairs: [(&str, CValue); N]) -> CValue {
    CValue::Obj(pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
}

fn strs(items: &[&str]) -> CValue {
    CValue::Arr(items.iter().map(|s| text(s)).collect())
}

fn described(schema: CValue, description: &str) -> CValue {
    match schema {
        CValue::Obj(mut o) => {
            o.insert("description".to_string(), text(description));
            CValue::Obj(o)
        }
        other => other,
    }
}

fn string_enum(values: &[&str]) -> CValue {
    obj([("type", text("string")), ("enum", strs(values))])
}

fn string_max(max: usize) -> CValue {
    obj([("type", text("string")), ("minLength", num(1)), ("maxLength", CValue::Num(max as f64))])
}

fn closed(required: &[&str], properties: CValue) -> CValue {
    obj([
        ("type", text("object")),
        ("additionalProperties", CValue::Bool(false)),
        ("required", strs(required)),
        ("properties", properties),
    ])
}

fn classes() -> Vec<&'static str> {
    SideEffect::ALL.iter().map(|c| c.as_str()).collect()
}

fn states() -> Vec<&'static str> {
    State::ALL.iter().map(|s| s.as_str()).collect()
}

fn major_schema() -> CValue {
    obj([("type", text("integer")), ("minimum", num(0)), ("maximum", num(u32::MAX))])
}

fn def(
    id: &str,
    title: &str,
    description: &str,
    params: CValue,
    result: CValue,
    fs: &str,
) -> Result<CommandDef, DefError> {
    CommandDef::from_value(obj([
        ("id", text(id)),
        ("major", num(1)),
        ("title", text(title)),
        ("description", text(description)),
        ("params_schema", params),
        ("result_schema", result),
        ("side_effect", text("R0")),
        ("mcp_projection", text("exposed")),
        ("determinism", text("D0")),
        ("idempotency", text("natural")),
        ("fs_effect", text(fs)),
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

/// The registry definition of `registry.list_commands@1`.
pub fn list_command_def() -> Result<CommandDef, DefError> {
    let summary = closed(
        &["id", "lifecycle", "major", "mcp_projection", "side_effect", "title"],
        obj([
            ("id", obj([("type", text("string"))])),
            ("major", major_schema()),
            ("title", obj([("type", text("string"))])),
            ("lifecycle", string_enum(&states())),
            ("side_effect", string_enum(&classes())),
            ("mcp_projection", string_enum(&["never", "request_only", "exposed"])),
        ]),
    );
    let params = closed(
        &[],
        obj([
            (
                "domain",
                described(string_max(MAX_DOMAIN_BYTES), "Only commands whose ID starts with this domain and a dot."),
            ),
            ("side_effect", described(string_enum(&classes()), "Only commands of this side-effect class.")),
            ("lifecycle", described(string_enum(&states()), "Only commands in this lifecycle state.")),
            (
                "include_hidden",
                obj([
                    ("type", text("boolean")),
                    ("default", CValue::Bool(false)),
                    ("description", text("Also list majors in the hidden state.")),
                ]),
            ),
        ]),
    );
    let result = closed(&["commands"], obj([("commands", obj([("type", text("array")), ("items", summary)]))]));
    def(
        "registry.list_commands",
        "List commands",
        "Lists the commands the caller can see, sorted by ID then major, with title, lifecycle state, side-effect class and MCP projection; filters combine.",
        params,
        result,
        "none",
    )
}

/// The registry definition of `registry.describe_command@1`.
pub fn describe_command_def() -> Result<CommandDef, DefError> {
    let params = closed(
        &["id"],
        obj([
            ("id", described(string_max(MAX_ID_BYTES), "The command ID without the major.")),
            ("major", described(major_schema(), "The major; the latest active major when omitted.")),
        ]),
    );
    let result = closed(
        &["definition", "definition_digest"],
        obj([
            ("definition", obj([("type", text("object"))])),
            ("definition_digest", obj([("type", text("string")), ("pattern", text(DIGEST_PATTERN))])),
        ]),
    );
    def(
        "registry.describe_command",
        "Describe command",
        "Returns the full definition of one command and its definition digest; an unknown ID or major fails with not_found and up to five closest IDs.",
        params,
        result,
        "none",
    )
}

/// The registry definition of `registry.diff_manifests@1` (`fs_effect: read_root`; both
/// parameters are `path_like` file handles to `.json` files).
pub fn diff_command_def() -> Result<CommandDef, DefError> {
    let handle = |d: &str| {
        obj([
            ("type", text("string")),
            ("path_like", CValue::Bool(true)),
            ("x-extensions", strs(&["json"])),
            ("minLength", num(1)),
            ("maxLength", CValue::Num(MAX_FILE_ARG_BYTES as f64)),
            ("description", text(d)),
        ])
    };
    let params = closed(
        &["a", "b"],
        obj([
            ("a", handle("The older manifest, as a file handle.")),
            ("b", handle("The newer manifest, as a file handle.")),
        ]),
    );
    let change = closed(
        &["compat", "fields", "id", "kind", "major"],
        obj([
            ("id", obj([("type", text("string"))])),
            ("major", major_schema()),
            ("kind", string_enum(&["added", "removed", "lifecycle", "changed"])),
            ("compat", string_enum(&["additive", "breaking"])),
            ("fields", obj([("type", text("array")), ("items", obj([("type", text("string"))]))])),
        ]),
    );
    let result = closed(&["changes"], obj([("changes", obj([("type", text("array")), ("items", change)]))]));
    def(
        "registry.diff_manifests",
        "Diff manifests",
        "Compares two registry manifests and lists added, removed, lifecycle-changed and changed commands, each marked additive or breaking, sorted by ID then major.",
        params,
        result,
        "read_root",
    )
}

/// The three query definitions, in ID order.
pub fn command_defs() -> Result<Vec<CommandDef>, DefError> {
    Ok(vec![describe_command_def()?, diff_command_def()?, list_command_def()?])
}
