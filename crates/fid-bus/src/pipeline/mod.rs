//! `Bus::execute`: stage order, the all-or-nothing commit, reads to the Activity Log, dry run
//! and the handler-only path of recompute. Spec node `CORE.BUS.PIPE.STAGE`.
//!
//! [`Bus::execute`] is a straight-line function with an early return on the first error. A
//! call runs these stages, each at most once and in this order (REQ-001):
//!
//! 1. **resolve**: the principal is derived from the channel (never from the parameters),
//!    then [`CommandTable::resolve_id`];
//! 2. **validate**: [`Resolved::validate_value`] against the call's [`StateView`];
//! 3. **authorise**: [`check_raw_paths`] (CORE.BUS.POL.FS REQ-001), then
//!    [`authorise`] (projection, approval and class checks): run, run as an approval, or
//!    file a pending request;
//! 4. **precondition**: [`Resolved::check_preconditions`];
//! 5. **route** (commit classes only): the branch from the injected [`Router`], then the
//!    guards: `expected_head` (`conflict/expected_head_mismatch`) and the call's view, which
//!    must be at the branch head (`conflict/head_moved`);
//! 6. **handler** and 7. **effects**: the handler of the command and the effects check
//!    against its definition (CORE.BUS.PIPE.HANDLER), as two stages;
//! 8. **commit** (commit classes only): one entry, all or nothing (below).
//!
//! A filed pending request (REQ-003) runs resolve, validate, authorise, route and commit: the
//! commit holds only the `request.file_request@1` entry of [`Filing::commit`], and the handler
//! is never called. An R0 or R1 command (REQ-006) skips route and commit: its call record is
//! handed to the Activity Log writer ([`Streams::complete_call`], CORE.JRN.STREAM.ALOG), which
//! stores it only while a recording session is active or for a Command Program step, and
//! nothing is written to the Record Journal. With `dry_run` (REQ-008) the call stops after the
//! effects check and returns the events it would commit; a call that would file a request
//! returns [`CommandResult::WouldFile`] after authorise.
//!
//! **Commit (REQ-002, REQ-CORE.RPL.RECON.FOLD-008 (d)).** Every commit of one [`Bus`] runs
//! under its commit lock:
//!
//! 1. read the stream tail, then the branch head; the head must still be the one the call
//!    was decided against (the view's head), else `conflict/head_moved`;
//! 2. build the entry (and any [`LeadingEntries`]) with fresh salts at the next sequence
//!    numbers, so its hash is known before it is appended;
//! 3. run every watching [`CommitHook::precommit`] on the built entry; a refusal aborts;
//! 4. append with `expected_prev` set to the tail read in step 1 and the salts of step 2, so
//!    the appended entry is byte-identical to the one the hooks judged, and an append that
//!    landed meanwhile (from anywhere) makes it fail with `conflict/head_moved`; the returned
//!    hash must equal the built one;
//! 5. fold the entry into every watching hook ([`CommitHook::fold`]).
//!
//! Reading the tail before the branch head and appending only on that tail makes steps 1 to
//! 4 a compare-and-swap: nothing can land between the check and the append. A failure in
//! steps 1 to 4 appends nothing (REQ-002; the idempotency table is CORE.BUS.ROUTE's and is not
//! touched here). A failure after the append (hash mismatch, fold refusal) poisons the bus:
//! every later commit is `internal/invariant_violated`, since the folded state no longer
//! matches the journal.
//!
//! **Entry contents.** Command `id@major`, class, determinism, the seed of the call
//! ([`BusEnv::seed`]), the arguments as entered ([`Validated::entry_params`]), the events
//! (the handler's, then the `RequestApproved` of an approval), the algorithm IDs of literal
//! normalisation, the `consumed_r1` digests (REQ-007; the journal tags them `r1_result`), the
//! principal as derived, `ai_assistance` `executed` for an agent, the logical parent (the
//! branch head, none on an empty branch) and, for a command registered with
//! [`Bus::set_reason_param`], that parameter as the entry's `reason`.
//!
//! [`Bus::run_handlers_as_system`] (REQ-009) runs resolve, validate, precondition, handler and
//! effects for each listed command against one view and commits nothing; only a `system`
//! principal may call it (`policy/system_only`).
//!
//! **Not built here** (see the leaf's NODE.md): long-running commands and the job hand-off
//! (REQ-004, REQ-005: the registry has no long-running field, and `fid-jobs` depends on this
//! crate), the prepared-file protocol of REQ-011, idempotency keys (CORE.BUS.ROUTE; a call
//! that carries one is refused, fail closed), agent branches (CORE.BUS.ROUTE; [`OpenBranch`]
//! refuses agent commits) and the network permit of X1, I1 and D1 handlers.
//!
//! Every error is from the bus error catalogue; this module adds no code or reason.
//! Determinism D0: no clock or randomness of its own (time, seed and request IDs come from
//! [`BusEnv`], salts from the journal's OS salt source).

mod hooks;

pub use hooks::{CommitHook, CommitRecord, LeadingEntries, Watch};

use crate::error::{BusError, ConflictReason, Detail, InternalReason, PolicyReason, ValidationReason};
use crate::handler::{run_stage, CancelToken, CheckedEvents, Event, HandlerCtx, HandlerSet, StateView};
use crate::policy::fs::check_raw_paths;
use crate::policy::projection::{authorise, Decision, EntryOrigin, Filing, RecordContext, RequestId};
use crate::principal::{Channel, ChannelKind, JournalAppend, Principal, PrincipalKind, SessionTable};
use crate::validate::{CommandTable, Validated};
use fid_canon::{CValue, Digest};
use fid_journal::branch::{EntryOrigin as JournalOrigin, EventSource};
use fid_journal::stream::{CallOutcome, Locality, ProgramStepRef, Streams, ACTIVITY_STREAM};
use fid_journal::{
    build, AiAssistance, AppendRequest, CommandDraft, DraftBody, Entry, EntryDraft, EntryHash, EntryKind, EntryTime,
    Journal, JournalError, Manifests, OsSalts, RecordedSalts, GENESIS_PREV,
};
use fid_registry::{CommandDef, PredContext};
use fid_types::{CommandId, SideEffect};
use hooks::Hooks;
use std::collections::BTreeMap;
use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

// ------------------------------------------------------------------ stages

/// The stages of [`Bus::execute`], in their fixed order (REQ-001).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Stage {
    Resolve,
    Validate,
    Authorise,
    Precondition,
    Route,
    Handler,
    Effects,
    Commit,
}

impl Stage {
    /// Every stage, in order.
    pub const ALL: &'static [Stage] = &[
        Stage::Resolve,
        Stage::Validate,
        Stage::Authorise,
        Stage::Precondition,
        Stage::Route,
        Stage::Handler,
        Stage::Effects,
        Stage::Commit,
    ];

    /// The stable name: `resolve`, `validate`, `authorise`, `precondition`, `route`,
    /// `handler`, `effects`, `commit`.
    pub const fn as_str(self) -> &'static str {
        match self {
            Stage::Resolve => "resolve",
            Stage::Validate => "validate",
            Stage::Authorise => "authorise",
            Stage::Precondition => "precondition",
            Stage::Route => "route",
            Stage::Handler => "handler",
            Stage::Effects => "effects",
            Stage::Commit => "commit",
        }
    }
}

impl fmt::Display for Stage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Called as each stage starts (the spec's test probe, AC-1, AC-2). It can only refuse: an
/// error is returned as that stage's error and no later stage runs. The default probe does
/// nothing.
pub trait StageProbe: Send + Sync {
    /// `stage` is about to run.
    fn enter(&self, stage: Stage) -> Result<(), BusError>;
}

struct NoProbe;

impl StageProbe for NoProbe {
    fn enter(&self, _stage: Stage) -> Result<(), BusError> {
        Ok(())
    }
}

// ------------------------------------------------------------------ invocation and result

/// One command invocation, as a surface builds it. The principal is not part of it: the bus
/// derives it from the channel.
#[derive(Debug, Clone, PartialEq)]
pub struct Invocation {
    /// The command `name@major`.
    pub command: CommandId,
    /// The parameter document as entered.
    pub params: CValue,
    /// Commit only if the branch head is this (`conflict/expected_head_mismatch`).
    pub expected_head: Option<Digest>,
    /// Idempotency key (CORE.BUS.ROUTE; refused until that leaf is built).
    pub idempotency_key: Option<String>,
    /// Stop after the effects check and return the events (REQ-008).
    pub dry_run: bool,
    /// The pending request a person approves with this call (CORE.BUS.POL.PROJ).
    pub approval_of: Option<RequestId>,
    /// Digests of R1 results this W-class call consumed (REQ-007).
    pub consumed_r1: Vec<Digest>,
    /// The Command Program step this call is part of (D-031).
    pub program_step: Option<ProgramStepRef>,
}

impl Invocation {
    /// A plain call of `command` with `params`: no guard, key, dry run, approval, R1 digests
    /// or program step.
    pub fn new(command: CommandId, params: CValue) -> Invocation {
        Invocation {
            command,
            params,
            expected_head: None,
            idempotency_key: None,
            dry_run: false,
            approval_of: None,
            consumed_r1: Vec::new(),
            program_step: None,
        }
    }
}

/// What [`Bus::execute`] returns.
#[derive(Debug, Clone, PartialEq)]
pub enum CommandResult {
    /// The command ran. `result` is the handler's result value
    /// ([`crate::handler::Handler::run_with_result`]); `events` are the committed events (for
    /// a read, the checked events, written nowhere); `entry` is the Record Journal entry of a
    /// commit class, `None` for a read; `activity` is the Activity Log hand-off of a read.
    Done { result: Option<CValue>, events: Vec<Event>, entry: Option<EntryHash>, activity: Option<CallOutcome> },
    /// A pending request was filed instead of running the command (REQ-003).
    Pending { request_id: RequestId, entry: EntryHash },
    /// Dry run: the events a real run would commit (REQ-008).
    DryRun { events: Vec<Event> },
    /// Dry run of a call that would file a pending request for `command`.
    WouldFile { command: CommandId },
}

/// The per-call context: the session table the principal is derived from, the channel, the
/// state the call runs against and the precondition roots besides `params`.
///
/// `view` is the state at the head of the branch the call commits to: its
/// [`StateView::head`] is that branch's head ([`GENESIS_PREV`] for a branch with no entry),
/// and the commit stage checks the branch is still there when it commits.
pub struct Call<'a> {
    pub sessions: &'a SessionTable,
    pub channel: &'a dyn Channel,
    pub view: &'a dyn StateView,
    pub roots: &'a dyn PredContext,
}

/// One command of [`Bus::run_handlers_as_system`].
#[derive(Debug, Clone, PartialEq)]
pub struct SystemCall {
    pub command: CommandId,
    pub params: CValue,
}

// ------------------------------------------------------------------ injected parts

/// The route stage's branch choice (CORE.BUS.ROUTE implements the full rules; until then
/// [`OpenBranch`]).
pub trait Router: Send + Sync {
    /// The branch a call of `def` by `principal` on `channel` commits to (for a read: the
    /// branch its call record names).
    fn branch(&self, principal: &Principal, channel: &dyn Channel, def: &CommandDef) -> Result<String, BusError>;
    /// The branch a pending request filed by `principal` is committed to.
    fn request_branch(&self, principal: &Principal, channel: &dyn Channel) -> Result<String, BusError>;
}

/// A [`Router`] for one session's open branch: every commit and request goes to it, except
/// that an `agent` principal's commit classes are refused with `policy/agent_write_forbidden`
/// (agents commit only to their own `agent/` branches, CONSTITUTION §3; that routing is
/// CORE.BUS.ROUTE's). Fail closed until CORE.BUS.ROUTE is built.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenBranch {
    branch: String,
}

impl OpenBranch {
    /// The router of a session whose open branch is `branch`.
    pub fn new(branch: &str) -> OpenBranch {
        OpenBranch { branch: branch.to_string() }
    }
}

impl Router for OpenBranch {
    fn branch(&self, principal: &Principal, _channel: &dyn Channel, def: &CommandDef) -> Result<String, BusError> {
        if !is_read(def.side_effect) && principal.kind() == PrincipalKind::Agent {
            return Err(BusError::Policy(
                PolicyReason::AgentWriteForbidden,
                Detail::new("agent commits go to an agent branch; agent branch routing (CORE.BUS.ROUTE) is not built"),
            ));
        }
        Ok(self.branch.clone())
    }

    fn request_branch(&self, _principal: &Principal, _channel: &dyn Channel) -> Result<String, BusError> {
        Ok(self.branch.clone())
    }
}

/// The Record Journal as the commit stage uses it. [`Journal`] implements it; any error means
/// nothing was appended.
pub trait RecordJournal: Send + Sync {
    /// The stream tail `(seq, hash)`, `None` for an empty stream.
    fn tail(&self) -> Result<Option<(u64, Digest)>, BusError>;
    /// The hash of the last entry on `branch`, `None` if it has none.
    fn branch_head(&self, branch: &str) -> Result<Option<Digest>, BusError>;
    /// Appends one entry. With `expected_prev` set, a tail that moved is `conflict/head_moved`.
    fn append(&self, req: AppendRequest) -> Result<EntryHash, BusError>;
}

/// The bus error of a journal error: `head_moved` is `conflict/head_moved`, `input_limit`
/// `validation/input_limit`, a malformed draft `internal/invariant_violated` (a bus bug), and
/// every storage, chain or clock failure `internal/journal_unavailable`.
pub fn journal_error(e: &JournalError) -> BusError {
    let d = Detail::new(&e.to_string());
    match e {
        JournalError::HeadMoved { .. } => BusError::Conflict(ConflictReason::HeadMoved, d),
        JournalError::InputLimit(_) => BusError::Validation(ValidationReason::InputLimit, d),
        JournalError::Schema(_) => BusError::Internal(InternalReason::InvariantViolated, d),
        JournalError::AppendFailed(_)
        | JournalError::MissingBlob(_)
        | JournalError::TimeRegressed { .. }
        | JournalError::TimeAhead { .. }
        | JournalError::ChainInvalid(_)
        | JournalError::EmptyStream(_) => BusError::Internal(InternalReason::JournalUnavailable, d),
    }
}

impl RecordJournal for Journal {
    fn tail(&self) -> Result<Option<(u64, Digest)>, BusError> {
        Journal::tail(self).map(|t| t.map(|t| (t.seq, t.hash))).map_err(|e| journal_error(&e))
    }

    fn branch_head(&self, branch: &str) -> Result<Option<Digest>, BusError> {
        Journal::head(self, branch).map(|h| h.map(|h| h.hash)).map_err(|e| journal_error(&e))
    }

    fn append(&self, req: AppendRequest) -> Result<EntryHash, BusError> {
        Journal::append(self, req).map_err(|e| journal_error(&e))
    }
}

/// The host inputs of a call: time, seed and request IDs (CONSTITUTION §4: recorded, never
/// drawn by the bus itself).
pub trait BusEnv: Send + Sync {
    /// The call's entry time.
    fn now(&self) -> EntryTime;
    /// The call's seed, recorded in its entry.
    fn seed(&self) -> u64;
    /// A fresh UUIDv7 request ID (recorded as an observed input of the filing).
    fn request_id(&self) -> Result<RequestId, BusError>;
}

/// The parts a [`Bus`] is built from.
pub struct BusParts {
    /// The command table of the Registry Manifest in force.
    pub table: Arc<CommandTable>,
    /// The handlers, each with its definition.
    pub handlers: HandlerSet,
    pub router: Arc<dyn Router>,
    pub journal: Arc<dyn RecordJournal>,
    /// The Activity Log router and writer (reads).
    pub streams: Streams,
    pub env: Arc<dyn BusEnv>,
    /// The Record Journal stream name (`record`).
    pub stream: String,
    /// The manifest digests every entry records.
    pub manifests: Manifests,
}

// ------------------------------------------------------------------ the bus

/// The command bus of one open project.
pub struct Bus {
    table: Arc<CommandTable>,
    handlers: HandlerSet,
    router: Arc<dyn Router>,
    journal: Arc<dyn RecordJournal>,
    streams: Streams,
    env: Arc<dyn BusEnv>,
    stream: String,
    manifests: Manifests,
    hooks: Hooks,
    leading: BTreeMap<String, Arc<dyn LeadingEntries>>,
    reasons: BTreeMap<String, String>,
    probe: Arc<dyn StageProbe>,
    commit_lock: Mutex<()>,
    poisoned: AtomicBool,
}

impl fmt::Debug for Bus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Bus")
            .field("handlers", &self.handlers)
            .field("stream", &self.stream)
            .field("hooks", &self.hooks.list.len())
            .field("poisoned", &self.poisoned.load(Ordering::SeqCst))
            .finish_non_exhaustive()
    }
}

fn invariant(detail: &str) -> BusError {
    BusError::Internal(InternalReason::InvariantViolated, Detail::new(detail))
}

/// Whether `class` is a read (R0, R1): never committed to the Record Journal (D-031).
fn is_read(class: SideEffect) -> bool {
    match class {
        SideEffect::R0 | SideEffect::R1 => true,
        SideEffect::W1
        | SideEffect::W2
        | SideEffect::W3
        | SideEffect::X1
        | SideEffect::I1
        | SideEffect::D1
        | SideEffect::M1
        | SideEffect::S1
        | SideEffect::A0 => false,
    }
}

/// Where a channel's principal sits, for the Activity Log router.
fn locality(kind: ChannelKind) -> Locality {
    match kind {
        ChannelKind::ServerMcp | ChannelKind::ServerSession | ChannelKind::ServerService => Locality::External,
        ChannelKind::Gui
        | ChannelKind::Cli
        | ChannelKind::Python
        | ChannelKind::Control
        | ChannelKind::Mcp
        | ChannelKind::Service
        | ChannelKind::Device
        | ChannelKind::Internal => Locality::Local,
    }
}

fn ai_assistance(kind: PrincipalKind) -> AiAssistance {
    match kind {
        PrincipalKind::Agent => AiAssistance::Executed,
        PrincipalKind::Human
        | PrincipalKind::Script
        | PrincipalKind::Service
        | PrincipalKind::Device
        | PrincipalKind::System => AiAssistance::None,
    }
}

/// The routed branch and the head the call is decided against.
struct Target {
    branch: String,
    head: Digest,
}

/// One built entry, ready to append.
struct Built {
    draft: EntryDraft,
    entry: Entry,
}

impl Bus {
    /// A bus over `parts`, with no hooks, leading entries or reason parameters, and no probe.
    pub fn new(parts: BusParts) -> Bus {
        Bus {
            table: parts.table,
            handlers: parts.handlers,
            router: parts.router,
            journal: parts.journal,
            streams: parts.streams,
            env: parts.env,
            stream: parts.stream,
            manifests: parts.manifests,
            hooks: Hooks::default(),
            leading: BTreeMap::new(),
            reasons: BTreeMap::new(),
            probe: Arc::new(NoProbe),
            commit_lock: Mutex::new(()),
            poisoned: AtomicBool::new(false),
        }
    }

    /// Registers a commit hook; hooks run in registration order.
    pub fn register_hook(&mut self, hook: Arc<dyn CommitHook>) {
        self.hooks.list.push(hook);
    }

    /// Registers the leading entries of `command`. A second registration for the same command
    /// is `internal/invariant_violated` and changes nothing.
    pub fn register_leading(&mut self, command: &CommandId, leading: Arc<dyn LeadingEntries>) -> Result<(), BusError> {
        if self.leading.contains_key(command.as_str()) {
            return Err(invariant(&format!("leading entries of {command} are already registered")));
        }
        self.leading.insert(command.as_str().to_string(), leading);
        Ok(())
    }

    /// Records the top-level string parameter `field` of `command` as its entries' `reason`
    /// (CORE.JRN.ENTRY `reason_commit`; CORE.JRN.UNDO.SUPER records `reason` so). A call
    /// without the parameter has no reason; a non-string value is `validation/schema`. A
    /// second registration for the same command is `internal/invariant_violated`.
    pub fn set_reason_param(&mut self, command: &CommandId, field: &str) -> Result<(), BusError> {
        if self.reasons.contains_key(command.as_str()) {
            return Err(invariant(&format!("the reason parameter of {command} is already set")));
        }
        self.reasons.insert(command.as_str().to_string(), field.to_string());
        Ok(())
    }

    /// Sets the stage probe (tests).
    pub fn set_probe(&mut self, probe: Arc<dyn StageProbe>) {
        self.probe = probe;
    }

    /// Whether a failure after an append left the bus poisoned (module docs).
    pub fn is_poisoned(&self) -> bool {
        self.poisoned.load(Ordering::SeqCst)
    }

    fn enter(&self, stage: Stage) -> Result<(), BusError> {
        self.probe.enter(stage)
    }

    /// Runs one invocation (module docs). Any error returns at once and changes nothing.
    pub fn execute(&self, call: &Call<'_>, inv: &Invocation) -> Result<CommandResult, BusError> {
        // 1. resolve
        self.enter(Stage::Resolve)?;
        let derived = call.sessions.derive_principal(call.channel)?;
        let resolved = self.table.resolve_id(&inv.command, &derived)?;
        let def = resolved.def();
        // 2. validate
        self.enter(Stage::Validate)?;
        let validated = resolved.validate_value(&inv.params, call.view).map_err(BusError::from)?;
        // 3. authorise
        self.enter(Stage::Authorise)?;
        check_raw_paths(&derived, def, &inv.params)?;
        let authorised =
            authorise(call.sessions, call.channel, def, validated.params(), inv.approval_of.as_ref(), call.view)?;
        let (principal, decision) = authorised.into_parts();
        let approval = match decision {
            Decision::File(filing) => return self.file(call, inv, &principal, &filing),
            Decision::RunApproved(a) => Some(a.event()?),
            Decision::Run => None,
        };
        // 4. precondition
        self.enter(Stage::Precondition)?;
        resolved.check_preconditions(validated.params(), call.roots)?;
        // 5. route
        let read = is_read(def.side_effect);
        let target = if read {
            None
        } else {
            self.enter(Stage::Route)?;
            Some(self.route(call, inv, &principal, def)?)
        };
        // 6. handler
        self.enter(Stage::Handler)?;
        let seed = self.env.seed();
        let ctx = HandlerCtx::derive(call.sessions, call.channel, seed, CancelToken::never())?;
        let ran = run_stage(&self.handlers, inv.command.as_str(), call.view, validated.params(), &ctx)?;
        // 7. effects
        self.enter(Stage::Effects)?;
        let (checked, result) = ran.check(&ctx)?;
        let (_, _, mut events) = checked.into_parts();
        events.extend(approval);
        if inv.dry_run {
            return Ok(CommandResult::DryRun { events });
        }
        let Some(target) = target else {
            return self.hand_off_read(call, inv, &principal, def, &validated, seed, events, result);
        };
        // 8. commit
        self.enter(Stage::Commit)?;
        let draft = self.command_draft(
            &principal,
            def,
            &validated,
            &events,
            &target.branch,
            &self.stream,
            seed,
            inv.consumed_r1.clone(),
            inv,
        )?;
        let leading = match self.leading.get(inv.command.as_str()) {
            Some(l) => l.leading(&draft, &draft.time)?,
            None => Vec::new(),
        };
        let hash = self.commit(&target, draft, &events, leading)?;
        Ok(CommandResult::Done { result, events, entry: Some(hash), activity: None })
    }

    /// The route stage of a commit-class call.
    fn route(
        &self,
        call: &Call<'_>,
        inv: &Invocation,
        principal: &Principal,
        def: &CommandDef,
    ) -> Result<Target, BusError> {
        let branch = self.router.branch(principal, call.channel, def)?;
        self.guard(call, inv, branch)
    }

    /// The guards of the route stage: idempotency keys refused (CORE.BUS.ROUTE), then
    /// `expected_head`, then the view's head against the branch head.
    fn guard(&self, call: &Call<'_>, inv: &Invocation, branch: String) -> Result<Target, BusError> {
        if inv.idempotency_key.is_some() {
            return Err(BusError::Validation(
                ValidationReason::Schema,
                Detail::new("idempotency_key needs the idempotency table of CORE.BUS.ROUTE, which is not built"),
            ));
        }
        let head = self.journal.branch_head(&branch)?.unwrap_or(GENESIS_PREV);
        if let Some(expected) = inv.expected_head {
            if expected != head {
                return Err(BusError::Conflict(
                    ConflictReason::ExpectedHeadMismatch,
                    Detail::new(&format!("expected head {expected}, branch {branch} is at {head}")),
                ));
            }
        }
        let seen = call.view.head();
        if seen != head {
            return Err(BusError::Conflict(
                ConflictReason::HeadMoved,
                Detail::new(&format!("the call's state is at {seen}, branch {branch} is at {head}")),
            ));
        }
        Ok(Target { branch, head })
    }

    /// REQ-003: a pending request instead of the command.
    fn file(
        &self,
        call: &Call<'_>,
        inv: &Invocation,
        principal: &Principal,
        filing: &Filing,
    ) -> Result<CommandResult, BusError> {
        if inv.dry_run {
            return Ok(CommandResult::WouldFile { command: filing.command().clone() });
        }
        self.enter(Stage::Route)?;
        let branch = self.router.request_branch(principal, call.channel)?;
        let target = self.guard(call, inv, branch)?;
        self.enter(Stage::Commit)?;
        let id = self.env.request_id()?;
        let ctx =
            RecordContext { stream: self.stream.clone(), branch: target.branch.clone(), manifests: self.manifests };
        let mut sink = FilingSink { bus: self, target: &target, entry: None };
        filing.commit(id, call.view, self.env.now(), &ctx, &mut sink)?;
        let entry = sink.entry.ok_or_else(|| invariant("the filing appended no entry"))?;
        Ok(CommandResult::Pending { request_id: id, entry })
    }

    /// REQ-006: the call record of a read goes to the Activity Log writer only.
    #[allow(clippy::too_many_arguments)]
    fn hand_off_read(
        &self,
        call: &Call<'_>,
        inv: &Invocation,
        principal: &Principal,
        def: &CommandDef,
        validated: &Validated,
        seed: u64,
        events: Vec<Event>,
        result: Option<CValue>,
    ) -> Result<CommandResult, BusError> {
        let branch = self.router.branch(principal, call.channel, def)?;
        let draft =
            self.command_draft(principal, def, validated, &events, &branch, ACTIVITY_STREAM, seed, Vec::new(), inv)?;
        let outcome = self
            .streams
            .complete_call(locality(call.channel.kind()), inv.program_step.as_ref(), &draft)
            .map_err(|e| invariant(&format!("call record refused by the stream router: {e}")))?;
        Ok(CommandResult::Done { result, events, entry: None, activity: Some(outcome) })
    }

    /// The command entry draft of a call (module docs, "Entry contents"). The commit stage
    /// sets the logical parent.
    #[allow(clippy::too_many_arguments)]
    fn command_draft(
        &self,
        principal: &Principal,
        def: &CommandDef,
        validated: &Validated,
        events: &[Event],
        branch: &str,
        stream: &str,
        seed: u64,
        consumed_r1: Vec<Digest>,
        inv: &Invocation,
    ) -> Result<EntryDraft, BusError> {
        let args = validated.entry_params().clone();
        let reason = match self.reasons.get(inv.command.as_str()) {
            None => None,
            Some(field) => match &args {
                CValue::Obj(o) => match o.get(field) {
                    None => None,
                    Some(CValue::Str(s)) => Some(s.clone()),
                    Some(_) => {
                        return Err(BusError::Validation(
                            ValidationReason::Schema,
                            Detail::new(&format!("reason parameter {field} is not a string")),
                        ))
                    }
                },
                _ => None,
            },
        };
        Ok(EntryDraft {
            stream: stream.to_string(),
            branch: branch.to_string(),
            logical_parent: None,
            time: self.env.now(),
            principal: principal.to_draft(),
            ai_assistance: ai_assistance(principal.kind()),
            manifests: self.manifests,
            reason,
            human_assertion: None,
            body: DraftBody::Command(CommandDraft {
                id: def.id.clone(),
                major: def.major,
                class: def.side_effect,
                determinism: def.determinism,
                seed,
                args,
                events: events.iter().map(Event::to_value).collect(),
                inputs: Vec::new(),
                consumed_r1,
                outputs: Vec::new(),
                algorithms: validated.algorithms(),
            }),
        })
    }

    /// The record a hook sees of `built`.
    fn record<'b>(&self, built: &'b Built, decided_at: Digest) -> Result<CommitRecord<'b>, BusError> {
        let d = &built.draft;
        let (origin, journal, args) = match &d.body {
            DraftBody::Command(c) => (
                Some(EntryOrigin::from_draft(d)?),
                Some(JournalOrigin {
                    kind: d.principal.kind,
                    command: c.id.clone(),
                    major: c.major,
                    class: c.class,
                    branch: d.branch.clone(),
                    source: EventSource::Handler,
                    human_assertion: built.entry.header.human_assertion.clone(),
                }),
                Some(&c.args),
            ),
            DraftBody::SessionHeader(_) => (None, None, None),
            DraftBody::PrincipalUpgrade(_) | DraftBody::HeadMove(_) => {
                return Err(invariant(&format!("the commit stage does not commit {} entries", d.kind())))
            }
        };
        Ok(CommitRecord {
            origin,
            journal,
            decided_at,
            header: &built.entry.header,
            payload: &built.entry.payload,
            args,
        })
    }

    /// The commit (module docs, steps 1 to 5).
    fn commit(
        &self,
        target: &Target,
        mut draft: EntryDraft,
        events: &[Event],
        leading: Vec<EntryDraft>,
    ) -> Result<EntryHash, BusError> {
        let _lock = self.commit_lock.lock().unwrap_or_else(PoisonError::into_inner);
        if self.is_poisoned() {
            return Err(invariant("the bus is poisoned by an earlier failure after an append; reopen the project"));
        }
        // 1. tail, then branch head.
        let (tail_seq, tail_hash) = self.journal.tail()?.unwrap_or((0, GENESIS_PREV));
        let head = self.journal.branch_head(&target.branch)?.unwrap_or(GENESIS_PREV);
        if head != target.head {
            return Err(BusError::Conflict(
                ConflictReason::HeadMoved,
                Detail::new(&format!("branch {} moved from {} to {head}", target.branch, target.head)),
            ));
        }
        draft.branch = target.branch.clone();
        draft.logical_parent = (head != GENESIS_PREV).then_some(head);
        // 2. build.
        let mut seq = tail_seq;
        let mut prev = tail_hash;
        let mut built = Vec::with_capacity(leading.len() + 1);
        for d in leading {
            if d.kind() != EntryKind::SessionHeader || d.stream != self.stream {
                return Err(invariant("leading entries must be session headers of the record stream"));
            }
            built.push(self.build(d, &mut seq, &mut prev)?);
        }
        built.push(self.build(draft, &mut seq, &mut prev)?);
        let last = built.len() - 1;
        let records = built.iter().map(|b| self.record(b, target.head)).collect::<Result<Vec<_>, BusError>>()?;
        let none: &[Event] = &[];
        let events_of = |i: usize| if i == last { events } else { none };
        // 3. precommit.
        for (i, r) in records.iter().enumerate() {
            self.hooks.precommit(r, events_of(i))?;
        }
        // 4. append on the tail of step 1.
        let mut expected = tail_hash;
        for b in &built {
            let req = AppendRequest {
                draft: b.draft.clone(),
                blobs: Vec::new(),
                expected_prev: Some(expected),
                salts: Some(RecordedSalts(b.entry.payload.salts.clone())),
            };
            let hash = self.journal.append(req)?;
            if hash != b.entry.header.hash {
                self.poisoned.store(true, Ordering::SeqCst);
                return Err(invariant(&format!(
                    "the journal appended {hash}, not the entry {} the hooks judged",
                    b.entry.header.hash
                )));
            }
            expected = hash;
        }
        // 5. fold.
        for (i, r) in records.iter().enumerate() {
            if let Err(e) = self.hooks.fold(r, events_of(i)) {
                self.poisoned.store(true, Ordering::SeqCst);
                return Err(e);
            }
        }
        Ok(expected)
    }

    fn build(&self, draft: EntryDraft, seq: &mut u64, prev: &mut Digest) -> Result<Built, BusError> {
        *seq = seq.checked_add(1).ok_or_else(|| invariant("sequence overflow"))?;
        let entry = build(&draft, *seq, *prev, &mut OsSalts).map_err(|e| match e {
            fid_journal::EntryError::RngUnavailable => {
                BusError::Internal(InternalReason::JournalUnavailable, Detail::new(&e.to_string()))
            }
            other => invariant(&format!("entry does not build: {other}")),
        })?;
        *prev = entry.header.hash;
        Ok(Built { draft, entry })
    }

    /// REQ-009: validate, precondition, handler and effects check of each command against
    /// `view`, as the `system` principal, committing nothing. Any other principal is
    /// `policy/system_only`; the first refusal of any command is returned.
    pub fn run_handlers_as_system(
        &self,
        sessions: &SessionTable,
        channel: &dyn Channel,
        calls: &[SystemCall],
        view: &dyn StateView,
        roots: &dyn PredContext,
    ) -> Result<Vec<CheckedEvents>, BusError> {
        let principal = sessions.derive_principal(channel)?;
        if principal.kind() != PrincipalKind::System {
            return Err(BusError::Policy(
                PolicyReason::SystemOnly,
                Detail::new(&format!(
                    "run_handlers_as_system is for the system principal; caller is {}",
                    principal.kind().as_str()
                )),
            ));
        }
        let ctx = HandlerCtx::derive(sessions, channel, self.env.seed(), CancelToken::never())?;
        let mut out = Vec::with_capacity(calls.len());
        for c in calls {
            let resolved = self.table.resolve_id(&c.command, &principal)?;
            let validated = resolved.validate_value(&c.params, view).map_err(BusError::from)?;
            resolved.check_preconditions(validated.params(), roots)?;
            let ran = run_stage(&self.handlers, c.command.as_str(), view, validated.params(), &ctx)?;
            out.push(ran.check(&ctx)?.0);
        }
        Ok(out)
    }
}

/// Commits the one entry of a [`Filing`] through the commit stage.
struct FilingSink<'b> {
    bus: &'b Bus,
    target: &'b Target,
    entry: Option<EntryHash>,
}

impl JournalAppend for FilingSink<'_> {
    fn append(&mut self, draft: &EntryDraft) -> Result<(), BusError> {
        if self.entry.is_some() {
            return Err(invariant("a filing appends exactly one entry"));
        }
        let DraftBody::Command(c) = &draft.body else { return Err(invariant("a filing is a command entry")) };
        let events = c.events.iter().map(|v| Event::from_value(v.clone())).collect::<Result<Vec<_>, BusError>>()?;
        let hash = self.bus.commit(self.target, draft.clone(), &events, Vec::new())?;
        self.entry = Some(hash);
        Ok(())
    }
}
