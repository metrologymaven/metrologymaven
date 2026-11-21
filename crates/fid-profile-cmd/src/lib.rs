//! Setting and showing the active governance profile on the bus. Spec node
//! `TRUST.PROFILE.SELECT` (the profile definitions are `fid_profile::defs`, which links no
//! CORE crate so that the verifier can read profile documents; that is why the commands live
//! here, as `fid-journal-cmd` does for `fid-journal`).
//!
//! # Commands ([`command_defs`])
//!
//! | Command | Class | MCP | Surfaces |
//! |---|---|---|---|
//! | `sign.set_profile@1` | A0, `human_only` | never | CLI `fid sign set-profile`, GUI |
//! | `sign.show_profile@1` | R0 | exposed (`sign_show_profile`) | CLI `fid sign show-profile`, MCP, GUI |
//!
//! # The active profile is a fold of the journal
//!
//! A successful set returns one `ProfileSet` event
//! `{document, name, new_digest, old_digest, params_commitment}`: the instantiated canonical
//! document (RFC 8785 text, exactly `fid_profile::CanonicalProfile::bytes`), its built-in name,
//! the new and the previously active digest, and the commitment of the call's parameters. The
//! bus commits it in the command's entry, so the configuration change and its journal record
//! are one atomic commit: [`ProfileState`] (configuration key [`CONFIG_KEY`]) is rebuilt by
//! folding the committed entries, and nothing else writes it.
//!
//! The fold ([`ProfileState::apply_entry`]) takes each entry's own principal, command and
//! parameters commitment ([`EntryOrigin`], from the entry header, never from an event body)
//! and accepts a `ProfileSet` event only if all of these hold, else it refuses the entry with
//! `internal/invariant_violated` and leaves the state unchanged:
//!
//! 1. at most one `ProfileSet` event in the entry;
//! 2. the entry's command is `sign.set_profile@1` and its principal is `human`;
//! 3. the body has exactly the five members, all strings;
//! 4. the body's `params_commitment` equals the entry's (an event cannot be spliced into an
//!    entry with other parameters);
//! 5. the document reads back with `CanonicalProfile::read` (canonical bytes of a document
//!    `fid-profile` could have produced), its digest is `new_digest` and its name is `name`;
//! 6. `old_digest` is the active digest (so an old event replayed later is refused) and
//!    differs from `new_digest` (an unchanged set is never journaled).
//!
//! Events of other types are ignored. `ProfileSet` is reserved in `fid_registry::RESERVED_EFFECTS`
//! to `sign.set_profile@1`, so no other command can declare it.
//!
//! # Set ([`run_set`])
//!
//! Check order, the first failure returned before any file is read or event built:
//!
//! 1. the caller is the principal the bus derived from the channel; anything but `human` is
//!    `policy/human_only` (REQ-001; the bus's class and projection checks refuse it earlier,
//!    this is defence in depth);
//! 2. the parameters: `name` one of the built-in names, `params` an optional string, nothing
//!    else (`validation/schema`);
//! 3. the tenant parameter file through [`TenantFiles`] (the bus's file policy, at most
//!    `fid_profile::defs::MAX_INPUT_BYTES` bytes);
//! 4. `fid_profile::instantiate`; its `profile.*` refusals become `validation/schema` whose
//!    detail starts with the profile code (the bus catalogue has no profile reasons);
//! 5. the active profile from [`ProfileSource`]; the same digest is `unchanged` with no event
//!    (REQ-003); otherwise the event is checked against the fold with the origin its entry
//!    will record, and returned (REQ-002).
//!
//! # Show ([`run_show`])
//!
//! `{digest, document, name, origin}`: the active document as a JSON object, its digest and
//! name, and `origin` `default` (the Lean built-in, nothing set) or `set` (REQ-004, REQ-005).
//!
//! Determinism D0: the result and the event depend only on the parameters, the file bytes and
//! the folded journal.

use fid_bus::error::{BusError, Detail, InternalReason, PolicyReason, ValidationReason};
use fid_bus::handler::{Event, EventType, Handler, HandlerCtx, HandlerError, HandlerSet, StateView, ValidParams};
use fid_bus::policy::fs::{resolve, Roots};
use fid_bus::policy::projection::{params_commitment, EntryOrigin};
use fid_bus::principal::{Principal, PrincipalKind};
use fid_canon::CValue;
use fid_profile::defs::MAX_INPUT_BYTES;
use fid_profile::{instantiate, CanonicalProfile, ProfileError, ProfileName};
use fid_registry::{CommandDef, DefError};
use fid_types::{CommandId, Digest};
use std::collections::BTreeMap;
use std::fmt;
use std::io::Read;
use std::sync::Arc;

/// `sign.set_profile` (major 1).
pub const SET_PROFILE: &str = "sign.set_profile";
/// `sign.show_profile` (major 1).
pub const SHOW_PROFILE: &str = "sign.show_profile";
/// The event a successful set commits; reserved to `sign.set_profile@1`.
pub const PROFILE_SET: &str = "ProfileSet";
/// The tenant configuration key the folded state stands for.
pub const CONFIG_KEY: &str = "trust.profile";
/// The `path_like` parameter that names the tenant parameter file.
pub const PARAMS_FILE: &str = "params";

const LIFECYCLE_SINCE: &str = "2026-11-21";
const BODY_KEYS: [&str; 5] = ["document", "name", "new_digest", "old_digest", "params_commitment"];

fn invariant(what: &str) -> BusError {
    BusError::Internal(InternalReason::InvariantViolated, Detail::new(what))
}

fn schema(what: &str) -> BusError {
    BusError::Validation(ValidationReason::Schema, Detail::new(what))
}

/// The bus error of a `fid-profile` refusal: `validation/schema`, detail starting with the
/// profile code (`profile.malformed`, `profile.param_forbidden`, `profile.param_invalid`).
pub fn profile_bus_error(e: &ProfileError) -> BusError {
    schema(&e.to_string())
}

fn set_id() -> Result<CommandId, BusError> {
    CommandId::parse(&format!("{SET_PROFILE}@1")).map_err(|_| invariant("command id"))
}

// ------------------------------------------------------------------ state

/// Where the active profile came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    /// Nothing set: the Lean built-in.
    Default,
    /// Set by a committed `sign.set_profile@1` entry.
    Set,
}

impl Origin {
    /// `default` or `set`.
    pub fn as_str(self) -> &'static str {
        match self {
            Origin::Default => "default",
            Origin::Set => "set",
        }
    }
}

/// The active profile, folded from committed entries in journal order (module docs).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ProfileState {
    active: Option<CanonicalProfile>,
}

fn str_member<'a>(o: &'a BTreeMap<String, CValue>, key: &str) -> Result<&'a str, BusError> {
    match o.get(key) {
        Some(CValue::Str(s)) => Ok(s),
        _ => Err(invariant(&format!("ProfileSet {key} is not a string"))),
    }
}

fn digest_member(o: &BTreeMap<String, CValue>, key: &str) -> Result<Digest, BusError> {
    Digest::parse(str_member(o, key)?).map_err(|_| invariant(&format!("ProfileSet {key} is not a sha256: digest")))
}

impl ProfileState {
    /// No entry folded: Lean, origin `default`.
    pub fn new() -> ProfileState {
        ProfileState::default()
    }

    /// The active profile and its origin.
    pub fn active(&self) -> Result<(CanonicalProfile, Origin), BusError> {
        match &self.active {
            Some(p) => Ok((p.clone(), Origin::Set)),
            None => CanonicalProfile::builtin(ProfileName::Lean)
                .map(|p| (p, Origin::Default))
                .map_err(|e| invariant(&format!("Lean built-in: {e}"))),
        }
    }

    /// The digest of the active profile.
    pub fn active_digest(&self) -> Result<Digest, BusError> {
        Ok(self.active()?.0.digest())
    }

    /// The value of [`CONFIG_KEY`]: the active document's canonical bytes, `None` when nothing
    /// is set (the default applies).
    pub fn config_value(&self) -> Option<&[u8]> {
        self.active.as_ref().map(CanonicalProfile::bytes)
    }

    fn check(&self, origin: &EntryOrigin, body: &CValue) -> Result<CanonicalProfile, BusError> {
        let cmd = origin.command();
        if cmd.name() != SET_PROFILE || cmd.major() != 1 {
            return Err(invariant("ProfileSet outside a sign.set_profile@1 entry"));
        }
        if origin.principal().kind != PrincipalKind::Human {
            return Err(invariant("ProfileSet in an entry of a non-human principal"));
        }
        let CValue::Obj(o) = body else { return Err(invariant("ProfileSet body is not an object")) };
        if o.len() != BODY_KEYS.len() || !BODY_KEYS.iter().all(|k| o.contains_key(*k)) {
            return Err(invariant("ProfileSet body does not have exactly its five members"));
        }
        if digest_member(o, "params_commitment")? != origin.params_commitment() {
            return Err(invariant("ProfileSet in an entry whose parameters are not the event's"));
        }
        let doc = CanonicalProfile::read(str_member(o, "document")?.as_bytes())
            .map_err(|e| invariant(&format!("ProfileSet document refused: {}", e.code())))?;
        let new = digest_member(o, "new_digest")?;
        let old = digest_member(o, "old_digest")?;
        if doc.digest() != new {
            return Err(invariant("ProfileSet new_digest is not the document's digest"));
        }
        if doc.document().name().as_str() != str_member(o, "name")? {
            return Err(invariant("ProfileSet name is not the document's name"));
        }
        if old != self.active_digest()? {
            return Err(invariant("ProfileSet old_digest is not the active profile's digest"));
        }
        if old == new {
            return Err(invariant("ProfileSet does not change the profile"));
        }
        Ok(doc)
    }

    /// Folds the events of one committed entry (module docs). `origin` is the entry's own
    /// principal, command and parameters commitment. On refusal the state is unchanged.
    pub fn apply_entry(&mut self, origin: &EntryOrigin, events: &[Event]) -> Result<(), BusError> {
        let mut mine = events.iter().filter(|e| e.type_id().as_str() == PROFILE_SET);
        let Some(event) = mine.next() else { return Ok(()) };
        if mine.next().is_some() {
            return Err(invariant("one entry holds two ProfileSet events"));
        }
        let doc = self.check(origin, event.body())?;
        self.active = Some(doc);
        Ok(())
    }
}

/// Where the handlers read the active profile. The bus wiring implements it over the journal.
///
/// Contract: [`ProfileSource::state`] is [`ProfileState::apply_entry`] over the events of every
/// committed entry of the tenant's journal, in append order, each with the [`EntryOrigin`] its
/// verified header records (`EntryOrigin::from_draft`). The wiring must check the payload's
/// events against the header's `events_commit` before folding and refuse an entry whose
/// payload does not reproduce it, as `fid_journal_cmd::BranchSource` requires.
pub trait ProfileSource: Send + Sync {
    /// The folded state.
    fn state(&self) -> Result<ProfileState, String>;
}

/// Reads the tenant parameter file a call names.
pub trait TenantFiles: Send + Sync {
    /// The bytes of the file `value` names for the caller `principal` and command `def`, at
    /// most `limit` bytes (more is `validation/input_limit`). Refusals are bus errors.
    fn read(&self, principal: &Principal, def: &CommandDef, value: &str, limit: usize) -> Result<Vec<u8>, BusError>;
}

/// [`TenantFiles`] through the bus's file policy (`CORE.BUS.POL.FS`): the value is resolved
/// with `fid_bus::policy::fs::resolve` against the approved roots (a human may pass a raw path,
/// D-012) and opened with its checked read.
#[derive(Debug, Clone, Default)]
pub struct RootFiles {
    roots: Roots,
}

impl RootFiles {
    pub fn new(roots: Roots) -> RootFiles {
        RootFiles { roots }
    }
}

impl TenantFiles for RootFiles {
    fn read(&self, principal: &Principal, def: &CommandDef, value: &str, limit: usize) -> Result<Vec<u8>, BusError> {
        let path = resolve(principal, def, PARAMS_FILE, value, &self.roots)?;
        let file = path.open_read()?;
        let mut buf = Vec::new();
        let cap = u64::try_from(limit).unwrap_or(u64::MAX).saturating_add(1);
        file.take(cap)
            .read_to_end(&mut buf)
            .map_err(|e| BusError::Internal(InternalReason::Storage, Detail::new(&format!("tenant file: {e}"))))?;
        if buf.len() > limit {
            return Err(BusError::Validation(ValidationReason::InputLimit, Detail::new("tenant file too large")));
        }
        Ok(buf)
    }
}

// ------------------------------------------------------------------ set and show

fn obj(pairs: Vec<(&str, CValue)>) -> CValue {
    CValue::Obj(pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
}

fn text(s: &str) -> CValue {
    CValue::Str(s.to_string())
}

/// The parsed parameters of `sign.set_profile@1`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetParams {
    pub name: ProfileName,
    pub params: Option<String>,
}

impl SetParams {
    /// `{name, params?}`; anything else is `validation/schema`.
    pub fn from_value(v: &CValue) -> Result<SetParams, BusError> {
        let CValue::Obj(o) = v else { return Err(schema("parameters are not an object")) };
        if o.keys().any(|k| k != "name" && k != PARAMS_FILE) {
            return Err(schema("unknown parameter"));
        }
        let name = match o.get("name") {
            Some(CValue::Str(s)) => {
                ProfileName::from_wire(s).ok_or_else(|| schema("name is not a built-in profile"))?
            }
            _ => return Err(schema("name is a required string")),
        };
        let params = match o.get(PARAMS_FILE) {
            None => None,
            Some(CValue::Str(s)) => Some(s.clone()),
            Some(_) => return Err(schema("params is a string")),
        };
        Ok(SetParams { name, params })
    }
}

/// Runs `sign.set_profile@1` for the call `ctx` (module docs): the events to commit (none or
/// one) and the result `{new_digest, old_digest, status}`.
pub fn run_set(
    def: &CommandDef,
    source: &dyn ProfileSource,
    files: &dyn TenantFiles,
    params: &CValue,
    ctx: &HandlerCtx,
) -> Result<(Vec<Event>, CValue), BusError> {
    let principal = ctx.principal();
    if principal.kind() != PrincipalKind::Human {
        return Err(BusError::Policy(
            PolicyReason::HumanOnly,
            Detail::new(&format!("a {} principal may not set the governance profile", principal.kind().as_str())),
        ));
    }
    if def.id != SET_PROFILE || def.major != 1 {
        return Err(invariant("run_set needs the sign.set_profile@1 definition"));
    }
    let p = SetParams::from_value(params)?;
    let file = match &p.params {
        Some(v) => Some(files.read(principal, def, v, MAX_INPUT_BYTES)?),
        None => None,
    };
    let new = instantiate(p.name, file.as_deref()).map_err(|e| profile_bus_error(&e))?;
    let state = source.state().map_err(|e| invariant(&format!("profile state: {e}")))?;
    let old = state.active_digest()?;
    let result = |status: &str| {
        obj(vec![
            ("new_digest", text(new.digest().as_str())),
            ("old_digest", text(old.as_str())),
            ("status", text(status)),
        ])
    };
    if new.digest() == old {
        return Ok((Vec::new(), result("unchanged")));
    }
    let document = std::str::from_utf8(new.bytes()).map_err(|_| invariant("canonical document is not UTF-8"))?;
    let commitment = params_commitment(params)?;
    let body = obj(vec![
        ("document", text(document)),
        ("name", text(p.name.as_str())),
        ("new_digest", text(new.digest().as_str())),
        ("old_digest", text(old.as_str())),
        ("params_commitment", text(commitment.as_str())),
    ]);
    let event = Event::new(EventType::new(PROFILE_SET)?, body);
    // The origin its entry will record: the fold must accept the event from it.
    let origin = EntryOrigin::new(principal.reference(), set_id()?, commitment);
    state.clone().apply_entry(&origin, std::slice::from_ref(&event))?;
    Ok((vec![event], result("changed")))
}

/// Runs `sign.show_profile@1`: `{digest, document, name, origin}` (module docs).
pub fn run_show(source: &dyn ProfileSource) -> Result<CValue, BusError> {
    let state = source.state().map_err(|e| invariant(&format!("profile state: {e}")))?;
    let (p, origin) = state.active()?;
    let document = fid_canon::parse(p.bytes()).map_err(|_| invariant("canonical document does not parse"))?;
    Ok(obj(vec![
        ("digest", text(p.digest().as_str())),
        ("document", document),
        ("name", text(p.document().name().as_str())),
        ("origin", text(origin.as_str())),
    ]))
}

/// The two commands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    Set,
    Show,
}

/// The handler of one profile command.
pub struct ProfileHandler {
    op: Op,
    def: CommandDef,
    source: Arc<dyn ProfileSource>,
    files: Arc<dyn TenantFiles>,
}

impl ProfileHandler {
    pub fn new(op: Op, def: CommandDef, source: Arc<dyn ProfileSource>, files: Arc<dyn TenantFiles>) -> ProfileHandler {
        ProfileHandler { op, def, source, files }
    }
}

impl fmt::Debug for ProfileHandler {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ProfileHandler").field("op", &self.op).finish_non_exhaustive()
    }
}

impl Handler for ProfileHandler {
    fn run(&self, _view: &dyn StateView, params: &ValidParams, ctx: &HandlerCtx) -> Result<Vec<Event>, HandlerError> {
        if ctx.cancel().is_cancelled() {
            return Err(HandlerError::Cancelled);
        }
        match self.op {
            Op::Set => Ok(run_set(&self.def, self.source.as_ref(), self.files.as_ref(), params.value(), ctx)?.0),
            Op::Show => {
                run_show(self.source.as_ref())?;
                Ok(Vec::new())
            }
        }
    }
}

/// Registers both handlers with their definitions from [`command_defs`].
pub fn register(
    set: &mut HandlerSet,
    source: &Arc<dyn ProfileSource>,
    files: &Arc<dyn TenantFiles>,
) -> Result<(), BusError> {
    let defs = command_defs().map_err(|e| invariant(&format!("profile definitions: {e}")))?;
    for (op, def) in [Op::Set, Op::Show].into_iter().zip(defs) {
        let h = ProfileHandler::new(op, def.clone(), Arc::clone(source), Arc::clone(files));
        set.register(def, Box::new(h))?;
    }
    Ok(())
}

// ------------------------------------------------------------------ definitions

fn strs(items: &[&str]) -> CValue {
    CValue::Arr(items.iter().map(|s| text(s)).collect())
}

fn object(required: &[&str], properties: CValue) -> CValue {
    obj(vec![
        ("type", text("object")),
        ("additionalProperties", CValue::Bool(false)),
        ("required", strs(required)),
        ("properties", properties),
    ])
}

fn digest_schema(description: &str) -> CValue {
    obj(vec![("type", text("string")), ("pattern", text(Digest::PATTERN)), ("description", text(description))])
}

fn name_schema() -> CValue {
    let names: Vec<&str> = ProfileName::ALL.iter().map(|n| n.as_str()).collect();
    obj(vec![
        ("type", text("string")),
        ("enum", strs(&names)),
        ("description", text("The built-in profile: lean, regulated or assistive_only.")),
    ])
}

fn def(fields: Vec<(&str, CValue)>) -> Result<CommandDef, DefError> {
    CommandDef::from_value(obj(fields))
}

/// The two definitions: `sign.set_profile@1`, then `sign.show_profile@1`.
pub fn command_defs() -> Result<Vec<CommandDef>, DefError> {
    let set_params = object(
        &["name"],
        obj(vec![
            ("name", name_schema()),
            (
                PARAMS_FILE,
                obj(vec![
                    ("type", text("string")),
                    ("path_like", CValue::Bool(true)),
                    ("x-extensions", strs(&["json"])),
                    ("description", text("Optional tenant parameter file (JSON) for the profile.")),
                ]),
            ),
        ]),
    );
    let set_result = object(
        &["new_digest", "old_digest", "status"],
        obj(vec![
            ("new_digest", digest_schema("Digest of the instantiated profile now active.")),
            ("old_digest", digest_schema("Digest of the profile active before the call.")),
            (
                "status",
                obj(vec![
                    ("type", text("string")),
                    ("enum", strs(&["changed", "unchanged"])),
                    ("description", text("unchanged: the profile already had this digest; nothing was journaled.")),
                ]),
            ),
        ]),
    );
    let show_result = object(
        &["digest", "document", "name", "origin"],
        obj(vec![
            ("digest", digest_schema("Digest of the active profile document.")),
            (
                "document",
                obj(vec![
                    ("type", text("object")),
                    ("description", text("The instantiated profile document (profile-v1 schema).")),
                ]),
            ),
            ("name", name_schema()),
            (
                "origin",
                obj(vec![
                    ("type", text("string")),
                    ("enum", strs(&["default", "set"])),
                    ("description", text("default: nothing set, the Lean built-in applies.")),
                ]),
            ),
        ]),
    );
    let lifecycle = || obj(vec![("state", text("active")), ("since", text(LIFECYCLE_SINCE))]);
    let set = def(vec![
        ("id", text(SET_PROFILE)),
        ("major", CValue::Num(1.0)),
        ("title", text("Set governance profile")),
        (
            "description",
            text(
                "Instantiates a built-in governance profile, optionally with a tenant parameter file, and makes it \
                 the tenant's active profile; journals the old and new digests. Human principals only.",
            ),
        ),
        ("params_schema", set_params),
        ("result_schema", set_result),
        ("side_effect", text("A0")),
        ("mcp_projection", text("never")),
        ("determinism", text("D0")),
        ("idempotency", text("natural")),
        ("fs_effect", text("read_root")),
        ("undo", text("not_undoable")),
        ("preconditions", CValue::Arr(Vec::new())),
        ("effects", strs(&[PROFILE_SET])),
        ("algorithms", CValue::Arr(Vec::new())),
        ("exposure", strs(&["cli", "gui"])),
        ("lifecycle", lifecycle()),
        ("human_only", CValue::Bool(true)),
        ("network", text("none")),
        ("web", text("unavailable")),
        ("web_note", text("Desktop only.")),
        ("cli_bare", CValue::Bool(false)),
    ])?;
    let show = def(vec![
        ("id", text(SHOW_PROFILE)),
        ("major", CValue::Num(1.0)),
        ("title", text("Show governance profile")),
        (
            "description",
            text("Returns the tenant's active governance profile: name, origin, instantiated document and digest."),
        ),
        (
            "params_schema",
            obj(vec![
                ("type", text("object")),
                ("additionalProperties", CValue::Bool(false)),
                ("properties", obj(Vec::new())),
            ]),
        ),
        ("result_schema", show_result),
        ("side_effect", text("R0")),
        ("mcp_projection", text("exposed")),
        ("determinism", text("D0")),
        ("idempotency", text("natural")),
        ("fs_effect", text("none")),
        ("undo", text("not_undoable")),
        ("preconditions", CValue::Arr(Vec::new())),
        ("effects", CValue::Arr(Vec::new())),
        ("algorithms", CValue::Arr(Vec::new())),
        ("exposure", strs(&["cli", "mcp", "gui"])),
        ("lifecycle", lifecycle()),
        ("human_only", CValue::Bool(false)),
        ("network", text("none")),
        ("web", text("available")),
        ("cli_bare", CValue::Bool(false)),
    ])?;
    Ok(vec![set, show])
}
