//! The eight lifecycle commands, declared with the registry macros, with their CLI and MCP
//! entry points.
//!
//! | Command | Class | MCP |
//! |---|---|---|
//! | `program.register@1`, `script.register@1` | W2 | exposed |
//! | `program.unregister@1`, `script.unregister@1` | W2 | exposed |
//! | `program.release@1`, `script.release@1` | W3, `human_only` | request-only |
//! | `program.deprecate@1`, `script.deprecate@1` | W3, `human_only` | request-only |
//!
//! Parameters: the artifact ID (`program` or `script`), `expected_digest` and an optional
//! `note`. For `register`, `expected_digest` is the digest of the draft the caller means to
//! freeze; for the others it names the version. A mismatch is `conflict`. Results: `state`,
//! `version`, `digest`.
//!
//! CLI: `fid program|script register|unregister|release|deprecate --program <id> |
//! --script <id> --expected-digest <sha256:..> [--note <text>] [--json]`.
//!
//! MCP: `program_register`, ..., `script_deprecate`. The W3 tools are **request-only**:
//! [`invoke_mcp`] reads the parameters strictly, files a request for a person through the
//! [`LifeRequests`] seam and returns [`McpReply::RequestFiled`]; it never runs the command,
//! so the state does not change.
//!
//! **Caller and branch.** Every handler first runs the bus class check
//! ([`fid_bus::policy::class::authorise_class`]) against its own declared definition. A W3
//! command then also requires the principal kind to be `human`. Branch rule: a person writes
//! lifecycle state only off agent branches, every non-human principal only on an agent branch
//! `agent/<client>/<n>`. The kind and the branch come from the context the bus builds, never
//! from a parameter.
//!
//! **Release preconditions** are declared in the registry (FPL, `CORE.REG.DEF.PRED`) over the
//! context root `artifact` ([`context_types`]): `artifact.check_passed` (`program.check@1`
//! passed) and `size(artifact.unadopted) == 0` (`AUTO.REC.ADOPT`). Until the bus evaluates
//! preconditions itself (`CORE.BUS.PIPE.VALID`), the handler compiles its own declared
//! predicates and evaluates them with the registry's evaluator, so the declaration is the rule.
//!
//! A refused or failing command changes nothing: every check runs on read-only views, then
//! the event is committed to the journal, and only after a successful commit is the store
//! changed.

use super::event::LifeEvent;
use super::{
    echo, transition_allowed, ArtifactId, ArtifactKind, Change, CheckVerdict, LifeError, LifeState, LifeStore,
    ProgramChecker, ScriptBundle, MAX_NOTE_BYTES,
};
use crate::adopt::{is_person, ProvenanceStore, ScriptId, Target, UUID7_PATTERN};
use fid_bus::principal::{Principal, PrincipalRef};
use fid_canon::{digest_bytes, parse_with_limits, to_canonical_bytes, CValue};
use fid_program::{Program, Uuid7};
use fid_registry::def::Projection;
use fid_registry::{
    compile_command, evaluate, fid_command, fid_commands, CommandDef, ContextTypes, FidSchema, FieldPath, PredContext,
    PredError, Type, Value, Verdict,
};
use fid_types::{Digest, SideEffect};
use std::collections::BTreeMap;

/// `program.register@1`.
pub const PROGRAM_REGISTER: &str = "program.register@1";
/// `program.unregister@1`.
pub const PROGRAM_UNREGISTER: &str = "program.unregister@1";
/// `program.release@1`.
pub const PROGRAM_RELEASE: &str = "program.release@1";
/// `program.deprecate@1`.
pub const PROGRAM_DEPRECATE: &str = "program.deprecate@1";
/// `script.register@1`.
pub const SCRIPT_REGISTER: &str = "script.register@1";
/// `script.unregister@1`.
pub const SCRIPT_UNREGISTER: &str = "script.unregister@1";
/// `script.release@1`.
pub const SCRIPT_RELEASE: &str = "script.release@1";
/// `script.deprecate@1`.
pub const SCRIPT_DEPRECATE: &str = "script.deprecate@1";

/// Largest MCP parameter payload, in bytes.
pub const MAX_PARAMS_BYTES: usize = 1024 + 6 * MAX_NOTE_BYTES;
const MAX_PARAMS_DEPTH: usize = 2;
/// Most CLI words.
const MAX_CLI_ARGS: usize = 12;

// ------------------------------------------------------------------ verbs

/// The four lifecycle commands of each artifact kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Verb {
    /// draft → registered (W2).
    Register,
    /// registered → draft (W2).
    Unregister,
    /// registered → released (W3).
    Release,
    /// released → deprecated (W3).
    Deprecate,
}

impl Verb {
    /// All four, in declaration order.
    pub const ALL: [Verb; 4] = [Verb::Register, Verb::Unregister, Verb::Release, Verb::Deprecate];

    /// The CLI subcommand and command-name suffix.
    pub fn as_str(self) -> &'static str {
        match self {
            Verb::Register => "register",
            Verb::Unregister => "unregister",
            Verb::Release => "release",
            Verb::Deprecate => "deprecate",
        }
    }

    fn parse(s: &str) -> Option<Verb> {
        Verb::ALL.into_iter().find(|v| v.as_str() == s)
    }

    /// The state the transition leads to.
    pub fn to_state(self) -> LifeState {
        match self {
            Verb::Register => LifeState::Registered,
            Verb::Unregister => LifeState::Draft,
            Verb::Release => LifeState::Released,
            Verb::Deprecate => LifeState::Deprecated,
        }
    }

    /// The command ID with major for an artifact kind.
    pub fn command(self, kind: ArtifactKind) -> &'static str {
        match (kind, self) {
            (ArtifactKind::Program, Verb::Register) => PROGRAM_REGISTER,
            (ArtifactKind::Program, Verb::Unregister) => PROGRAM_UNREGISTER,
            (ArtifactKind::Program, Verb::Release) => PROGRAM_RELEASE,
            (ArtifactKind::Program, Verb::Deprecate) => PROGRAM_DEPRECATE,
            (ArtifactKind::Script, Verb::Register) => SCRIPT_REGISTER,
            (ArtifactKind::Script, Verb::Unregister) => SCRIPT_UNREGISTER,
            (ArtifactKind::Script, Verb::Release) => SCRIPT_RELEASE,
            (ArtifactKind::Script, Verb::Deprecate) => SCRIPT_DEPRECATE,
        }
    }

    /// Reads a command ID with major.
    pub fn from_command(s: &str) -> Option<(ArtifactKind, Verb)> {
        [ArtifactKind::Program, ArtifactKind::Script]
            .into_iter()
            .flat_map(|k| Verb::ALL.into_iter().map(move |v| (k, v)))
            .find(|(k, v)| v.command(*k) == s)
    }
}

// ------------------------------------------------------------------ context

/// The project's current drafts.
pub trait Drafts {
    /// The current draft of program `id`, if it exists.
    fn program(&self, id: &Uuid7) -> Option<&Program>;
    /// The current draft of script `id`, if it exists.
    fn script(&self, id: &ScriptId) -> Option<&ScriptBundle>;
}

/// The journal seam the host wires (the Record Journal behind the bus). `Ok` holds the
/// sequence number of the committed, durable entry; `Err` means nothing was committed
/// (display-safe reason).
pub trait LifeJournal {
    /// Commits one lifecycle event on the branch it names.
    fn commit(&mut self, event: &LifeEvent) -> Result<u64, String>;
}

/// A request for a person to run a W3 lifecycle command, filed by a request-only MCP call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LifeRequest {
    /// The command ID with major.
    pub command: &'static str,
    /// The canonical parameter object.
    pub params: Vec<u8>,
    /// The bus-derived principal that asked.
    pub requester: PrincipalRef,
}

/// The request queue the host wires (the Proposal Inbox). `Ok` holds the request ID.
pub trait LifeRequests {
    /// Files one request.
    fn file(&mut self, request: &LifeRequest) -> Result<String, String>;
}

/// The context every handler gets.
pub struct LifeCtx<'a> {
    /// The bus-derived principal of the call.
    pub principal: &'a Principal,
    /// The branch the bus routed the call to.
    pub branch: &'a str,
    /// The lifecycle state of that branch.
    pub store: &'a mut LifeStore,
    /// The project's drafts.
    pub drafts: &'a dyn Drafts,
    /// The provenance store of `AUTO.REC.ADOPT`; script registration records authorship.
    pub provenance: &'a mut ProvenanceStore,
    /// `program.check@1`.
    pub checker: &'a dyn ProgramChecker,
    /// The journal seam.
    pub journal: &'a mut dyn LifeJournal,
    /// The request seam for request-only MCP calls.
    pub requests: &'a mut dyn LifeRequests,
}

// ------------------------------------------------------------------ params and results

/// Parameters of the four `program.*` lifecycle commands.
#[derive(FidSchema, Debug, Clone, PartialEq, Eq)]
pub struct ProgramLifeParams {
    #[fid(description = "The program (its program_id).", pattern = UUID7_PATTERN)]
    pub program: String,
    #[fid(
        description = "Digest of the draft to register, or of the version to change; a mismatch is a conflict.",
        pattern = Digest::PATTERN
    )]
    pub expected_digest: String,
    #[fid(description = "An optional note recorded in the journal entry.", min_length = 1, max_length = MAX_NOTE_BYTES)]
    pub note: Option<String>,
}

/// Parameters of the four `script.*` lifecycle commands.
#[derive(FidSchema, Debug, Clone, PartialEq, Eq)]
pub struct ScriptLifeParams {
    #[fid(description = "The script.", pattern = crate::adopt::SCRIPT_ID_PATTERN)]
    pub script: String,
    #[fid(
        description = "Digest of the draft bundle to register, or of the version to change; a mismatch is a conflict.",
        pattern = Digest::PATTERN
    )]
    pub expected_digest: String,
    #[fid(description = "An optional note recorded in the journal entry.", min_length = 1, max_length = MAX_NOTE_BYTES)]
    pub note: Option<String>,
}

/// Result of every lifecycle command.
#[derive(FidSchema, Debug, Clone, PartialEq, Eq)]
pub struct LifeResult {
    #[fid(description = "The version's state after the command.")]
    pub state: LifeState,
    #[fid(description = "The version number in the artifact's lineage, from 1.")]
    pub version: u32,
    #[fid(description = "SHA-256 of the version's canonical bytes: its identity.", pattern = Digest::PATTERN)]
    pub digest: String,
}

impl LifeResult {
    fn from_event(e: &LifeEvent) -> LifeResult {
        LifeResult { state: e.state, version: e.version, digest: e.digest.as_str().to_owned() }
    }

    /// The JSON value.
    pub fn to_value(&self) -> CValue {
        let mut o = BTreeMap::new();
        o.insert("digest".to_owned(), CValue::Str(self.digest.clone()));
        o.insert("state".to_owned(), CValue::Str(self.state.as_str().to_owned()));
        o.insert("version".to_owned(), CValue::Num(f64::from(self.version)));
        CValue::Obj(o)
    }

    /// Canonical JSON bytes, no trailing newline.
    pub fn to_canonical_bytes(&self) -> Vec<u8> {
        to_canonical_bytes(&self.to_value()).unwrap_or_default()
    }
}

/// One parsed call of any lifecycle command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LifeCall {
    /// The command.
    pub verb: Verb,
    /// The artifact kind.
    pub kind: ArtifactKind,
    /// The artifact ID as given.
    pub artifact: String,
    /// The expected digest as given.
    pub expected_digest: String,
    /// The note, if any.
    pub note: Option<String>,
}

impl LifeCall {
    /// The command ID with major.
    pub fn command(&self) -> &'static str {
        self.verb.command(self.kind)
    }

    /// The parameter object (the shape the command's schema describes).
    pub fn to_value(&self) -> CValue {
        let mut o = BTreeMap::new();
        o.insert(self.kind.as_str().to_owned(), CValue::Str(self.artifact.clone()));
        o.insert("expected_digest".to_owned(), CValue::Str(self.expected_digest.clone()));
        if let Some(n) = &self.note {
            o.insert("note".to_owned(), CValue::Str(n.clone()));
        }
        CValue::Obj(o)
    }
}

/// What an MCP call returns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum McpReply {
    /// A W2 command ran and committed: the canonical result JSON.
    Committed(Vec<u8>),
    /// A W3 command was not run; a request for a person was filed.
    RequestFiled {
        /// The request ID from the request seam.
        request_id: String,
    },
}

impl McpReply {
    /// The surface outcome name (`AUTO.SURF.ENV`).
    pub fn outcome(&self) -> &'static str {
        match self {
            McpReply::Committed(_) => "committed",
            McpReply::RequestFiled { .. } => "request_filed",
        }
    }
}

// ------------------------------------------------------------------ release preconditions

/// The types of the `artifact` context root the release preconditions read.
pub fn context_types() -> ContextTypes {
    ContextTypes::new()
        .with("artifact.check_passed", Type::Bool)
        .with("artifact.unadopted", Type::List(Box::new(Type::String)))
}

/// What the release preconditions see about the version to release.
struct Facts {
    check_passed: bool,
    diagnostics: Vec<String>,
    unadopted: Vec<String>,
}

impl PredContext for Facts {
    fn get(&self, path: &FieldPath) -> Result<Value, PredError> {
        let p: Vec<&str> = path.0.iter().map(String::as_str).collect();
        match p.as_slice() {
            ["artifact", "check_passed"] => Ok(Value::Bool(self.check_passed)),
            ["artifact", "unadopted"] => {
                Ok(Value::List(self.unadopted.iter().map(|s| Value::Str(s.clone())).collect()))
            }
            _ => Err(PredError::MissingField { path: path.to_string() }),
        }
    }
}

/// Evaluates the declared preconditions of `def` in order; the first false one maps to its
/// error. A definition without preconditions, a predicate that does not compile or one that
/// errors is `internal` (fail closed).
fn release_preconditions(def: &CommandDef, facts: &Facts, digest: &Digest) -> Result<(), LifeError> {
    let preds = compile_command(def, &context_types())
        .map_err(|_| LifeError::Internal { reason: "a release precondition does not compile" })?;
    if preds.is_empty() {
        return Err(LifeError::Internal { reason: "release declares no preconditions" });
    }
    for p in &preds {
        match evaluate(p, facts).result {
            Ok(Verdict::True) => {}
            Ok(Verdict::False { message_key }) => {
                return Err(match message_key.as_str() {
                    "check_failed" => LifeError::CheckFailed { diagnostics: facts.diagnostics.clone() },
                    "unadopted_steps" => LifeError::UnadoptedSteps { steps: facts.unadopted.clone() },
                    "unadopted_version" => LifeError::UnadoptedVersion { version: digest.as_str().to_owned() },
                    _ => LifeError::Internal { reason: "unknown precondition message key" },
                })
            }
            Err(_) => return Err(LifeError::Internal { reason: "a release precondition could not be evaluated" }),
        }
    }
    Ok(())
}

// ------------------------------------------------------------------ checks shared with replay

/// Read-only inputs of a transition check.
pub(crate) struct View<'a> {
    pub(crate) store: &'a LifeStore,
    pub(crate) drafts: &'a dyn Drafts,
    pub(crate) provenance: &'a ProvenanceStore,
    pub(crate) checker: &'a dyn ProgramChecker,
}

/// A checked transition: the event to journal and the change to apply after the commit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Prepared {
    pub(crate) event: LifeEvent,
    pub(crate) change: Change,
}

/// Who runs a transition, and where.
pub(crate) struct Actor<'a> {
    pub(crate) by: &'a str,
    pub(crate) branch: &'a str,
}

fn draft_known(drafts: &dyn Drafts, id: &ArtifactId) -> bool {
    match id {
        ArtifactId::Program(p) => drafts.program(p).is_some(),
        ArtifactId::Script(s) => drafts.script(s).is_some(),
    }
}

/// The canonical bytes of the current draft of `id`.
fn draft_bytes(drafts: &dyn Drafts, id: &ArtifactId) -> Result<Vec<u8>, LifeError> {
    let missing = || LifeError::ArtifactNotFound { artifact: id.to_string() };
    match id {
        ArtifactId::Program(p) => {
            let prog = drafts.program(p).ok_or_else(missing)?;
            if prog.program_id != *p {
                return Err(LifeError::Internal { reason: "the draft has another program_id" });
            }
            prog.to_canonical().map_err(|_| LifeError::Internal { reason: "the draft program cannot be serialised" })
        }
        ArtifactId::Script(s) => drafts
            .script(s)
            .ok_or_else(missing)?
            .to_canonical_bytes()
            .ok_or(LifeError::Internal { reason: "the draft bundle cannot be serialised" }),
    }
}

/// The release facts of a frozen version.
fn facts(view: &View<'_>, id: &ArtifactId, digest: &Digest, bytes: &[u8]) -> Result<Facts, LifeError> {
    match id {
        ArtifactId::Program(_) => {
            let program =
                Program::parse(bytes).map_err(|_| LifeError::Internal { reason: "a frozen program does not parse" })?;
            let (check_passed, diagnostics) = match view.checker.check(&program) {
                CheckVerdict::Passed => (true, Vec::new()),
                CheckVerdict::Failed { diagnostics } => (false, diagnostics),
            };
            let unadopted = view.provenance.unadopted_steps(&program).iter().map(Uuid7::to_string).collect();
            Ok(Facts { check_passed, diagnostics, unadopted })
        }
        ArtifactId::Script(s) => {
            let unadopted =
                if view.provenance.unadopted_script(s, digest) { vec![digest.as_str().to_owned()] } else { Vec::new() };
            Ok(Facts { check_passed: true, diagnostics: Vec::new(), unadopted })
        }
    }
}

/// Runs every check of a transition against read-only views and returns what to commit.
/// Order: register: open draft, draft exists, digest, version limit; the others: version by
/// digest, transition table, open draft (unregister), release preconditions.
pub(crate) fn prepare(
    view: &View<'_>,
    verb: Verb,
    id: &ArtifactId,
    expected: &Digest,
    actor: &Actor<'_>,
    note: Option<String>,
) -> Result<Prepared, LifeError> {
    let lv = view.store.lineage_view(id);
    let to = verb.to_state();
    let artifact = id.to_string();
    let event = |version: u32, digest: Digest, source: Option<Digest>, note: Option<String>| LifeEvent {
        verb,
        artifact: id.clone(),
        branch: actor.branch.to_owned(),
        by: actor.by.to_owned(),
        version,
        digest,
        state: to,
        source_digest: source,
        note,
    };
    if verb == Verb::Register {
        if !lv.draft_open() || !transition_allowed(LifeState::Draft, to) {
            let from = lv.head_state().unwrap_or(LifeState::Registered);
            return Err(LifeError::IllegalTransition { artifact, from, to });
        }
        let bytes = draft_bytes(view.drafts, id)?;
        let digest = Digest::from_bytes(digest_bytes(&bytes).0);
        if digest != *expected {
            return Err(LifeError::Conflict { detail: format!("the draft is {}", digest.as_str()) });
        }
        let version = lv.next_version().ok_or(LifeError::Internal { reason: "the lineage has too many versions" })?;
        let source = lv.draft_source();
        return Ok(Prepared {
            event: event(version, digest, source, note),
            change: Change::Register { id: id.clone(), digest, source, bytes },
        });
    }
    let Some((version, state, source, bytes)) = lv.find(expected) else {
        if lv.exists() {
            return Err(LifeError::Conflict {
                detail: "no version of the artifact has the expected digest".to_owned(),
            });
        }
        if draft_known(view.drafts, id) {
            return Err(LifeError::IllegalTransition { artifact, from: LifeState::Draft, to });
        }
        return Err(LifeError::ArtifactNotFound { artifact });
    };
    if !transition_allowed(state, to) {
        return Err(LifeError::IllegalTransition { artifact, from: state, to });
    }
    if verb == Verb::Unregister && lv.draft_open() {
        return Err(LifeError::Conflict { detail: "a draft of the artifact is already open".to_owned() });
    }
    if verb == Verb::Release {
        let f = facts(view, id, expected, bytes)?;
        release_preconditions(&def_for(verb, id.kind()), &f, expected)?;
    }
    Ok(Prepared {
        event: event(version, *expected, source, note),
        change: Change::SetState { id: id.clone(), version, to },
    })
}

/// Whether `b` is an agent branch `agent/<client>/<n>` (CORE.BUS.ROUTE): `<client>` 1 to 32
/// of `[a-z0-9-]`, `<n>` a positive integer without a leading zero.
pub fn is_agent_branch(b: &str) -> bool {
    let mut parts = b.split('/');
    let (Some("agent"), Some(client), Some(n), None) = (parts.next(), parts.next(), parts.next(), parts.next()) else {
        return false;
    };
    let client_ok = !client.is_empty()
        && client.len() <= 32
        && client.bytes().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-');
    let n_ok = !n.is_empty() && n.len() <= 20 && !n.starts_with('0') && n.bytes().all(|c| c.is_ascii_digit());
    client_ok && n_ok
}

/// The branch rule for a principal kind: a person writes off agent branches, every other
/// kind only on an agent branch.
pub(crate) fn branch_allowed(person: bool, branch: &str) -> Result<(), LifeError> {
    match (person, is_agent_branch(branch)) {
        (true, true) => Err(LifeError::PolicyRefused { reason: "agent_branch" }),
        (false, false) => Err(LifeError::PolicyRefused { reason: "not_agent_branch" }),
        (true, false) | (false, true) => Ok(()),
    }
}

/// The caller check: the bus class rule for `def`, then for W3 the principal kind itself,
/// then the branch rule. Everything else is refused.
fn authorise(principal: &Principal, branch: &str, def: &CommandDef) -> Result<(), LifeError> {
    fid_bus::policy::class::authorise_class(principal, def)
        .map_err(|e| LifeError::PolicyRefused { reason: e.reason() })?;
    let person = is_person(principal.kind());
    if def.side_effect == SideEffect::W3 && (!def.human_only || !person) {
        // The declarations set human_only on every W3 command; without it, fail closed.
        return Err(LifeError::PolicyRefused { reason: "human_only" });
    }
    branch_allowed(person, branch)
}

fn check_note(note: Option<&str>) -> Result<Option<String>, LifeError> {
    match note {
        None => Ok(None),
        Some(n) if n.is_empty() || n.len() > MAX_NOTE_BYTES => Err(LifeError::bad("note must be 1 to 1024 bytes")),
        Some(n) if n.chars().any(char::is_control) => Err(LifeError::bad("note holds a control character")),
        Some(n) => Ok(Some(n.to_owned())),
    }
}

fn run(
    ctx: &mut LifeCtx<'_>,
    verb: Verb,
    kind: ArtifactKind,
    artifact: &str,
    expected: &str,
    note: Option<&str>,
) -> Result<LifeResult, LifeError> {
    authorise(ctx.principal, ctx.branch, &def_for(verb, kind))?;
    let id = ArtifactId::parse(kind, artifact)?;
    let expected =
        Digest::parse(expected).map_err(|_| LifeError::bad("expected_digest is not sha256:<64 lowercase hex>"))?;
    let note = check_note(note)?;
    let by = ctx.principal.id().as_str().to_owned();
    let prepared = {
        let view = View { store: &*ctx.store, drafts: ctx.drafts, provenance: &*ctx.provenance, checker: ctx.checker };
        prepare(&view, verb, &id, &expected, &Actor { by: &by, branch: ctx.branch }, note)?
    };
    let entry = ctx.journal.commit(&prepared.event).map_err(|reason| LifeError::Journal { reason: echo(&reason) })?;
    ctx.store.apply(&prepared.change, entry);
    if let (Verb::Register, ArtifactId::Script(script)) = (verb, &id) {
        // AUTO.REC.ADOPT: script registration records who authored the version, so an agent's
        // version needs a person's adoption before release.
        let digest = prepared.event.digest;
        ctx.provenance.record_authorship(
            Target::ScriptVersion { script: script.clone(), version: digest },
            digest,
            ctx.principal,
            entry,
        );
    }
    Ok(LifeResult::from_event(&prepared.event))
}

// ------------------------------------------------------------------ the commands

#[fid_command(
    id = "program.register",
    major = 1,
    title = "Register program",
    description = "Freezes the draft program, assigns the next version number of its lineage and identifies it by the SHA-256 of its canonical bytes. Agents register on their agent branch.",
    side_effect = W2,
    mcp_projection = Exposed,
    determinism = D0,
    idempotency = None,
    fs_effect = None,
    undo = HeadMove,
    preconditions = [],
    effects = [PROGRAM_REGISTER],
    algorithms = [],
    exposure = [Cli, Mcp, Gui],
    lifecycle(state = "active", since = "2026-11-19"),
    web = Available,
)]
pub fn program_register(ctx: &mut LifeCtx<'_>, params: ProgramLifeParams) -> Result<LifeResult, LifeError> {
    run(ctx, Verb::Register, ArtifactKind::Program, &params.program, &params.expected_digest, params.note.as_deref())
}

#[fid_command(
    id = "program.unregister",
    major = 1,
    title = "Unregister program",
    description = "Returns a registered program version to draft so it can be edited again; its version number is retired.",
    side_effect = W2,
    mcp_projection = Exposed,
    determinism = D0,
    idempotency = None,
    fs_effect = None,
    undo = HeadMove,
    preconditions = [],
    effects = [PROGRAM_UNREGISTER],
    algorithms = [],
    exposure = [Cli, Mcp, Gui],
    lifecycle(state = "active", since = "2026-11-19"),
    web = Available,
)]
pub fn program_unregister(ctx: &mut LifeCtx<'_>, params: ProgramLifeParams) -> Result<LifeResult, LifeError> {
    run(ctx, Verb::Unregister, ArtifactKind::Program, &params.program, &params.expected_digest, params.note.as_deref())
}

#[fid_command(
    id = "program.release",
    major = 1,
    title = "Release program",
    description = "A person releases a registered program version: it must pass program.check@1 and have no unadopted agent-authored steps. Released programs are immutable. Agents may request it.",
    side_effect = W3,
    mcp_projection = RequestOnly,
    determinism = D0,
    idempotency = None,
    fs_effect = None,
    undo = NotUndoable,
    preconditions = [
        (expr = "artifact.check_passed", bound = 4, message_key = "check_failed"),
        (expr = "size(artifact.unadopted) == 0", bound = 8, message_key = "unadopted_steps")
    ],
    effects = [PROGRAM_RELEASE],
    algorithms = [],
    exposure = [Cli, Mcp, Gui],
    lifecycle(state = "active", since = "2026-11-19"),
    human_only = true,
    web = Available,
)]
pub fn program_release(ctx: &mut LifeCtx<'_>, params: ProgramLifeParams) -> Result<LifeResult, LifeError> {
    run(ctx, Verb::Release, ArtifactKind::Program, &params.program, &params.expected_digest, params.note.as_deref())
}

#[fid_command(
    id = "program.deprecate",
    major = 1,
    title = "Deprecate program",
    description = "A person deprecates a released program version. Its bytes and earlier run records are unchanged; new runs are refused unless a person overrides. Agents may request it.",
    side_effect = W3,
    mcp_projection = RequestOnly,
    determinism = D0,
    idempotency = None,
    fs_effect = None,
    undo = NotUndoable,
    preconditions = [],
    effects = [PROGRAM_DEPRECATE],
    algorithms = [],
    exposure = [Cli, Mcp, Gui],
    lifecycle(state = "active", since = "2026-11-19"),
    human_only = true,
    web = Available,
)]
pub fn program_deprecate(ctx: &mut LifeCtx<'_>, params: ProgramLifeParams) -> Result<LifeResult, LifeError> {
    run(ctx, Verb::Deprecate, ArtifactKind::Program, &params.program, &params.expected_digest, params.note.as_deref())
}

#[fid_command(
    id = "script.register",
    major = 1,
    title = "Register script",
    description = "Freezes the draft script bundle, assigns the next version number of its lineage and identifies it by the SHA-256 of its canonical bytes. Agents register on their agent branch.",
    side_effect = W2,
    mcp_projection = Exposed,
    determinism = D0,
    idempotency = None,
    fs_effect = None,
    undo = HeadMove,
    preconditions = [],
    effects = [SCRIPT_REGISTER],
    algorithms = [],
    exposure = [Cli, Mcp, Gui],
    lifecycle(state = "active", since = "2026-11-19"),
    web = Available,
)]
pub fn script_register(ctx: &mut LifeCtx<'_>, params: ScriptLifeParams) -> Result<LifeResult, LifeError> {
    run(ctx, Verb::Register, ArtifactKind::Script, &params.script, &params.expected_digest, params.note.as_deref())
}

#[fid_command(
    id = "script.unregister",
    major = 1,
    title = "Unregister script",
    description = "Returns a registered script version to draft so it can be edited again; its version number is retired.",
    side_effect = W2,
    mcp_projection = Exposed,
    determinism = D0,
    idempotency = None,
    fs_effect = None,
    undo = HeadMove,
    preconditions = [],
    effects = [SCRIPT_UNREGISTER],
    algorithms = [],
    exposure = [Cli, Mcp, Gui],
    lifecycle(state = "active", since = "2026-11-19"),
    web = Available,
)]
pub fn script_unregister(ctx: &mut LifeCtx<'_>, params: ScriptLifeParams) -> Result<LifeResult, LifeError> {
    run(ctx, Verb::Unregister, ArtifactKind::Script, &params.script, &params.expected_digest, params.note.as_deref())
}

#[fid_command(
    id = "script.release",
    major = 1,
    title = "Release script",
    description = "A person releases a registered script version; an agent-registered version must first be adopted by a person. Released scripts are immutable. Agents may request it.",
    side_effect = W3,
    mcp_projection = RequestOnly,
    determinism = D0,
    idempotency = None,
    fs_effect = None,
    undo = NotUndoable,
    preconditions = [(expr = "size(artifact.unadopted) == 0", bound = 8, message_key = "unadopted_version")],
    effects = [SCRIPT_RELEASE],
    algorithms = [],
    exposure = [Cli, Mcp, Gui],
    lifecycle(state = "active", since = "2026-11-19"),
    human_only = true,
    web = Available,
)]
pub fn script_release(ctx: &mut LifeCtx<'_>, params: ScriptLifeParams) -> Result<LifeResult, LifeError> {
    run(ctx, Verb::Release, ArtifactKind::Script, &params.script, &params.expected_digest, params.note.as_deref())
}

#[fid_command(
    id = "script.deprecate",
    major = 1,
    title = "Deprecate script",
    description = "A person deprecates a released script version. Its bytes and earlier run records are unchanged; new runs are refused unless a person overrides. Agents may request it.",
    side_effect = W3,
    mcp_projection = RequestOnly,
    determinism = D0,
    idempotency = None,
    fs_effect = None,
    undo = NotUndoable,
    preconditions = [],
    effects = [SCRIPT_DEPRECATE],
    algorithms = [],
    exposure = [Cli, Mcp, Gui],
    lifecycle(state = "active", since = "2026-11-19"),
    human_only = true,
    web = Available,
)]
pub fn script_deprecate(ctx: &mut LifeCtx<'_>, params: ScriptLifeParams) -> Result<LifeResult, LifeError> {
    run(ctx, Verb::Deprecate, ArtifactKind::Script, &params.script, &params.expected_digest, params.note.as_deref())
}

fid_commands![
    program_register,
    program_unregister,
    program_release,
    program_deprecate,
    script_register,
    script_unregister,
    script_release,
    script_deprecate,
];

/// The declared definition of a command.
pub(crate) fn def_for(verb: Verb, kind: ArtifactKind) -> CommandDef {
    match (kind, verb) {
        (ArtifactKind::Program, Verb::Register) => __fid_command_program_register().def,
        (ArtifactKind::Program, Verb::Unregister) => __fid_command_program_unregister().def,
        (ArtifactKind::Program, Verb::Release) => __fid_command_program_release().def,
        (ArtifactKind::Program, Verb::Deprecate) => __fid_command_program_deprecate().def,
        (ArtifactKind::Script, Verb::Register) => __fid_command_script_register().def,
        (ArtifactKind::Script, Verb::Unregister) => __fid_command_script_unregister().def,
        (ArtifactKind::Script, Verb::Release) => __fid_command_script_release().def,
        (ArtifactKind::Script, Verb::Deprecate) => __fid_command_script_deprecate().def,
    }
}

// ------------------------------------------------------------------ parameter readers

fn call_from(kind: ArtifactKind, verb: Verb, o: &BTreeMap<String, CValue>) -> Result<LifeCall, LifeError> {
    let id_key = kind.as_str();
    if o.keys().any(|k| k != id_key && k != "expected_digest" && k != "note") {
        return Err(LifeError::bad("unknown parameter"));
    }
    let s = |k: &str| match o.get(k) {
        None => Ok(None),
        Some(CValue::Str(s)) => Ok(Some(s.clone())),
        Some(_) => Err(LifeError::bad("parameter is not a string")),
    };
    Ok(LifeCall {
        verb,
        kind,
        artifact: s(id_key)?.ok_or(LifeError::bad("missing artifact parameter"))?,
        expected_digest: s("expected_digest")?.ok_or(LifeError::bad("missing expected_digest"))?,
        note: s("note")?,
    })
}

/// Reads MCP parameters for `tool` (`program_register`, ..., `script_deprecate`) strictly: at
/// most [`MAX_PARAMS_BYTES`] bytes, duplicate keys refused, and any key the schema does not
/// declare (a `kind`, a `principal`, a `branch`, anything) is `bad_params`.
pub fn mcp_params(tool: &str, raw: &[u8]) -> Result<LifeCall, LifeError> {
    let (kind, verb) = tool
        .split_once('_')
        .and_then(|(k, v)| {
            let kind = [ArtifactKind::Program, ArtifactKind::Script].into_iter().find(|x| x.as_str() == k)?;
            Some((kind, Verb::parse(v)?))
        })
        .ok_or(LifeError::bad("unknown tool"))?;
    let v = parse_with_limits(raw, MAX_PARAMS_BYTES, MAX_PARAMS_DEPTH)
        .map_err(|_| LifeError::bad("params are not JSON within the limits"))?;
    let CValue::Obj(o) = v else { return Err(LifeError::bad("params is not an object")) };
    call_from(kind, verb, &o)
}

/// Reads `fid <args>`: `program|script register|unregister|release|deprecate --program <P> |
/// --script <X> --expected-digest <D> [--note <text>] [--json]`. Unknown, repeated or
/// value-less options and stray words are `bad_params`.
pub fn cli_params(args: &[&str]) -> Result<LifeCall, LifeError> {
    if args.len() > MAX_CLI_ARGS {
        return Err(LifeError::bad("too many arguments"));
    }
    let [group, sub, rest @ ..] = args else { return Err(LifeError::bad("missing subcommand")) };
    let kind = [ArtifactKind::Program, ArtifactKind::Script]
        .into_iter()
        .find(|k| k.as_str() == *group)
        .ok_or(LifeError::bad("unknown command group"))?;
    let verb = Verb::parse(sub).ok_or(LifeError::bad("unknown subcommand"))?;
    let id_flag = match kind {
        ArtifactKind::Program => "--program",
        ArtifactKind::Script => "--script",
    };
    let mut o = BTreeMap::new();
    let mut json = false;
    let mut it = rest.iter();
    while let Some(arg) = it.next() {
        let key = match *arg {
            "--json" if !json => {
                json = true;
                continue;
            }
            "--expected-digest" => "expected_digest",
            "--note" => "note",
            a if a == id_flag => kind.as_str(),
            _ => return Err(LifeError::bad("unknown or repeated option")),
        };
        let value = it.next().ok_or(LifeError::bad("option needs a value"))?;
        if o.insert(key.to_owned(), CValue::Str((*value).to_owned())).is_some() {
            return Err(LifeError::bad("repeated option"));
        }
    }
    call_from(kind, verb, &o)
}

// ------------------------------------------------------------------ surfaces

fn dispatch(ctx: &mut LifeCtx<'_>, call: LifeCall) -> Result<LifeResult, LifeError> {
    let LifeCall { verb, kind, artifact, expected_digest, note } = call;
    match kind {
        ArtifactKind::Program => {
            let p = ProgramLifeParams { program: artifact, expected_digest, note };
            match verb {
                Verb::Register => program_register(ctx, p),
                Verb::Unregister => program_unregister(ctx, p),
                Verb::Release => program_release(ctx, p),
                Verb::Deprecate => program_deprecate(ctx, p),
            }
        }
        ArtifactKind::Script => {
            let p = ScriptLifeParams { script: artifact, expected_digest, note };
            match verb {
                Verb::Register => script_register(ctx, p),
                Verb::Unregister => script_unregister(ctx, p),
                Verb::Release => script_release(ctx, p),
                Verb::Deprecate => script_deprecate(ctx, p),
            }
        }
    }
}

/// The MCP tools. Parameters are read strictly first. A request-only tool (the W3 commands)
/// never runs: it files a [`LifeRequest`] naming the bus-derived requester and returns
/// [`McpReply::RequestFiled`]; nothing is journaled and no state changes. An exposed tool
/// (the W2 commands) runs its handler with the caller's principal and branch, and so is
/// subject to the class and branch checks.
pub fn invoke_mcp(ctx: &mut LifeCtx<'_>, tool: &str, raw: &[u8]) -> Result<McpReply, LifeError> {
    let call = mcp_params(tool, raw)?;
    let def = def_for(call.verb, call.kind);
    if def.effective_projection() != Projection::Exposed {
        let params = to_canonical_bytes(&call.to_value())
            .map_err(|_| LifeError::Internal { reason: "the request cannot be serialised" })?;
        let request = LifeRequest { command: call.command(), params, requester: ctx.principal.reference() };
        let request_id =
            ctx.requests.file(&request).map_err(|reason| LifeError::RequestFailed { reason: echo(&reason) })?;
        if request_id.is_empty() {
            return Err(LifeError::RequestFailed { reason: "the request queue returned no ID".to_owned() });
        }
        return Ok(McpReply::RequestFiled { request_id });
    }
    dispatch(ctx, call).map(|r| McpReply::Committed(r.to_canonical_bytes()))
}

/// Runs `fid <args>` (see [`cli_params`]). Output is the canonical result JSON either way.
pub fn invoke_cli(ctx: &mut LifeCtx<'_>, args: &[&str]) -> Result<Vec<u8>, LifeError> {
    let call = cli_params(args)?;
    dispatch(ctx, call).map(|r| r.to_canonical_bytes())
}
