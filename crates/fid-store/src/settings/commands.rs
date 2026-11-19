//! The four settings commands, declared with the `fid-registry` macros
//! (REQ-CORE.STO.SET-004 to -010).
//!
//! Handlers take a [`SettingsCtx`] (registry, caller, stores, journal seam, Activity Log) and
//! typed parameters, and return typed results; `#[fid_command]` builds each command's
//! parameter and result schemas from those very types. A change runs these checks in order and
//! returns the first failure before the journal is touched, so a refused command changes
//! nothing:
//!
//! 1. the key is declared (`unknown_key`);
//! 2. the admin flag: a W2 command on an admin key is `admin_setting` whatever the principal;
//!    the A0 command on an ordinary key is `validation`;
//! 3. surface and principal: the A0 command only for a `human` principal and only from the CLI
//!    or the GUI (never MCP, Python or the control channel);
//!    an agent (an `agent` principal, or any MCP caller) may change only project values, and
//!    only on its agent branch `agent/<client>/<n>` (`policy`);
//! 4. the scope is allowed for the key (`scope_not_allowed`);
//! 5. the value passes the key's schema (`validation`);
//! 6. the scope's store is writable: a project is open and writable, or the settings file can
//!    be replaced (`scope_not_writable`).
//!
//! Then one [`ChangeEntry`] is committed through [`SettingsJournal`] to the project's Record
//! Journal (project scope, on the caller's branch) or the per-user settings journal (user and
//! machine scope). For project scope the journal stages the `settings_current` row in its own
//! transaction ([`super::stage_project_value`]); for file scopes the file is written after the
//! commit.

use super::check::{validate_value, wire_form};
use super::decl::SettingsRegistry;
use super::files::{remove_created, value_bytes, FileStore};
use super::resolve::{lookup, resolve, Loads, Lookup, Sources};
use super::table::ProjectValues;
use super::{is_valid_key, ActivityLog, Scope, SettingsError, Source, KEY_PATTERN, MAX_KEY_BYTES};
use fid_canon::{parse_with_limits, CValue};
use fid_registry::{fid_command, fid_commands, FidSchema, Surface};
use std::collections::BTreeMap;

/// `settings.list_values@1` (R0).
pub const LIST_VALUES: &str = "settings.list_values@1";
/// `settings.set_value@1` (W2). Also its journal event type.
pub const SET_VALUE: &str = "settings.set_value@1";
/// `settings.reset_value@1` (W2). Also its journal event type.
pub const RESET_VALUE: &str = "settings.reset_value@1";
/// `settings.set_admin_value@1` (A0, never on MCP). Also its journal event type.
pub const SET_ADMIN_VALUE: &str = "settings.set_admin_value@1";

/// Largest parameter document [`SetValueParams::from_json`] and friends read.
pub const MAX_PARAMS_BYTES: usize = super::MAX_VALUE_BYTES + 1024;
const MAX_PARAMS_DEPTH: usize = 64;
/// Longest client segment of an agent branch.
const MAX_CLIENT_BYTES: usize = 32;

// ------------------------------------------------------------------ caller and context

/// The kind of principal a call runs as (derived by the bus from the channel, never from
/// parameters). Device, service and system principals never change settings and are not
/// represented.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrincipalKind {
    Human,
    Agent,
    Script,
}

impl PrincipalKind {
    /// The wire string (as in the journal's `principal.kind`).
    pub fn as_str(self) -> &'static str {
        match self {
            PrincipalKind::Human => "human",
            PrincipalKind::Agent => "agent",
            PrincipalKind::Script => "script",
        }
    }

    fn parse(s: &str) -> Option<PrincipalKind> {
        [PrincipalKind::Human, PrincipalKind::Agent, PrincipalKind::Script].into_iter().find(|k| k.as_str() == s)
    }
}

/// Who calls, and through which surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Caller {
    pub kind: PrincipalKind,
    pub surface: Surface,
}

impl Caller {
    /// Whether the agent rules apply: an agent principal, or anything arriving over MCP.
    pub fn is_agent(&self) -> bool {
        self.kind == PrincipalKind::Agent || self.surface == Surface::Mcp
    }
}

/// The open project: its values on the branch the call reads, the branch a change commits to
/// (the session's working branch, or the agent branch the bus routed an agent to) and whether
/// this process holds it for writing.
pub struct ProjectScope<'a> {
    pub values: &'a dyn ProjectValues,
    pub branch: String,
    pub writable: bool,
}

/// Where a change is journaled (REQ-008).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JournalTarget {
    /// The project's Record Journal, on this branch.
    Project { branch: String },
    /// The per-user settings journal (same entry format and hash chain).
    UserSettings,
}

/// The journal seam the host wires (fid-journal's `Append` behind the bus). `Ok` means the
/// entry is durable; `Err` means nothing was committed (display-safe reason).
pub trait SettingsJournal {
    fn commit(&self, target: &JournalTarget, entry: &ChangeEntry) -> Result<(), String>;
}

/// The context every settings handler gets.
pub struct SettingsCtx<'a> {
    pub registry: &'a SettingsRegistry,
    pub caller: Caller,
    /// `None` outside any project.
    pub project: Option<ProjectScope<'a>>,
    pub user: &'a FileStore,
    pub machine: &'a FileStore,
    pub journal: &'a dyn SettingsJournal,
    pub log: &'a dyn ActivityLog,
}

impl SettingsCtx<'_> {
    fn sources(&self) -> Sources<'_> {
        Sources { project: self.project.as_ref().map(|p| p.values), user: self.user, machine: self.machine }
    }

    fn file(&self, scope: Scope) -> Option<&FileStore> {
        match scope {
            Scope::Project => None,
            Scope::User => Some(self.user),
            Scope::Machine => Some(self.machine),
        }
    }
}

// ------------------------------------------------------------------ parameter and result types

/// A setting value: any JSON value, checked against the key's own declared schema.
#[derive(Debug, Clone, PartialEq)]
pub struct SettingValue(pub CValue);

/// The registry schema of a setting value: any JSON value. The key's own schema
/// (`settings-manifest.json`) is the real constraint; the handler checks it.
impl FidSchema for SettingValue {
    fn schema() -> CValue {
        let mut o = BTreeMap::new();
        o.insert(
            "description".to_string(),
            CValue::Str("A JSON value; checked against the declared schema of the key.".to_string()),
        );
        CValue::Obj(o)
    }
}

/// Parameters of `settings.list_values@1`.
#[derive(FidSchema, Debug, Clone, PartialEq, Eq, Default)]
pub struct ListValuesParams {
    /// Only keys starting with this text.
    #[fid(description = "Only keys starting with this text.", max_length = MAX_KEY_BYTES)]
    pub prefix: Option<String>,
    /// Only keys that allow this scope.
    #[fid(description = "Only keys that allow this scope.")]
    pub scope: Option<Scope>,
}

/// One row of `settings.list_values@1`.
#[derive(FidSchema, Debug, Clone, PartialEq)]
pub struct ListedValue {
    #[fid(pattern = KEY_PATTERN, max_length = MAX_KEY_BYTES)]
    pub key: String,
    /// The effective value.
    pub value: SettingValue,
    pub source_scope: Source,
    pub default: SettingValue,
    #[fid(min_items = 1, max_items = 3)]
    pub allowed_scopes: Vec<Scope>,
    pub admin: bool,
}

/// Parameters of `settings.set_value@1` and `settings.set_admin_value@1`.
#[derive(FidSchema, Debug, Clone, PartialEq)]
pub struct SetValueParams {
    #[fid(pattern = KEY_PATTERN, max_length = MAX_KEY_BYTES)]
    pub key: String,
    pub scope: Scope,
    pub value: SettingValue,
}

/// Result of `settings.set_value@1` and `settings.set_admin_value@1`.
#[derive(FidSchema, Debug, Clone, PartialEq)]
pub struct SetValueResult {
    #[fid(pattern = KEY_PATTERN, max_length = MAX_KEY_BYTES)]
    pub key: String,
    pub scope: Scope,
    pub value: SettingValue,
    /// The valid value stored at this scope before; absent if none.
    pub previous: Option<SettingValue>,
}

/// Parameters of `settings.reset_value@1`.
#[derive(FidSchema, Debug, Clone, PartialEq, Eq)]
pub struct ResetValueParams {
    #[fid(pattern = KEY_PATTERN, max_length = MAX_KEY_BYTES)]
    pub key: String,
    pub scope: Scope,
}

/// Result of `settings.reset_value@1`.
#[derive(FidSchema, Debug, Clone, PartialEq)]
pub struct ResetValueResult {
    #[fid(pattern = KEY_PATTERN, max_length = MAX_KEY_BYTES)]
    pub key: String,
    pub scope: Scope,
    /// The valid value stored at this scope before; absent if none.
    pub previous: Option<SettingValue>,
}

fn obj(pairs: Vec<(&str, CValue)>) -> CValue {
    CValue::Obj(pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
}

fn text(s: &str) -> CValue {
    CValue::Str(s.to_string())
}

impl ListedValue {
    pub fn to_value(&self) -> CValue {
        obj(vec![
            ("admin", CValue::Bool(self.admin)),
            ("allowed_scopes", CValue::Arr(self.allowed_scopes.iter().map(|s| text(s.as_str())).collect())),
            ("default", self.default.0.clone()),
            ("key", text(&self.key)),
            ("source_scope", text(self.source_scope.as_str())),
            ("value", self.value.0.clone()),
        ])
    }
}

/// The JSON result of `settings.list_values@1`.
pub fn list_to_value(rows: &[ListedValue]) -> CValue {
    CValue::Arr(rows.iter().map(ListedValue::to_value).collect())
}

impl SetValueResult {
    pub fn to_value(&self) -> CValue {
        let mut pairs =
            vec![("key", text(&self.key)), ("scope", text(self.scope.as_str())), ("value", self.value.0.clone())];
        if let Some(p) = &self.previous {
            pairs.push(("previous", p.0.clone()));
        }
        obj(pairs)
    }
}

impl ResetValueResult {
    pub fn to_value(&self) -> CValue {
        let mut pairs = vec![("key", text(&self.key)), ("scope", text(self.scope.as_str()))];
        if let Some(p) = &self.previous {
            pairs.push(("previous", p.0.clone()));
        }
        obj(pairs)
    }
}

// ------------------------------------------------------------------ parameter decoding

fn invalid(m: &str) -> SettingsError {
    SettingsError::Validation(m.to_string())
}

/// The members of a parameter object, refusing any name outside `allowed`.
fn members(v: &CValue, allowed: &[&str]) -> Result<BTreeMap<String, CValue>, SettingsError> {
    let CValue::Obj(o) = v else { return Err(invalid("parameters are an object")) };
    if let Some(k) = o.keys().find(|k| !allowed.contains(&k.as_str())) {
        let shown = if is_valid_key(k) || (k.len() <= 32 && k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')) {
            k.as_str()
        } else {
            "?"
        };
        return Err(SettingsError::Validation(format!("unknown parameter {shown:?}")));
    }
    Ok(o.clone())
}

fn key_param(o: &BTreeMap<String, CValue>) -> Result<String, SettingsError> {
    match o.get("key") {
        Some(CValue::Str(k)) if is_valid_key(k) => Ok(k.clone()),
        Some(CValue::Str(_)) => Err(invalid("key does not match the key grammar")),
        _ => Err(invalid("key is a required string")),
    }
}

fn scope_param(o: &BTreeMap<String, CValue>) -> Result<Scope, SettingsError> {
    match o.get("scope") {
        Some(CValue::Str(s)) => Scope::parse(s).ok_or_else(|| invalid("scope is project, user or machine")),
        _ => Err(invalid("scope is a required string")),
    }
}

fn parse_params(bytes: &[u8]) -> Result<CValue, SettingsError> {
    parse_with_limits(bytes, MAX_PARAMS_BYTES, MAX_PARAMS_DEPTH)
        .map_err(|e| SettingsError::Validation(format!("parameters are not JSON ({})", e.code())))
}

impl ListValuesParams {
    pub fn from_value(v: &CValue) -> Result<ListValuesParams, SettingsError> {
        let o = members(v, &["prefix", "scope"])?;
        let prefix = match o.get("prefix") {
            None => None,
            Some(CValue::Str(p)) => Some(p.clone()),
            Some(_) => return Err(invalid("prefix is a string")),
        };
        let scope = match o.get("scope") {
            None => None,
            Some(_) => Some(scope_param(&o)?),
        };
        Ok(ListValuesParams { prefix, scope })
    }

    pub fn from_json(bytes: &[u8]) -> Result<ListValuesParams, SettingsError> {
        ListValuesParams::from_value(&parse_params(bytes)?)
    }
}

impl SetValueParams {
    pub fn from_value(v: &CValue) -> Result<SetValueParams, SettingsError> {
        let o = members(v, &["key", "scope", "value"])?;
        let value = o.get("value").cloned().ok_or_else(|| invalid("value is required"))?;
        Ok(SetValueParams { key: key_param(&o)?, scope: scope_param(&o)?, value: SettingValue(value) })
    }

    pub fn from_json(bytes: &[u8]) -> Result<SetValueParams, SettingsError> {
        SetValueParams::from_value(&parse_params(bytes)?)
    }
}

impl ResetValueParams {
    pub fn from_value(v: &CValue) -> Result<ResetValueParams, SettingsError> {
        let o = members(v, &["key", "scope"])?;
        Ok(ResetValueParams { key: key_param(&o)?, scope: scope_param(&o)? })
    }

    pub fn from_json(bytes: &[u8]) -> Result<ResetValueParams, SettingsError> {
        ResetValueParams::from_value(&parse_params(bytes)?)
    }
}

// ------------------------------------------------------------------ journal entry

/// One settings change, as committed to a journal. Built only by the commands, so nothing else
/// can produce what [`super::stage_project_value`] writes. Its JSON form
/// ([`ChangeEntry::to_value`]) is the entry's arguments and result:
/// `{branch?, command, key, previous?, principal, scope, value?}` (`value` absent for a reset).
#[derive(Debug, Clone, PartialEq)]
pub struct ChangeEntry {
    command: &'static str,
    key: String,
    scope: Scope,
    value: Option<CValue>,
    previous: Option<CValue>,
    principal: PrincipalKind,
    branch: Option<String>,
}

impl ChangeEntry {
    /// The command ID, e.g. `settings.set_value@1` (also the journal event type).
    pub fn command(&self) -> &'static str {
        self.command
    }

    pub fn key(&self) -> &str {
        &self.key
    }

    pub fn scope(&self) -> Scope {
        self.scope
    }

    /// The new value; `None` for a reset.
    pub fn value(&self) -> Option<&CValue> {
        self.value.as_ref()
    }

    pub fn previous(&self) -> Option<&CValue> {
        self.previous.as_ref()
    }

    pub fn principal(&self) -> PrincipalKind {
        self.principal
    }

    /// The project branch, for a project-scope change.
    pub fn branch(&self) -> Option<&str> {
        self.branch.as_deref()
    }

    pub fn to_value(&self) -> CValue {
        let mut pairs = vec![
            ("command", text(self.command)),
            ("key", text(&self.key)),
            ("principal", text(self.principal.as_str())),
            ("scope", text(self.scope.as_str())),
        ];
        if let Some(b) = &self.branch {
            pairs.push(("branch", text(b)));
        }
        if let Some(v) = &self.value {
            pairs.push(("value", v.clone()));
        }
        if let Some(p) = &self.previous {
            pairs.push(("previous", p.clone()));
        }
        obj(pairs)
    }
}

/// A recorded settings command, decoded from a journal entry for replay: re-running it
/// through [`invoke`] against the state before it must give the same entry.
#[derive(Debug, Clone, PartialEq)]
pub struct Invocation {
    pub command: &'static str,
    pub principal: PrincipalKind,
    pub branch: Option<String>,
    pub key: String,
    pub scope: Scope,
    pub value: Option<CValue>,
}

impl Invocation {
    /// Decodes [`ChangeEntry::to_value`]; `previous` is not an input and is ignored.
    pub fn from_entry(v: &CValue) -> Result<Invocation, SettingsError> {
        let o = members(v, &["branch", "command", "key", "previous", "principal", "scope", "value"])?;
        let command = match o.get("command") {
            Some(CValue::Str(c)) => [SET_VALUE, RESET_VALUE, SET_ADMIN_VALUE]
                .into_iter()
                .find(|k| k == c)
                .ok_or_else(|| invalid("command is not a settings change"))?,
            _ => return Err(invalid("command is a string")),
        };
        let principal = match o.get("principal") {
            Some(CValue::Str(p)) => PrincipalKind::parse(p).ok_or_else(|| invalid("unknown principal kind"))?,
            _ => return Err(invalid("principal is a string")),
        };
        let branch = match o.get("branch") {
            None => None,
            Some(CValue::Str(b)) => Some(b.clone()),
            Some(_) => return Err(invalid("branch is a string")),
        };
        let value = o.get("value").cloned();
        if value.is_some() == (command == RESET_VALUE) {
            return Err(invalid("a set carries a value and a reset none"));
        }
        Ok(Invocation { command, principal, branch, key: key_param(&o)?, scope: scope_param(&o)?, value })
    }
}

/// Runs a decoded [`Invocation`] with `ctx` and returns the command's JSON result.
pub fn invoke(ctx: &SettingsCtx<'_>, inv: &Invocation) -> Result<CValue, SettingsError> {
    let (key, scope) = (inv.key.clone(), inv.scope);
    match (inv.command, &inv.value) {
        (SET_VALUE, Some(v)) => {
            set_value(ctx, SetValueParams { key, scope, value: SettingValue(v.clone()) }).map(|r| r.to_value())
        }
        (SET_ADMIN_VALUE, Some(v)) => {
            set_admin_value(ctx, SetValueParams { key, scope, value: SettingValue(v.clone()) }).map(|r| r.to_value())
        }
        (RESET_VALUE, None) => reset_value(ctx, ResetValueParams { key, scope }).map(|r| r.to_value()),
        _ => Err(invalid("malformed invocation")),
    }
}

/// Replay outcome of a settings journal (CONSTITUTION §4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplayOutcome {
    Identical,
    Divergent,
}

/// `Identical` when the replayed entries equal the recorded ones byte for byte, in order.
pub fn replay_outcome(recorded: &[Vec<u8>], replayed: &[Vec<u8>]) -> ReplayOutcome {
    if recorded == replayed {
        ReplayOutcome::Identical
    } else {
        ReplayOutcome::Divergent
    }
}

// ------------------------------------------------------------------ the change pipeline

/// Whether `b` is an agent branch `agent/<client>/<n>`: `<client>` 1 to 32 of `[a-z0-9-]`,
/// `<n>` a positive integer without leading zero (CORE.BUS.ROUTE).
pub fn is_agent_branch(b: &str) -> bool {
    let mut parts = b.split('/');
    let (Some("agent"), Some(client), Some(n), None) = (parts.next(), parts.next(), parts.next(), parts.next()) else {
        return false;
    };
    let client_ok = !client.is_empty()
        && client.len() <= MAX_CLIENT_BYTES
        && client.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
    let n_ok = !n.is_empty() && n.len() <= 20 && !n.starts_with('0') && n.chars().all(|c| c.is_ascii_digit());
    client_ok && n_ok
}

/// Runs one change (see the module documentation for the order of the checks) and returns the
/// previous valid value at that scope.
fn change(
    ctx: &SettingsCtx<'_>,
    command: &'static str,
    key: &str,
    scope: Scope,
    value: Option<&CValue>,
) -> Result<(Option<CValue>, Option<CValue>), SettingsError> {
    // 1. Declared key.
    let decl = ctx.registry.get(key).ok_or_else(|| SettingsError::UnknownKey(format!("no setting {key}")))?;
    // 2. Admin flag (REQ-005, REQ-006).
    let admin_command = command == SET_ADMIN_VALUE;
    if decl.admin() && !admin_command {
        return Err(SettingsError::AdminSetting(format!(
            "{key} is an admin setting; it changes only through {SET_ADMIN_VALUE}"
        )));
    }
    if admin_command && !decl.admin() {
        return Err(invalid("not an admin setting; use settings.set_value@1"));
    }
    // 3. Surface and principal (REQ-006, REQ-009).
    let caller = ctx.caller;
    if admin_command {
        // Only the CLI and the desktop GUI (defence in depth: MCP, Python and the control
        // channel never reach the A0 command; maintainer decision on #275).
        if !matches!(caller.surface, Surface::Cli | Surface::Gui) {
            return Err(SettingsError::Policy(format!("{SET_ADMIN_VALUE} is available only from the CLI and the GUI")));
        }
        if caller.kind != PrincipalKind::Human {
            return Err(SettingsError::Policy(format!("{SET_ADMIN_VALUE} commits only for a human principal")));
        }
    } else if caller.is_agent() {
        if scope != Scope::Project {
            return Err(SettingsError::Policy(format!("agents cannot change {scope}-scope values")));
        }
        if !ctx.project.as_ref().is_some_and(|p| is_agent_branch(&p.branch)) {
            return Err(SettingsError::Policy("agents change project values only on their agent branch".into()));
        }
    }
    // 4. Allowed scope (REQ-010).
    if !decl.allows(scope) {
        return Err(SettingsError::ScopeNotAllowed(format!("{key} cannot be set in scope {scope}")));
    }
    // 5. The value against the key's schema, in its wire form: what is validated is exactly
    //    what is committed and read back.
    let value = match value {
        None => None,
        Some(v) => {
            value_bytes(v).map_err(SettingsError::Validation)?;
            let wire = wire_form(v).map_err(|e| SettingsError::Validation(e.message))?;
            validate_value(decl.schema(), &wire)
                .map_err(|e| SettingsError::Validation(format!("value fails the schema of {key} {e}")))?;
            Some(wire)
        }
    };
    // 6. Writable store (REQ-010).
    let mut created = Vec::new();
    let target = match scope {
        Scope::Project => match &ctx.project {
            None => return Err(SettingsError::ScopeNotWritable("no project is open".into())),
            Some(p) if !p.writable => {
                return Err(SettingsError::ScopeNotWritable("the project is not open for writing".into()))
            }
            Some(p) => JournalTarget::Project { branch: p.branch.clone() },
        },
        Scope::User | Scope::Machine => {
            created = ctx.file(scope).ok_or_else(|| invalid("no file store"))?.check_writable()?;
            JournalTarget::UserSettings
        }
    };
    // The previous value is what this scope's store holds. The project store holds only the
    // working branch, so a change on another branch (an agent branch) has no readable
    // previous value.
    let sources = ctx.sources();
    let off_working_branch = matches!((&target, sources.project),
        (JournalTarget::Project { branch }, Some(p)) if branch != p.working_branch());
    let previous = match lookup(decl, scope, &sources, &Loads::new(&sources)) {
        Lookup::Valid(v) if !off_working_branch => Some(v),
        Lookup::Valid(_) | Lookup::Absent | Lookup::Unusable(_) => None,
    };
    let entry = ChangeEntry {
        command,
        key: key.to_string(),
        scope,
        value: value.clone(),
        previous: previous.clone(),
        principal: caller.kind,
        branch: match &target {
            JournalTarget::Project { branch } => Some(branch.clone()),
            JournalTarget::UserSettings => None,
        },
    };
    if let Err(e) = ctx.journal.commit(&target, &entry) {
        remove_created(&created);
        return Err(SettingsError::Journal(e));
    }
    if let Some(file) = ctx.file(scope) {
        file.write_value(key, value.as_ref())?;
    }
    Ok((previous, value))
}

// ------------------------------------------------------------------ commands

#[fid_command(
    id = "settings.list_values",
    major = 1,
    title = "List setting values",
    description = "Lists every setting with its effective value, the scope that supplied it, the default, \
                   the allowed scopes and the admin flag, sorted by key.",
    side_effect = R0,
    mcp_projection = Exposed,
    determinism = D0,
    idempotency = Natural,
    fs_effect = None,
    undo = NotUndoable,
    preconditions = [],
    effects = [],
    algorithms = [],
    exposure = [Cli, Mcp, Python, Gui],
    lifecycle(state = "active", since = "2026-11-19"),
    web = Available,
)]
pub fn list_values(ctx: &SettingsCtx<'_>, params: ListValuesParams) -> Result<Vec<ListedValue>, SettingsError> {
    if let Some(p) = &params.prefix {
        if p.len() > MAX_KEY_BYTES
            || !p.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '.')
        {
            return Err(invalid("prefix is at most 128 characters of [a-z0-9_.]"));
        }
    }
    let prefix = params.prefix.unwrap_or_default();
    let (values, warnings) = resolve(ctx.registry, &ctx.sources(), |d| {
        d.key().starts_with(&prefix) && params.scope.is_none_or(|s| d.allows(s))
    });
    for w in &warnings {
        ctx.log.warn(w);
    }
    let mut rows = Vec::with_capacity(values.len());
    for r in values {
        let Some(decl) = ctx.registry.get(&r.key) else { continue };
        rows.push(ListedValue {
            key: r.key,
            value: SettingValue(r.value),
            source_scope: r.source,
            default: SettingValue(decl.default_value().clone()),
            allowed_scopes: decl.scopes().collect(),
            admin: decl.admin(),
        });
    }
    Ok(rows)
}

#[fid_command(
    id = "settings.set_value",
    major = 1,
    title = "Set a setting value",
    description = "Sets an ordinary setting in one of its allowed scopes. Admin settings are refused.",
    side_effect = W2,
    mcp_projection = Exposed,
    determinism = D0,
    idempotency = Natural,
    fs_effect = None,
    undo = HeadMove,
    preconditions = [],
    effects = [SET_VALUE],
    algorithms = [],
    exposure = [Cli, Mcp, Python, Gui],
    lifecycle(state = "active", since = "2026-11-19"),
    web = Available,
)]
pub fn set_value(ctx: &SettingsCtx<'_>, params: SetValueParams) -> Result<SetValueResult, SettingsError> {
    let (previous, value) = change(ctx, SET_VALUE, &params.key, params.scope, Some(&params.value.0))?;
    let value = SettingValue(value.unwrap_or(params.value.0));
    Ok(SetValueResult { key: params.key, scope: params.scope, value, previous: previous.map(SettingValue) })
}

#[fid_command(
    id = "settings.reset_value",
    major = 1,
    title = "Reset a setting value",
    description = "Removes an ordinary setting's value from one scope, so the next scope or the default applies.",
    side_effect = W2,
    mcp_projection = Exposed,
    determinism = D0,
    idempotency = Natural,
    fs_effect = None,
    undo = HeadMove,
    preconditions = [],
    effects = [RESET_VALUE],
    algorithms = [],
    exposure = [Cli, Mcp, Python, Gui],
    lifecycle(state = "active", since = "2026-11-19"),
    web = Available,
)]
pub fn reset_value(ctx: &SettingsCtx<'_>, params: ResetValueParams) -> Result<ResetValueResult, SettingsError> {
    let (previous, _) = change(ctx, RESET_VALUE, &params.key, params.scope, None)?;
    Ok(ResetValueResult { key: params.key, scope: params.scope, previous: previous.map(SettingValue) })
}

#[fid_command(
    id = "settings.set_admin_value",
    major = 1,
    title = "Set an admin setting value",
    description = "Sets an admin setting (machine limits, approved roots, security). Human principals only.",
    side_effect = A0,
    mcp_projection = Never,
    determinism = D0,
    idempotency = Natural,
    fs_effect = None,
    undo = NotUndoable,
    preconditions = [],
    effects = [SET_ADMIN_VALUE],
    algorithms = [],
    exposure = [Cli, Gui],
    lifecycle(state = "active", since = "2026-11-19"),
    human_only = true,
    web = Unavailable,
    web_note = "Desktop only.",
)]
pub fn set_admin_value(ctx: &SettingsCtx<'_>, params: SetValueParams) -> Result<SetValueResult, SettingsError> {
    let (previous, value) = change(ctx, SET_ADMIN_VALUE, &params.key, params.scope, Some(&params.value.0))?;
    let value = SettingValue(value.unwrap_or(params.value.0));
    Ok(SetValueResult { key: params.key, scope: params.scope, value, previous: previous.map(SettingValue) })
}

fid_commands![list_values, set_value, reset_value, set_admin_value];
