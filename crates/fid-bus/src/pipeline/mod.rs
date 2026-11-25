//! `Bus::execute`: stage order, the all-or-nothing commit, reads to the Activity Log, dry run
//! and the handler-only path of recompute. Spec node `CORE.BUS.PIPE.STAGE`.
//!
//! [`Bus::execute`] is a straight-line function with an early return on the first error. A
//! call runs these stages, each at most once and in this order (REQ-001):
//!
//! 1. **resolve**: the principal is derived from the channel (never from the parameters),
//!    then [`CommandTable::resolve_id`];
//! 2. **validate**: [`Resolved::validate_value`] against the call's [`StateView`]; a read
//!    (R0, R1) that carries `expected_head`, `idempotency_key` or `consumed_r1` is
//!    `validation/schema`, since a read has no branch head to guard and no entry to carry
//!    them;
//! 3. **authorise**: [`check_raw_paths`] (CORE.BUS.POL.FS REQ-001), then
//!    [`authorise`] (projection, approval and class checks): run, run as an approval, or
//!    file a pending request;
//! 4. **precondition**: [`Resolved::check_preconditions`];
//! 5. **route** (commit classes only): the branch from the injected [`Router`]
//!    ([`crate::route::SessionRouter`] in production); then the bus's own check, independent
//!    of the router, that the name is a valid branch name (`validation/bad_branch_name`) and
//!    that an `agent` principal's branch is one an agent owns, in the `agent/` namespace
//!    (`policy/agent_write_forbidden`); then the branch's FIFO writer ticket
//!    ([`BranchLocks`], CORE.BUS.ROUTE REQ-005), held until the call returns; then, under it,
//!    the guards: an idempotency key is refused (`validation/schema`, until CORE.BUS.ROUTE
//!    part 2), `expected_head` (`conflict/expected_head_mismatch`, naming the current head)
//!    and the call's view, which must be at the branch head (`conflict/head_moved`);
//! 6. **handler** and 7. **effects**: the handler of the command and the effects check
//!    against its definition (CORE.BUS.PIPE.HANDLER), as two stages;
//! 8. **commit** (commit classes only): one entry, all or nothing (below).
//!
//! A filed pending request (REQ-003) runs resolve, validate, authorise, route and commit: the
//! route stage applies the same agent-branch check and guards to the request branch, the
//! commit holds only the `request.file_request@1` entry of [`Filing::commit`], and the handler
//! is never called. An R0 or R1 command (REQ-006) skips route and commit: its call record is
//! handed to the Activity Log writer ([`Streams::complete_call`], CORE.JRN.STREAM.ALOG), which
//! stores it only while a recording session is active or for a Command Program step, and
//! nothing is written to the Record Journal. With `dry_run` (REQ-008) the call stops after the
//! effects check and returns the events it would commit; a call that would file a request
//! returns [`CommandResult::WouldFile`] after the route stage, so a dry run reports the same
//! route refusal or conflict a real filing would.
//!
//! **Locks (CORE.BUS.ROUTE).** A commit-class call holds its branch's ticket from the route
//! stage to its return, so one branch has one writer at a time, served in arrival order, and
//! the view a call was decided against cannot be overtaken by another bus commit to the same
//! branch. Inside that, every commit of one [`Bus`] (any branch) runs under the bus-wide
//! commit lock, because the hooks are bus-wide. Order: branch ticket, then commit lock, never
//! the reverse; one ticket per call (`crate::route` module docs).
//!
//! **Commit (REQ-002, REQ-CORE.RPL.RECON.FOLD-008 (d)).** Every commit of one [`Bus`] runs
//! under its commit lock:
//!
//! 1. read the stream tail, then the branch head; the head must still be the one the call
//!    was decided against (the view's head), else `conflict/head_moved`; then take the entry
//!    time ([`BusEnv::now`]), so entry times follow commit order;
//! 2. build the entry with fresh salts at the next sequence number, so its hash is known
//!    before it is appended;
//! 3. stage every watching hook and run its precommit on the built entry
//!    ([`CommitHook::stage`], [`StagedFold::precommit`]); a refusal aborts;
//! 4. append with `expected_prev` set to the tail read in step 1 and the salts of step 2, so
//!    the appended entry is byte-identical to the one the hooks judged, and an append that
//!    landed meanwhile (from anywhere) makes it fail with `conflict/head_moved`; the returned
//!    hash must equal the built one;
//! 5. install every staged hook state ([`StagedFold::install`]).
//!
//! Reading the tail before the branch head and appending only on that tail makes steps 1 to
//! 4 a compare-and-swap: nothing can land between the check and the append. A failure in
//! steps 1 to 3, or an append refused with a known outcome ([`AppendOutcome::NotAppended`]:
//! a head that moved, a clock refusal, a read-only store), appends nothing and leaves the bus
//! usable (REQ-002; the idempotency table is CORE.BUS.ROUTE's and is not touched here). The
//! bus is poisoned, so every later commit, commit-class dry run and filing is
//! `internal/invariant_violated` until the project is reopened, when the folded state may no
//! longer match the journal: an append whose outcome is unknown ([`AppendOutcome::Unknown`]:
//! a failed or interrupted COMMIT, a chain that no longer verifies), a hash
//! mismatch, an install refusal, or a panic while the commit lock was held (a panicking hook
//! poisons the lock). Reopening the project rebuilds every hook from the journal; a fold
//! refusal during that rebuild refuses to open the project (the rebuild entry point is
//! deferred, NODE.md).
//!
//! **Entry contents.** Command `id@major`, class, determinism, the seed of the call
//! ([`BusEnv::seed`]), the arguments as entered ([`Validated::entry_params`]), the events
//! (the handler's, then the `RequestApproved` of an approval), the algorithm IDs of literal
//! normalisation, the `consumed_r1` digests (REQ-007; the journal tags them `r1_result`), the
//! principal as derived, `ai_assistance` `executed` for an agent, the logical parent (the
//! branch head, none on an empty branch), the entry time taken under the commit lock and,
//! for a command registered with [`Bus::set_reason_param`], that parameter as the entry's
//! `reason`. The stage writes no `human_assertion` and no `promoted_from`.
//!
//! [`Bus::run_handlers_as_system`] (REQ-009) runs resolve, validate, precondition, handler and
//! effects for each listed command against one view and commits nothing; only a `system`
//! principal may call it (`policy/system_only`).
//!
//! **Not built here** (see the leaf's NODE.md): long-running commands and the job hand-off
//! (REQ-004, REQ-005: the registry has no long-running field, and `fid-jobs` depends on this
//! crate), the prepared-file protocol of REQ-011, idempotency keys (CORE.BUS.ROUTE part 2; a
//! call that carries one is refused, fail closed), agent branch routing (CORE.BUS.ROUTE part
//! 2; [`crate::route::SessionRouter`] refuses agent commits, and the bus refuses an agent
//! request on a branch that is not an `agent/` branch), the network permit of X1, I1 and D1 handlers, and
//! rebuilding hooks from the journal on reopen. Appends made outside the bus (not through
//! [`Bus::execute`]) are not seen by the hooks.
//!
//! Every error is from the bus error catalogue; this module adds no code or reason.
//! Determinism D0: no clock or randomness of its own (time, seed and request IDs come from
//! [`BusEnv`], salts from the journal's OS salt source).

mod hooks;

pub use hooks::{CommitHook, CommitRecord, StagedFold, Watch};

use crate::error::{
    BusError, ConflictReason, Detail, InternalReason, PolicyReason, PreconditionReason, ValidationReason,
};
use crate::handler::{run_stage, CancelToken, CheckedEvents, Event, HandlerCtx, HandlerSet, StateView};
use crate::policy::fs::check_raw_paths;
use crate::policy::projection::{authorise, Decision, EntryOrigin, Filing, RecordContext, RequestId};
use crate::principal::{Channel, ChannelKind, JournalAppend, Principal, PrincipalKind, SessionTable};
use crate::route::{check_expected_head, BranchLocks, BranchTicket};
use crate::validate::{CommandTable, Validated};
use fid_canon::{CValue, Digest};
use fid_journal::branch::{BranchName, EntryOrigin as JournalOrigin, EventSource, Owner, AGENT_PREFIX};
use fid_journal::stream::{CallOutcome, Locality, ProgramStepRef, Streams, ACTIVITY_STREAM};
use fid_journal::{
    build, AiAssistance, AppendRequest, CommandDraft, DraftBody, Entry, EntryDraft, EntryHash, EntryTime, Journal,
    JournalError, Manifests, OsSalts, RecordedSalts, GENESIS_PREV,
};
use fid_registry::{CommandDef, PredContext};
use fid_types::{CommandId, SideEffect};
use hooks::Hooks;
use std::cell::Cell;
use std::collections::BTreeMap;
use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

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
    /// Commit only if the branch head is this (`conflict/expected_head_mismatch`). Checked at
    /// the route stage, also for a filing and a dry run; a read carrying it is
    /// `validation/schema`.
    pub expected_head: Option<Digest>,
    /// Idempotency key (CORE.BUS.ROUTE; refused with `validation/schema` until that leaf is
    /// built, on every call).
    pub idempotency_key: Option<String>,
    /// Stop after the effects check and return the events (REQ-008).
    pub dry_run: bool,
    /// The pending request a person approves with this call (CORE.BUS.POL.PROJ).
    pub approval_of: Option<RequestId>,
    /// Digests of R1 results this W-class call consumed (REQ-007); a read carrying any is
    /// `validation/schema`. A filing does not carry them: the approved call that runs the
    /// command does.
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

/// The route stage's branch choice; [`crate::route::SessionRouter`] implements CORE.BUS.ROUTE's
/// rules. The bus does not trust it on agent writes: whatever a router returns, an `agent`
/// principal's commit or request branch must start with `agent/`, else
/// `policy/agent_write_forbidden`.
pub trait Router: Send + Sync {
    /// The branch a call of `def` by `principal` on `channel` commits to (for a read: the
    /// branch its call record names).
    fn branch(&self, principal: &Principal, channel: &dyn Channel, def: &CommandDef) -> Result<String, BusError>;
    /// The branch a pending request filed by `principal` is committed to.
    fn request_branch(&self, principal: &Principal, channel: &dyn Channel) -> Result<String, BusError>;
}

/// The Record Journal as the commit stage uses it. [`Journal`] implements it.
pub trait RecordJournal: Send + Sync {
    /// The stream tail `(seq, hash)`, `None` for an empty stream.
    fn tail(&self) -> Result<Option<(u64, Digest)>, BusError>;
    /// The hash of the last entry on `branch`, `None` if it has none.
    fn branch_head(&self, branch: &str) -> Result<Option<Digest>, BusError>;
    /// Appends one entry. With `expected_prev` set, a tail that moved is `conflict/head_moved`.
    /// A failure says whether its outcome is known ([`AppendFailure`]); an implementation
    /// reports every failure whose outcome it does not know as [`AppendOutcome::Unknown`].
    fn append(&self, req: AppendRequest) -> Result<EntryHash, AppendFailure>;
}

/// What a failed append left behind, decided where the journal error is still in hand (the
/// [`RecordJournal`] boundary).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppendOutcome {
    /// A refusal: nothing was appended. The bus stays usable.
    NotAppended,
    /// The entry may have landed (a failed or interrupted COMMIT, a chain that no longer
    /// verifies). The commit stage poisons the bus.
    Unknown,
}

/// A failed [`RecordJournal::append`]: the bus error for the caller and whether anything may
/// have been appended.
#[derive(Debug, Clone, PartialEq)]
pub struct AppendFailure {
    pub error: BusError,
    pub outcome: AppendOutcome,
}

impl AppendFailure {
    /// The failure of a journal append error: [`journal_error`] for the bus error, and
    /// [`append_outcome`] for the outcome.
    pub fn from_journal(e: &JournalError) -> AppendFailure {
        AppendFailure { error: journal_error(e), outcome: append_outcome(e) }
    }
}

/// Whether nothing was appended when `Journal::append` returned `e`. Only `append_failed` (a
/// write or COMMIT that failed, including after the COMMIT, and every append of a journal
/// poisoned by one) and `chain_invalid` have an unknown outcome; every other error is a
/// refusal checked before anything is written.
pub fn append_outcome(e: &JournalError) -> AppendOutcome {
    match e {
        JournalError::AppendFailed(_) | JournalError::ChainInvalid(_) => AppendOutcome::Unknown,
        JournalError::HeadMoved { .. }
        | JournalError::InputLimit(_)
        | JournalError::Schema(_)
        | JournalError::MissingBlob(_)
        | JournalError::TimeRegressed { .. }
        | JournalError::TimeAhead { .. }
        | JournalError::EmptyStream(_)
        | JournalError::ReadOnly(_) => AppendOutcome::NotAppended,
    }
}

/// The bus error of a journal error:
///
/// - `head_moved` is `conflict/head_moved`; `input_limit` is `validation/input_limit`; a
///   malformed draft (`schema`) is `internal/invariant_violated` (a bus bug);
/// - a clock refusal (`time_regressed`, `time_ahead`: the host clock stepped back behind the
///   chain tail, or is far ahead of it) is `precondition/false`, with the journal's detail.
///   The catalogue has no clock reason; adding one is CORE.BUS.PIPE.ERR's (a new catalogue
///   version);
/// - `read_only` (the store is not opened for writing or lost its project write lock) is
///   `precondition/project_locked`;
/// - every other storage or chain failure is `internal/journal_unavailable`.
pub fn journal_error(e: &JournalError) -> BusError {
    let d = Detail::new(&e.to_string());
    match e {
        JournalError::HeadMoved { .. } => BusError::Conflict(ConflictReason::HeadMoved, d),
        JournalError::InputLimit(_) => BusError::Validation(ValidationReason::InputLimit, d),
        JournalError::Schema(_) => BusError::Internal(InternalReason::InvariantViolated, d),
        JournalError::TimeRegressed { .. } | JournalError::TimeAhead { .. } => {
            BusError::Precondition(PreconditionReason::False, Detail::new(&format!("clock: {e}")))
        }
        JournalError::ReadOnly(_) => BusError::Precondition(PreconditionReason::ProjectLocked, d),
        JournalError::AppendFailed(_)
        | JournalError::MissingBlob(_)
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

    fn append(&self, req: AppendRequest) -> Result<EntryHash, AppendFailure> {
        Journal::append(self, req).map_err(|e| AppendFailure::from_journal(&e))
    }
}

/// The host inputs of a call: time, seed and request IDs (CONSTITUTION §4: recorded, never
/// drawn by the bus itself).
pub trait BusEnv: Send + Sync {
    /// The call's entry time. For a commit it is read under the commit lock, so entry times
    /// follow commit order.
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
    reasons: BTreeMap<String, String>,
    probe: Arc<dyn StageProbe>,
    branch_locks: BranchLocks,
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
pub(crate) fn is_read(class: SideEffect) -> bool {
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

/// The placeholder time of a commit's draft (and a filing's): the commit stage replaces it
/// with [`BusEnv::now`] under its commit lock, so no time is read before the lock.
fn unset_time() -> EntryTime {
    EntryTime { utc: String::new(), mono_ns: 0, sync: String::new() }
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

/// The bus's own branch check, independent of the [`Router`]: the routed name must be a valid
/// branch name ([`BranchName::parse`], else `validation/bad_branch_name`), and an `agent`
/// principal commits and files requests only on a branch it owns as an agent
/// ([`Owner::Agent`], the `agent/` namespace; CONSTITUTION §3). Returns the checked name.
fn agent_branch(principal: &Principal, branch: &str) -> Result<BranchName, BusError> {
    let name = BranchName::parse(branch).map_err(|e| {
        BusError::Validation(ValidationReason::BadBranchName, Detail::new(&format!("routed branch: {e}")))
    })?;
    if principal.kind() == PrincipalKind::Agent && name.owner() != Owner::Agent {
        return Err(BusError::Policy(
            PolicyReason::AgentWriteForbidden,
            Detail::new(&format!("an agent writes only to an {AGENT_PREFIX} branch; the call was routed to {branch}")),
        ));
    }
    Ok(name)
}

/// A read (R0, R1) has no branch head to guard and no entry: an envelope field that only a
/// commit can honour is refused rather than ignored.
fn read_envelope(inv: &Invocation) -> Result<(), BusError> {
    let field = if inv.expected_head.is_some() {
        "expected_head"
    } else if inv.idempotency_key.is_some() {
        "idempotency_key"
    } else if !inv.consumed_r1.is_empty() {
        "consumed_r1"
    } else {
        return Ok(());
    };
    Err(BusError::Validation(
        ValidationReason::Schema,
        Detail::new(&format!("{field} applies to commits only; {} is a read", inv.command)),
    ))
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

/// The routed branch, the head the call is decided against, and the branch's writer ticket,
/// held until the target is dropped at the end of the call (CORE.BUS.ROUTE REQ-005).
struct Target {
    branch: String,
    head: Digest,
    _ticket: BranchTicket,
}

/// One built entry, ready to append.
struct Built {
    draft: EntryDraft,
    entry: Entry,
}

impl Bus {
    /// A bus over `parts`, with no hooks or reason parameters, and no probe.
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
            reasons: BTreeMap::new(),
            probe: Arc::new(NoProbe),
            branch_locks: BranchLocks::new(),
            commit_lock: Mutex::new(()),
            poisoned: AtomicBool::new(false),
        }
    }

    /// Registers a commit hook; hooks run in registration order. A hook registered on a bus
    /// that already committed has not seen those entries: register every hook before the first
    /// call (rebuilding hooks from the journal on reopen is deferred, NODE.md).
    pub fn register_hook(&mut self, hook: Arc<dyn CommitHook>) {
        self.hooks.list.push(hook);
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

    /// Whether the bus is poisoned (module docs): a failure with an unknown or inconsistent
    /// outcome after the precommit, or a panic while the commit lock was held.
    pub fn is_poisoned(&self) -> bool {
        self.poisoned.load(Ordering::SeqCst) || self.commit_lock.is_poisoned()
    }

    /// How many calls hold or wait for `branch`'s writer ticket (CORE.BUS.ROUTE REQ-005);
    /// 0 when no call is writing it.
    pub fn branch_writers(&self, branch: &str) -> u64 {
        self.branch_locks.queued(branch)
    }

    fn poison(&self) {
        self.poisoned.store(true, Ordering::SeqCst);
    }

    /// `internal/invariant_violated` on a poisoned bus. Checked before every commit, and
    /// before a commit-class dry run and a filing's request ID, so a poisoned bus does not
    /// report what a commit would do.
    fn refuse_if_poisoned(&self) -> Result<(), BusError> {
        if self.is_poisoned() {
            return Err(invariant("the bus is poisoned by an earlier failure after its precommit; reopen the project"));
        }
        Ok(())
    }

    /// Records `stage` as the one running (for [`Bus::execute_staged`]), then calls the probe.
    fn enter(&self, at: &Cell<Option<Stage>>, stage: Stage) -> Result<(), BusError> {
        at.set(Some(stage));
        self.probe.enter(stage)
    }

    /// Runs one invocation (module docs). Any error returns at once and changes nothing.
    pub fn execute(&self, call: &Call<'_>, inv: &Invocation) -> Result<CommandResult, BusError> {
        self.run(call, inv, &Cell::new(None))
    }

    /// [`Bus::execute`], also reporting the stage an error was raised in (CORE.BUS.PORT
    /// REQ-012): the last stage entered, or `None` for an error raised outside the stages
    /// (deriving the principal from the channel, which CORE.BUS.PORT reports as `session`).
    /// The run itself is exactly [`Bus::execute`]'s.
    pub(crate) fn execute_staged(
        &self,
        call: &Call<'_>,
        inv: &Invocation,
    ) -> Result<CommandResult, (Option<Stage>, BusError)> {
        let at = Cell::new(None);
        self.run(call, inv, &at).map_err(|e| (at.get(), e))
    }

    fn run(&self, call: &Call<'_>, inv: &Invocation, at: &Cell<Option<Stage>>) -> Result<CommandResult, BusError> {
        // 1. resolve
        self.enter(at, Stage::Resolve)?;
        let derived = call.sessions.derive_principal(call.channel).inspect_err(|_| at.set(None))?;
        let resolved = self.table.resolve_id(&inv.command, &derived)?;
        let def = resolved.def();
        // 2. validate
        self.enter(at, Stage::Validate)?;
        let validated = resolved.validate_value(&inv.params, call.view).map_err(BusError::from)?;
        let read = is_read(def.side_effect);
        if read {
            read_envelope(inv)?;
        }
        // 3. authorise
        self.enter(at, Stage::Authorise)?;
        check_raw_paths(&derived, def, &inv.params)?;
        let authorised =
            authorise(call.sessions, call.channel, def, validated.params(), inv.approval_of.as_ref(), call.view)?;
        let (principal, decision) = authorised.into_parts();
        let approval = match decision {
            Decision::File(filing) => return self.file(call, inv, &principal, &filing, at),
            Decision::RunApproved(a) => Some(a.event()?),
            Decision::Run => None,
        };
        // 4. precondition
        self.enter(at, Stage::Precondition)?;
        resolved.check_preconditions(validated.params(), call.roots)?;
        // 5. route
        let target = if read {
            None
        } else {
            self.enter(at, Stage::Route)?;
            Some(self.route(call, inv, &principal, def)?)
        };
        // 6. handler
        self.enter(at, Stage::Handler)?;
        let seed = self.env.seed();
        let ctx = HandlerCtx::derive(call.sessions, call.channel, seed, CancelToken::never())?;
        let ran = run_stage(&self.handlers, inv.command.as_str(), call.view, validated.params(), &ctx)?;
        // 7. effects
        self.enter(at, Stage::Effects)?;
        let (checked, result) = ran.check(&ctx)?;
        let (_, _, mut events) = checked.into_parts();
        // The stage that added each event, recorded in the entry header (CORE.JRN.ENTRY
        // REQ-013): the handler's events, then the authorisation stage's `RequestApproved`.
        let mut sources = vec![EventSource::Handler; events.len()];
        if let Some(a) = approval {
            events.push(a);
            sources.push(EventSource::BusApproval);
        }
        if inv.dry_run {
            if target.is_some() {
                self.refuse_if_poisoned()?;
            }
            return Ok(CommandResult::DryRun { events });
        }
        let Some(target) = target else {
            return self.hand_off_read(call, inv, &principal, def, &validated, seed, events, &sources, result);
        };
        // 8. commit
        self.enter(at, Stage::Commit)?;
        let draft = self.command_draft(
            &principal,
            def,
            &validated,
            &events,
            &sources,
            &target.branch,
            &self.stream,
            seed,
            inv.consumed_r1.clone(),
            inv,
            unset_time(),
        )?;
        let hash = self.commit(&target, draft, &events)?;
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
        let name = agent_branch(principal, &branch)?;
        self.guard(call, inv, &name)
    }

    /// The branch's writer ticket (CORE.BUS.ROUTE REQ-005; taken while the call holds no
    /// other bus lock), then, under it, the guards of the route stage: idempotency keys
    /// refused (CORE.BUS.ROUTE part 2), then `expected_head` (the conflict names the current
    /// head, REQ-004), then the view's head against the branch head.
    fn guard(&self, call: &Call<'_>, inv: &Invocation, name: &BranchName) -> Result<Target, BusError> {
        if inv.idempotency_key.is_some() {
            return Err(BusError::Validation(
                ValidationReason::Schema,
                Detail::new("idempotency_key needs the idempotency table of CORE.BUS.ROUTE part 2, which is not built"),
            ));
        }
        let ticket = self.branch_locks.acquire(name)?;
        let branch = name.as_str().to_string();
        let head = self.journal.branch_head(&branch)?.unwrap_or(GENESIS_PREV);
        check_expected_head(&branch, inv.expected_head, head)?;
        let seen = call.view.head();
        if seen != head {
            return Err(BusError::Conflict(
                ConflictReason::HeadMoved,
                Detail::new(&format!("the call's state is at {seen}, branch {branch} is at {head}")),
            ));
        }
        Ok(Target { branch, head, _ticket: ticket })
    }

    /// REQ-003: a pending request instead of the command. A dry run stops after the route
    /// stage, so it reports the same route refusal or conflict a real filing would.
    fn file(
        &self,
        call: &Call<'_>,
        inv: &Invocation,
        principal: &Principal,
        filing: &Filing,
        at: &Cell<Option<Stage>>,
    ) -> Result<CommandResult, BusError> {
        self.enter(at, Stage::Route)?;
        let branch = self.router.request_branch(principal, call.channel)?;
        let name = agent_branch(principal, &branch)?;
        let target = self.guard(call, inv, &name)?;
        self.refuse_if_poisoned()?;
        if inv.dry_run {
            return Ok(CommandResult::WouldFile { command: filing.command().clone() });
        }
        self.enter(at, Stage::Commit)?;
        self.refuse_if_poisoned()?;
        let id = self.env.request_id()?;
        let ctx =
            RecordContext { stream: self.stream.clone(), branch: target.branch.clone(), manifests: self.manifests };
        let mut sink = FilingSink { bus: self, target: &target, entry: None };
        filing.commit(id, call.view, unset_time(), &ctx, &mut sink)?;
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
        sources: &[EventSource],
        result: Option<CValue>,
    ) -> Result<CommandResult, BusError> {
        let branch = self.router.branch(principal, call.channel, def)?;
        let draft = self.command_draft(
            principal,
            def,
            validated,
            &events,
            sources,
            &branch,
            ACTIVITY_STREAM,
            seed,
            Vec::new(),
            inv,
            self.env.now(),
        )?;
        let outcome = self
            .streams
            .complete_call(locality(call.channel.kind()), inv.program_step.as_ref(), &draft)
            .map_err(|e| invariant(&format!("call record refused by the stream router: {e}")))?;
        Ok(CommandResult::Done { result, events, entry: None, activity: Some(outcome) })
    }

    /// The command entry draft of a call (module docs, "Entry contents"), at `time`. For a
    /// commit, `time` is [`unset_time`]: the commit stage sets the logical parent and, under
    /// its lock, the time.
    #[allow(clippy::too_many_arguments)]
    fn command_draft(
        &self,
        principal: &Principal,
        def: &CommandDef,
        validated: &Validated,
        events: &[Event],
        sources: &[EventSource],
        branch: &str,
        stream: &str,
        seed: u64,
        consumed_r1: Vec<Digest>,
        inv: &Invocation,
        time: EntryTime,
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
            time,
            principal: principal.to_draft(),
            ai_assistance: ai_assistance(principal.kind()),
            manifests: self.manifests,
            reason,
            human_assertion: None,
            // Set by the route stage once CORE.BUS.ROUTE routes co-edited agent commits.
            agent_coedited: false,
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
                event_sources: sources.to_vec(),
            }),
        })
    }

    /// The record a hook sees of `built`, a command entry.
    fn record<'b>(&self, built: &'b Built, decided_at: Digest) -> Result<CommitRecord<'b>, BusError> {
        let d = &built.draft;
        let DraftBody::Command(c) = &d.body else {
            return Err(invariant(&format!("the commit stage commits command entries only, not {}", d.kind())));
        };
        Ok(CommitRecord {
            origin: Some(EntryOrigin::from_draft(d)?),
            journal: Some(JournalOrigin {
                kind: d.principal.kind,
                command: c.id.clone(),
                major: c.major,
                class: c.class,
                branch: d.branch.clone(),
                source: EventSource::Handler,
                human_assertion: built.entry.header.human_assertion.clone(),
            }),
            decided_at,
            header: &built.entry.header,
            payload: &built.entry.payload,
            args: Some(&c.args),
        })
    }

    /// The commit (module docs, steps 1 to 5).
    fn commit(&self, target: &Target, mut draft: EntryDraft, events: &[Event]) -> Result<EntryHash, BusError> {
        let Ok(_lock) = self.commit_lock.lock() else {
            // A panic while the lock was held (a hook, the journal): the folded state may no
            // longer match the journal.
            self.poison();
            return Err(invariant(
                "an earlier commit panicked while holding the commit lock; the bus is poisoned; reopen the project",
            ));
        };
        self.refuse_if_poisoned()?;
        // 1. tail, then branch head, then the entry time.
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
        draft.time = self.env.now();
        // 2. build.
        let built = self.build(draft, tail_seq, tail_hash)?;
        // 3. stage and precommit.
        let staged = self.hooks.stage(&[(self.record(&built, target.head)?, events)])?;
        // 4. append on the tail of step 1.
        let Built { draft, entry } = built;
        let req = AppendRequest {
            draft,
            blobs: Vec::new(),
            expected_prev: Some(tail_hash),
            salts: Some(RecordedSalts(entry.payload.salts)),
        };
        let hash = match self.journal.append(req) {
            Ok(hash) => hash,
            Err(f) => {
                if f.outcome == AppendOutcome::Unknown {
                    // The entry may have landed without being folded.
                    self.poison();
                }
                return Err(f.error);
            }
        };
        if hash != entry.header.hash {
            self.poison();
            return Err(invariant(&format!(
                "the journal appended {hash}, not the entry {} the hooks judged",
                entry.header.hash
            )));
        }
        // 5. install the staged hook states.
        if let Err(e) = staged.install() {
            self.poison();
            return Err(e);
        }
        Ok(hash)
    }

    fn build(&self, draft: EntryDraft, tail_seq: u64, prev: Digest) -> Result<Built, BusError> {
        let seq = tail_seq.checked_add(1).ok_or_else(|| invariant("sequence overflow"))?;
        let entry = build(&draft, seq, prev, &mut OsSalts).map_err(|e| match e {
            fid_journal::EntryError::RngUnavailable => {
                BusError::Internal(InternalReason::JournalUnavailable, Detail::new(&e.to_string()))
            }
            other => invariant(&format!("entry does not build: {other}")),
        })?;
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
        let hash = self.bus.commit(self.target, draft.clone(), &events)?;
        self.entry = Some(hash);
        Ok(())
    }
}
