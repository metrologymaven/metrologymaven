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
//! # The active profile is a fold of `main`
//!
//! A successful set returns one `ProfileSet` event
//! `{decided_at, document, name, new_digest, old_digest, params_commitment}`: the profile
//! decision it builds on (`decided_at`: the entry hash of the last applied `ProfileSet` entry,
//! or `genesis`), the instantiated canonical document (RFC 8785 text, exactly
//! `fid_profile::CanonicalProfile::bytes`), its built-in name, the new and the previously active
//! digest, and the commitment of the call's parameters. The bus commits it in the command's
//! entry, so the configuration change and its journal record are one atomic commit:
//! [`ProfileState`] (configuration key [`CONFIG_KEY`]) is rebuilt by folding the committed
//! entries, and nothing else writes it. `trust.profile` must never be declared as a
//! `CORE.STO.SET` setting.
//!
//! The fold ([`ProfileState::apply_entry`]) takes each entry's own record ([`ProfileEntry`]:
//! the bus [`EntryOrigin`] with principal, command and parameters commitment; the journal's
//! `fid_journal::branch::EntryOrigin` with kind, command, class, branch and the stage that
//! added the events; the header's `promoted_from`; the entry hash), never an event body. Only
//! entries on `main` count: an entry on any other branch is ignored, whatever it holds. For an
//! entry on `main` that holds a `ProfileSet` event, all of these must hold, else the fold
//! refuses the entry with `internal/invariant_violated` and leaves the state unchanged:
//!
//! 1. at most one `ProfileSet` event and no `Request*` event in the entry (a profile change
//!    is a direct human action, never an approved agent request);
//! 2. the entry is not a promoted copy (`promoted_from` absent): `sign.set_profile@1` runs
//!    directly on `main`;
//! 3. the entry's command is `sign.set_profile@1`, class A0, its events added by the handler,
//!    and its principal `human` in both records;
//! 4. the body has exactly the six members, all strings;
//! 5. the body's `params_commitment` equals the entry's (an event cannot be spliced into an
//!    entry with other parameters);
//! 6. `decided_at` is the hash of the last applied `ProfileSet` entry (or `genesis`): two sets
//!    decided on the same state cannot both apply, and an old payload replayed later is
//!    refused even when its digests happen to match again;
//! 7. the document reads back with `CanonicalProfile::read`, its digest is `new_digest` and its
//!    name is `name`;
//! 8. `old_digest` is the active digest and differs from `new_digest`.
//!
//! Other event types are ignored. `ProfileSet` is reserved in `fid_registry::RESERVED_EFFECTS`
//! to `sign.set_profile@1`, so no other command can declare it.
//!
//! A committed entry the fold refuses means the commit stage let through what it must not
//! (see [`ProfileSource`]): it is fail-closed, the folded state is not trusted past it.
//!
//! # Set ([`run_set`])
//!
//! Check order, the first failure returned before any file is read or event built:
//!
//! 1. the caller is the principal the bus derived from the channel; anything but `human` is
//!    `policy/human_only` (REQ-001; the bus's class and projection checks refuse it earlier,
//!    this is defence in depth);
//! 2. the call commits to `main` ([`ProfileSource::call_branch`]), else `precondition/false`;
//! 3. the parameters: `name` one of the built-in names, `params` an optional string, nothing
//!    else (`validation/schema`);
//! 4. the tenant parameter file through [`TenantFiles`] (the bus's file policy, at most
//!    `fid_profile::defs::MAX_INPUT_BYTES` bytes);
//! 5. `fid_profile::instantiate`; its `profile.*` refusals become `validation/schema` whose
//!    detail starts with the profile code (the bus catalogue has no profile reasons);
//! 6. the active profile from [`ProfileSource`]; the same digest is `unchanged` with no event
//!    (REQ-003); otherwise the event, with `decided_at` from the state read, is checked
//!    against the fold with the record its entry will have, and returned (REQ-002).
//!
//! # Show ([`run_show`])
//!
//! `{digest, document, name, origin}`: the active document as a JSON object, its digest and
//! name, and `origin` `default` (the Lean built-in, nothing set) or `set` (REQ-004, REQ-005).
//!
//! Determinism D0: the result and the event depend only on the parameters, the file bytes and
//! the folded journal.

use fid_bus::error::ValidationReason;
use fid_bus::error::{BusError, ConflictReason, Detail, InternalReason, PolicyReason, PreconditionReason};
use fid_bus::handler::{Event, EventType, Handler, HandlerCtx, HandlerError, HandlerSet, StateView, ValidParams};
use fid_bus::policy::fs::{resolve, Roots};
use fid_bus::policy::projection::{params_commitment, EntryOrigin};
use fid_bus::principal::{Principal, PrincipalKind};
use fid_canon::CValue;
use fid_journal::branch::{EntryOrigin as JournalOrigin, EventSource, MAIN};
use fid_profile::defs::MAX_INPUT_BYTES;
use fid_profile::{instantiate, CanonicalProfile, ProfileError, ProfileName};
use fid_registry::{CommandDef, DefError};
use fid_types::{CommandId, Digest, SideEffect};
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
/// The tenant configuration key the folded state stands for. Never a `CORE.STO.SET` setting.
pub const CONFIG_KEY: &str = "trust.profile";
/// The `path_like` parameter that names the tenant parameter file.
pub const PARAMS_FILE: &str = "params";
/// `decided_at` of the first profile decision.
pub const GENESIS: &str = "genesis";
/// Event types of the pending-request flow (CORE.BUS.POL.PROJ); none may share an entry with
/// a `ProfileSet` event.
pub const REQUEST_EVENT_PREFIX: &str = "Request";

const LIFECYCLE_SINCE: &str = "2026-11-21";
const BODY_KEYS: [&str; 6] = ["decided_at", "document", "name", "new_digest", "old_digest", "params_commitment"];

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

/// One committed entry as its verified header records it (module docs). Never built from an
/// event body.
#[derive(Debug, Clone, PartialEq)]
pub struct ProfileEntry {
    /// Principal, command `id@major` and parameters commitment (`EntryOrigin::from_draft`).
    pub origin: EntryOrigin,
    /// Principal kind, command, class, branch and the stage that added the events.
    pub journal: JournalOrigin,
    /// The header's `promoted_from`: the entry this one is a promoted copy of, if any.
    pub promoted_from: Option<Digest>,
    /// This entry's hash.
    pub hash: Digest,
}

impl ProfileEntry {
    /// The record the commit stage gives the entry of a `sign.set_profile@1` call by
    /// `principal` with parameters commitment `commitment`, committed directly on `main`.
    pub fn for_call(principal: &Principal, commitment: Digest, hash: Digest) -> Result<ProfileEntry, BusError> {
        Ok(ProfileEntry {
            origin: EntryOrigin::new(principal.reference(), set_id()?, commitment),
            journal: JournalOrigin {
                kind: principal.kind(),
                command: SET_PROFILE.to_string(),
                major: 1,
                class: SideEffect::A0,
                branch: MAIN.to_string(),
                source: EventSource::Handler,
                human_assertion: None,
            },
            promoted_from: None,
            hash,
        })
    }
}

/// The active profile, folded from committed `main` entries in journal order (module docs).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ProfileState {
    active: Option<CanonicalProfile>,
    /// Hash of the last applied `ProfileSet` entry.
    decided: Option<Digest>,
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

/// What the fold would do with an entry.
enum Verdict {
    /// Nothing for the profile (no `ProfileSet` event, or not on `main`).
    Ignore,
    /// Apply this document.
    Apply(CanonicalProfile),
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

    /// The `decided_at` a new `ProfileSet` event must carry: the hash of the last applied
    /// `ProfileSet` entry, or [`GENESIS`].
    pub fn decided_at(&self) -> String {
        match &self.decided {
            Some(d) => d.as_str().to_string(),
            None => GENESIS.to_string(),
        }
    }

    /// The value of [`CONFIG_KEY`]: the active document's canonical bytes, `None` when nothing
    /// is set (the default applies).
    pub fn config_value(&self) -> Option<&[u8]> {
        self.active.as_ref().map(CanonicalProfile::bytes)
    }

    /// The verdict on one entry. `decided_at` mismatches are reported with `stale`, so the
    /// commit stage can map them to `conflict` while the fold keeps them an invariant.
    fn verdict(
        &self,
        entry: &ProfileEntry,
        events: &[Event],
        stale: &dyn Fn() -> BusError,
    ) -> Result<Verdict, BusError> {
        let mut mine = events.iter().filter(|e| e.type_id().as_str() == PROFILE_SET);
        let Some(event) = mine.next() else { return Ok(Verdict::Ignore) };
        if entry.journal.branch != MAIN {
            return Ok(Verdict::Ignore);
        }
        if mine.next().is_some() {
            return Err(invariant("one entry holds two ProfileSet events"));
        }
        if events.iter().any(|e| e.type_id().as_str().starts_with(REQUEST_EVENT_PREFIX)) {
            return Err(invariant("ProfileSet in an entry that carries a request event"));
        }
        if entry.promoted_from.is_some() {
            return Err(invariant("ProfileSet in a promoted copy; sign.set_profile@1 runs directly on main"));
        }
        let cmd = entry.origin.command();
        let j = &entry.journal;
        if cmd.name() != SET_PROFILE || cmd.major() != 1 || j.command != SET_PROFILE || j.major != 1 {
            return Err(invariant("ProfileSet outside a sign.set_profile@1 entry"));
        }
        if j.class != SideEffect::A0 || j.source != EventSource::Handler {
            return Err(invariant("ProfileSet not added by the sign.set_profile@1 handler in an A0 entry"));
        }
        if entry.origin.principal().kind != PrincipalKind::Human || j.kind != PrincipalKind::Human {
            return Err(invariant("ProfileSet in an entry of a non-human principal"));
        }
        let CValue::Obj(o) = event.body() else { return Err(invariant("ProfileSet body is not an object")) };
        if o.len() != BODY_KEYS.len() || !BODY_KEYS.iter().all(|k| o.contains_key(*k)) {
            return Err(invariant("ProfileSet body does not have exactly its six members"));
        }
        if digest_member(o, "params_commitment")? != entry.origin.params_commitment() {
            return Err(invariant("ProfileSet in an entry whose parameters are not the event's"));
        }
        if str_member(o, "decided_at")? != self.decided_at() {
            return Err(stale());
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
        Ok(Verdict::Apply(doc))
    }

    /// The commit stage's check, run under its compare-and-swap of the profile decision
    /// (see [`ProfileSource`]): the same rules as the fold, except that a `ProfileSet` whose
    /// `decided_at` is not the current decision is `conflict/head_moved` (another set
    /// committed first); the caller then commits nothing.
    pub fn precommit(&self, entry: &ProfileEntry, events: &[Event]) -> Result<(), BusError> {
        let stale = || {
            BusError::Conflict(
                ConflictReason::HeadMoved,
                Detail::new("the governance profile changed since this call read it"),
            )
        };
        self.verdict(entry, events, &stale).map(|_| ())
    }

    /// Folds the events of one committed entry (module docs). On refusal
    /// (`internal/invariant_violated`, a `decided_at` mismatch included) the state is unchanged.
    pub fn apply_entry(&mut self, entry: &ProfileEntry, events: &[Event]) -> Result<(), BusError> {
        let stale = || invariant("ProfileSet decided_at is not the last applied profile decision");
        if let Verdict::Apply(doc) = self.verdict(entry, events, &stale)? {
            self.active = Some(doc);
            self.decided = Some(entry.hash);
        }
        Ok(())
    }
}

/// Where the handlers read the active profile. The bus wiring implements it over the journal.
///
/// Contract:
///
/// - [`ProfileSource::state`] is [`ProfileState::apply_entry`] over the events of every
///   committed entry of the tenant's journal, in append order, each with the [`ProfileEntry`]
///   its verified header records (`EntryOrigin::from_draft`, the journal's origin with its
///   branch and adding stage, `promoted_from`, the entry hash). The wiring checks the payload's
///   events against the header's `events_commit` before folding, as
///   `fid_journal_cmd::BranchSource` requires.
/// - The commit stage, before it commits an entry that holds a `ProfileSet` event, runs
///   [`ProfileState::precommit`] on the state as of its commit, atomically with the commit (a
///   compare-and-swap on [`ProfileState::decided_at`]), and commits nothing on refusal
///   (`conflict/head_moved` for a concurrent set). A committed entry the fold refuses is then
///   a commit-stage bug, and the state fails closed.
/// - [`ProfileSource::call_branch`] is the branch the commit stage would commit the call's
///   entry to.
pub trait ProfileSource: Send + Sync {
    /// The folded state.
    fn state(&self) -> Result<ProfileState, String>;
    /// The branch the entry of the call `ctx` would commit to.
    fn call_branch(&self, ctx: &HandlerCtx) -> Result<String, String>;
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
    let branch = source.call_branch(ctx).map_err(|e| invariant(&format!("call branch: {e}")))?;
    if branch != MAIN {
        return Err(BusError::Precondition(
            PreconditionReason::False,
            Detail::new("sign.set_profile@1 runs only directly on main"),
        ));
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
        ("decided_at", text(&state.decided_at())),
        ("document", text(document)),
        ("name", text(p.name.as_str())),
        ("new_digest", text(new.digest().as_str())),
        ("old_digest", text(old.as_str())),
        ("params_commitment", text(commitment.as_str())),
    ]);
    let event = Event::new(EventType::new(PROFILE_SET)?, body);
    // The record its entry will have (the hash is not known yet and not checked): the fold
    // must accept the event from it.
    let entry = ProfileEntry::for_call(principal, commitment, Digest::TOMBSTONE)?;
    state.precommit(&entry, std::slice::from_ref(&event))?;
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
