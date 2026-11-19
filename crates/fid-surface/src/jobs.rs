//! Invocation tracker: progress, cancellation and honest timeouts for surfaces.
//! Spec node `AUTO.SURF.JOBS`.
//!
//! A surface submits a command through a [`BusTransport`], watches the CORE job the bus
//! runs it as, may ask for cancellation, and, when it stops waiting, gets a [`Report`] whose
//! envelope says honestly what is known:
//!
//! - `not_run` only when it is **certain** the bus never accepted the request: no attempt
//!   left the surface ([`SendError::NotSent`]), or the bus explicitly refused every attempt
//!   that left ([`BusEvent::Refused`]) and no acceptance was ever seen;
//! - `no_reply` in every other case where no final job state arrived, with the job ID once
//!   an explicit acceptance ([`BusEvent::Accepted`]) was seen. A lost acceptance, a dropped
//!   connection, a send whose fate is unknown or a misbehaving bus all end here.
//!
//! The classification is the pure function [`classify`] of the invocation's [`Evidence`].
//! Evidence only grows: once an attempt has left the surface or an acceptance was seen,
//! nothing makes `not_run` reachable again except an explicit refusal of that attempt.
//!
//! Time comes from a [`MonotonicClock`]; the tracker never reads the wall clock for
//! deadlines and never lets its view of time go backwards. Wall-clock time is read only by
//! [`SystemKeySource`] for the UUIDv7 timestamp of idempotency keys.
//!
//! Every resend of an invocation carries the same idempotency key, so the bus's idempotency
//! rule (REQ-CORE.BUS-009) returns the original result instead of committing twice.
//!
//! Memory is bounded: at most [`MAX_INVOCATIONS`] tracked invocations, [`MAX_ATTEMPTS`]
//! attempts each, [`MAX_EVENTS_PER_PUMP`] events per [`Tracker::pump`], and progress is
//! handed to the subscriber synchronously instead of being queued.

use crate::{Envelope, Outcome};
use fid_canon::CValue;
use fid_types::CommandId;
use fid_uuid::Uuid;
use std::cell::Cell;
use std::collections::hash_map::RandomState;
use std::collections::BTreeMap;
use std::fmt;
use std::hash::{BuildHasher, Hasher};
use std::rc::Rc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Cancel command of `CORE.JOB.CMD` (same text as `fid_jobs::CANCEL_JOB`).
pub const CANCEL_JOB: &str = "job.cancel_job@1";
/// Wait command of `CORE.JOB.CMD` (same text as `fid_jobs::WAIT_JOB`).
pub const WAIT_JOB: &str = "job.wait_job@1";
/// Longest wait `job.wait_job@1` accepts (same as `fid_jobs::MAX_WAIT_TIMEOUT`).
pub const MAX_WAIT: Duration = Duration::from_secs(3600);
/// Stage labels are cut to this many characters (same as `fid_jobs::MAX_STAGE_CHARS`).
pub const MAX_STAGE_CHARS: usize = 128;
/// Refusal reasons are cut to this many characters.
pub const MAX_REASON_CHARS: usize = 256;
/// Attempts per invocation (and cancel attempts per invocation).
pub const MAX_ATTEMPTS: u32 = 64;
/// Invocations a tracker holds at once, finished ones included until [`Tracker::take`].
pub const MAX_INVOCATIONS: usize = 4096;
/// Events one [`Tracker::pump`] reads at most.
pub const MAX_EVENTS_PER_PUMP: usize = 4096;

// ------------------------------------------------------------------ clocks

/// Monotonic time: elapsed time since an arbitrary origin. Never wall-clock time.
pub trait MonotonicClock {
    fn now(&self) -> Duration;
}

/// Real monotonic time from [`Instant`].
#[derive(Debug, Clone, Copy)]
pub struct InstantClock {
    origin: Instant,
}

impl InstantClock {
    pub fn new() -> Self {
        InstantClock { origin: Instant::now() }
    }
}

impl Default for InstantClock {
    fn default() -> Self {
        Self::new()
    }
}

impl MonotonicClock for InstantClock {
    fn now(&self) -> Duration {
        self.origin.elapsed()
    }
}

/// A clock moved only by hand, for deterministic tests. Clones share the same time.
/// [`ManualClock::set`] may move it backwards, to test that the tracker ignores that.
#[derive(Debug, Clone, Default)]
pub struct ManualClock {
    now: Rc<Cell<Duration>>,
}

impl ManualClock {
    pub fn new() -> Self {
        Self::default()
    }

    /// Moves forward by `by` (saturating).
    pub fn advance(&self, by: Duration) {
        self.now.set(self.now.get().saturating_add(by));
    }

    /// Sets the time, forwards or backwards.
    pub fn set(&self, to: Duration) {
        self.now.set(to);
    }
}

impl MonotonicClock for ManualClock {
    fn now(&self) -> Duration {
        self.now.get()
    }
}

// ------------------------------------------------------------------ idempotency keys

/// The RFC 9562 §5.7 UUIDv7 with this 48-bit Unix-millisecond timestamp and these 74
/// random bits (higher bits of both inputs are ignored).
pub fn uuid_v7(unix_ms: u64, random: u128) -> Uuid {
    let ms = u128::from(unix_ms) & ((1u128 << 48) - 1);
    let r = random & ((1u128 << 74) - 1);
    let rand_a = r >> 62;
    let rand_b = r & ((1u128 << 62) - 1);
    Uuid::from_u128((ms << 80) | (0x7u128 << 76) | (rand_a << 64) | (0b10u128 << 62) | rand_b)
}

/// Source of fresh idempotency keys.
pub trait KeySource {
    fn next_key(&mut self) -> Uuid;
}

/// UUIDv7 keys from the system time and OS-seeded hashing (uniqueness, not secrecy; the
/// tracker also refuses a key it already holds).
#[derive(Debug, Clone, Default)]
pub struct SystemKeySource {
    seq: u64,
}

impl SystemKeySource {
    pub fn new() -> Self {
        Self::default()
    }
}

impl KeySource for SystemKeySource {
    fn next_key(&mut self) -> Uuid {
        let ms = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
        let ms = u64::try_from(ms).unwrap_or(u64::MAX);
        self.seq = self.seq.wrapping_add(1);
        let hi = random_u64(self.seq, 0x7375_7266_5f68_6921);
        let lo = random_u64(self.seq, 0x7375_7266_5f6c_6f21);
        uuid_v7(ms, (u128::from(hi) << 64) | u128::from(lo))
    }
}

fn random_u64(seq: u64, salt: u64) -> u64 {
    let mut h = RandomState::new().build_hasher();
    h.write_u64(seq);
    h.write_u64(salt);
    h.finish()
}

/// Deterministic UUIDv7 keys `uuid_v7(unix_ms, 1)`, `uuid_v7(unix_ms, 2)`, ... for tests.
#[derive(Debug, Clone)]
pub struct CountingKeySource {
    unix_ms: u64,
    next: u128,
}

impl CountingKeySource {
    pub fn new(unix_ms: u64) -> Self {
        CountingKeySource { unix_ms, next: 1 }
    }
}

impl KeySource for CountingKeySource {
    fn next_key(&mut self) -> Uuid {
        let k = uuid_v7(self.unix_ms, self.next);
        self.next = self.next.wrapping_add(1);
        k
    }
}

// ------------------------------------------------------------------ wire

/// Local handle of one tracked invocation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct InvocationId(u64);

impl InvocationId {
    pub fn get(self) -> u64 {
        self.0
    }
}

/// Why a request is sent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Purpose {
    /// The invocation's command (first send or resend).
    Submit,
    /// `job.cancel_job@1` for the invocation's job.
    Cancel,
    /// `job.wait_job@1` to re-attach to the invocation's job.
    Wait,
}

/// One message to the bus.
#[derive(Debug, Clone, PartialEq)]
pub struct BusRequest {
    pub purpose: Purpose,
    /// Idempotency key; the same on every attempt of the same request.
    pub key: Uuid,
    /// Attempt number under this key, from 0. The bus echoes it in [`BusEvent::Refused`].
    pub attempt: u32,
    pub command: CommandId,
    pub params: CValue,
}

/// A failed send.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SendError {
    /// Certain that nothing left the surface (e.g. the transport was closed before writing).
    NotSent,
    /// The request may or may not have left (e.g. the connection broke mid-write).
    Unknown,
}

impl SendError {
    pub fn code(self) -> &'static str {
        match self {
            SendError::NotSent => "transport_closed",
            SendError::Unknown => "transport_unknown",
        }
    }
}

/// One message from the bus or the transport.
#[derive(Debug, Clone, PartialEq)]
pub enum BusEvent {
    /// The bus explicitly did not accept attempt `attempt` of `key` and will not run it.
    Refused { key: Uuid, attempt: u32, reason: String },
    /// The bus accepted `key` as job `job` (REQ-AUTO.SURF.JOBS-002).
    Accepted { key: Uuid, job: Uuid },
    /// Progress of a job.
    Progress { job: Uuid, fraction: f64, stage: String },
    /// The final state of the job that runs `key`.
    Finished { key: Uuid, job: Uuid, envelope: Box<Envelope> },
    /// The connection to the bus was lost.
    Disconnected,
    /// The connection to the bus is back.
    Reconnected,
}

/// How a surface talks to the bus: in-process, control channel or host protocol.
pub trait BusTransport {
    fn send(&mut self, request: &BusRequest) -> Result<(), SendError>;
    /// The next pending event, without blocking.
    fn recv(&mut self) -> Option<BusEvent>;
}

/// A progress update handed to the subscriber.
#[derive(Debug, Clone, PartialEq)]
pub struct ProgressUpdate {
    /// Finite, in [0, 1].
    pub fraction: f64,
    /// At most [`MAX_STAGE_CHARS`] characters.
    pub stage: String,
}

/// Receives progress of accepted invocations, in the order the job published it.
pub trait ProgressSubscriber {
    fn progress(&mut self, id: InvocationId, update: &ProgressUpdate);
}

impl ProgressSubscriber for () {
    fn progress(&mut self, _id: InvocationId, _update: &ProgressUpdate) {}
}

impl ProgressSubscriber for Vec<(InvocationId, ProgressUpdate)> {
    fn progress(&mut self, id: InvocationId, update: &ProgressUpdate) {
        self.push((id, update.clone()));
    }
}

// ------------------------------------------------------------------ evidence and reports

/// What the tracker knows about whether the bus accepted an invocation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Evidence {
    /// Bit `a` set: attempt `a` left the surface or may have.
    pub left: u64,
    /// Bit `a` set: the bus explicitly refused attempt `a` (always a subset of `left`).
    pub refused: u64,
    /// The job named by the first explicit acceptance.
    pub job: Option<Uuid>,
    /// Sticky: some `Accepted` or `Finished` message named this key, so the bus took it.
    /// Kept apart from [`State::Accepted`], which only an explicit acceptance enters
    /// (REQ-AUTO.SURF.JOBS-002). Once set, `not_run` is unreachable whatever comes later.
    pub taken: bool,
}

impl Evidence {
    /// True only when it is certain the bus never accepted the invocation.
    pub fn never_accepted(&self) -> bool {
        !self.taken && self.job.is_none() && self.left & !self.refused == 0
    }
}

/// The classification of an invocation that has no final job state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    NotRun,
    NoReply(Option<Uuid>),
}

/// Pure classification (REQ-AUTO.SURF.JOBS-003, REQ-AUTO.SURF-007): `not_run` only when
/// [`Evidence::never_accepted`], otherwise `no_reply` with the accepted job if any.
pub fn classify(e: &Evidence) -> Verdict {
    match e.job {
        Some(job) => Verdict::NoReply(Some(job)),
        None if e.never_accepted() => Verdict::NotRun,
        None => Verdict::NoReply(None),
    }
}

/// A bus message that breaks the protocol.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Violation {
    /// A final state before any acceptance.
    FinishBeforeAccept,
    /// An acceptance or final state naming a different job than the first acceptance.
    JobMismatch,
    /// An acceptance naming a job another invocation already owns.
    JobIdReused,
    /// A final envelope that is invalid, not final, or for another command or job.
    BadFinalEnvelope(&'static str),
}

impl Violation {
    pub fn code(&self) -> &'static str {
        match self {
            Violation::FinishBeforeAccept => "finish_before_accept",
            Violation::JobMismatch => "job_mismatch",
            Violation::JobIdReused => "job_id_reused",
            Violation::BadFinalEnvelope(c) => c,
        }
    }
}

/// Why an invocation ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reason {
    /// The bus sent the job's final state; the envelope is the bus's own.
    Final,
    /// No attempt left the surface.
    NotSent,
    /// The bus explicitly refused every attempt that left; the last reason given.
    Refused(String),
    /// The caller's deadline passed.
    Deadline,
    /// The connection to the bus was lost.
    TransportClosed,
    /// The bus broke the protocol for this key. Its final message is not trusted, and
    /// [`Tracker::reattach`] is refused for the key from then on ([`TrackerError::Untrusted`]).
    ProtocolViolation(Violation),
}

impl Reason {
    pub fn code(&self) -> &'static str {
        match self {
            Reason::Final => "final",
            Reason::NotSent => "not_sent",
            Reason::Refused(_) => "refused",
            Reason::Deadline => "deadline",
            Reason::TransportClosed => "transport_closed",
            Reason::ProtocolViolation(_) => "protocol_violation",
        }
    }
}

/// How an invocation ended.
#[derive(Debug, Clone, PartialEq)]
pub struct Report {
    pub envelope: Envelope,
    pub reason: Reason,
}

/// Invocation state (spec: Created | Sent | Accepted{job} | Finished(Envelope)).
#[derive(Debug, Clone, PartialEq)]
pub enum State {
    /// Created; no attempt has left yet.
    Created,
    /// At least one attempt may have left; no acceptance seen.
    Sent,
    /// Accepted as this job; no final state yet.
    Accepted {
        job: Uuid,
    },
    Finished(Box<Report>),
}

/// Cancellation progress of one invocation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CancelState {
    NotRequested,
    /// Requested before acceptance; sent when the acceptance arrives.
    Deferred,
    /// `job.cancel_job@1` left the surface.
    Sent,
    /// The last cancel send may or may not have left; may be retried.
    Unsure,
    /// The last cancel send did not leave; may be retried.
    Failed,
}

/// Result of [`Tracker::request_cancel`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CancelStatus {
    Sent,
    Deferred,
    AlreadySent,
}

/// Tracker call errors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrackerError {
    UnknownInvocation,
    /// [`MAX_INVOCATIONS`] are held; [`Tracker::take`] finished ones.
    Full,
    /// The key source returned a key the tracker already holds.
    DuplicateKey,
    TooManyAttempts,
    /// The invocation has a final job state.
    AlreadyFinal,
    /// The invocation is accepted and still waiting; resending is not needed.
    AlreadyAccepted,
    /// No job is known for the invocation.
    NoJob,
    /// The bus broke the protocol for this invocation's key; the tracker will not reattach.
    Untrusted,
    /// A built-in command ID did not parse (a bug).
    BadCommandId,
    Transport(SendError),
}

impl TrackerError {
    pub fn code(&self) -> &'static str {
        match self {
            TrackerError::UnknownInvocation => "unknown_invocation",
            TrackerError::Full => "tracker_full",
            TrackerError::DuplicateKey => "duplicate_key",
            TrackerError::TooManyAttempts => "too_many_attempts",
            TrackerError::AlreadyFinal => "already_final",
            TrackerError::AlreadyAccepted => "already_accepted",
            TrackerError::NoJob => "no_job",
            TrackerError::Untrusted => "bus_untrusted",
            TrackerError::BadCommandId => "bad_command_id",
            TrackerError::Transport(e) => e.code(),
        }
    }
}

impl fmt::Display for TrackerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code())
    }
}

impl std::error::Error for TrackerError {}

/// Counts of messages the tracker ignored or found wrong (all saturating).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Counters {
    /// Events for a key or job the tracker does not hold.
    pub unmatched: u64,
    /// Repeated acceptances of the same job.
    pub duplicates: u64,
    /// Events for an invocation that had already ended.
    pub late: u64,
    /// Protocol violations, including those after an invocation ended.
    pub violations: u64,
    /// A refusal of an attempt that never left.
    pub bad_refusals: u64,
    /// Progress dropped: invalid fraction, or the job is not accepted and waiting.
    pub ignored_progress: u64,
    pub disconnects: u64,
    pub reconnects: u64,
}

fn bump(c: &mut u64) {
    *c = c.saturating_add(1);
}

struct Inv {
    key: Uuid,
    command: CommandId,
    params: CValue,
    state: State,
    evidence: Evidence,
    attempts: u32,
    deadline: Duration,
    cancel: CancelState,
    cancel_key: Option<Uuid>,
    cancel_attempts: u32,
    /// The last refusal reason the bus gave.
    refusal: Option<String>,
    /// Sticky: the bus broke the protocol for this key; never reattach to it.
    violated: bool,
}

impl Inv {
    fn waiting(&self) -> bool {
        !matches!(self.state, State::Finished(_))
    }

    /// Ends the invocation with the classification of its evidence.
    fn conclude(&mut self, reason: Reason) {
        let envelope = match classify(&self.evidence) {
            Verdict::NotRun => Envelope::bare(self.command.as_str(), Outcome::NotRun),
            Verdict::NoReply(job) => Envelope {
                job_id: job.map(|j| j.to_string()),
                ..Envelope::bare(self.command.as_str(), Outcome::NoReply)
            },
        };
        if matches!(reason, Reason::ProtocolViolation(_)) {
            self.violated = true;
        }
        self.state = State::Finished(Box::new(Report { envelope, reason }));
    }

    fn reported_not_run(&self) -> bool {
        matches!(&self.state, State::Finished(r) if r.envelope.outcome == Outcome::NotRun)
    }
}

fn cut(s: &str, max: usize) -> String {
    s.chars().take(max).collect()
}

fn command(s: &str) -> Result<CommandId, TrackerError> {
    CommandId::parse(s).map_err(|_| TrackerError::BadCommandId)
}

fn bit(attempt: u32) -> u64 {
    1u64.checked_shl(attempt).unwrap_or(0)
}

/// Checks a final envelope from the bus for this command and job.
fn check_final(env: &Envelope, cmd: &CommandId, job: Uuid) -> Result<(), &'static str> {
    if env.validate().is_err() {
        return Err("invalid_final_envelope");
    }
    if env.command_id != cmd.as_str() {
        return Err("final_command_mismatch");
    }
    let is_final = matches!(
        env.outcome,
        Outcome::Committed
            | Outcome::Computed
            | Outcome::DryRun
            | Outcome::RequestFiled
            | Outcome::Failed
            | Outcome::Cancelled
    );
    if !is_final {
        return Err("not_final_outcome");
    }
    match &env.job_id {
        Some(j) if *j != job.to_string() => Err("final_job_mismatch"),
        _ => Ok(()),
    }
}

// ------------------------------------------------------------------ tracker

/// Tracks invocations over one transport.
pub struct Tracker<T, C, K> {
    transport: T,
    clock: C,
    keys: K,
    invs: BTreeMap<InvocationId, Inv>,
    by_key: BTreeMap<Uuid, InvocationId>,
    by_job: BTreeMap<Uuid, InvocationId>,
    next_id: u64,
    floor: Duration,
    counters: Counters,
}

impl<T, C, K> fmt::Debug for Tracker<T, C, K> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Tracker")
            .field("invocations", &self.invs.len())
            .field("counters", &self.counters)
            .finish_non_exhaustive()
    }
}

impl<T: BusTransport, C: MonotonicClock, K: KeySource> Tracker<T, C, K> {
    pub fn new(transport: T, clock: C, keys: K) -> Self {
        Tracker {
            transport,
            clock,
            keys,
            invs: BTreeMap::new(),
            by_key: BTreeMap::new(),
            by_job: BTreeMap::new(),
            next_id: 0,
            floor: Duration::ZERO,
            counters: Counters::default(),
        }
    }

    pub fn transport(&self) -> &T {
        &self.transport
    }

    pub fn transport_mut(&mut self) -> &mut T {
        &mut self.transport
    }

    pub fn counters(&self) -> Counters {
        self.counters
    }

    /// Number of invocations held.
    pub fn len(&self) -> usize {
        self.invs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.invs.is_empty()
    }

    /// The tracker's time: the clock, but never earlier than any time already seen.
    fn now(&mut self) -> Duration {
        let n = self.clock.now();
        if n > self.floor {
            self.floor = n;
        }
        self.floor
    }

    pub fn state(&self, id: InvocationId) -> Option<&State> {
        self.invs.get(&id).map(|i| &i.state)
    }

    pub fn report(&self, id: InvocationId) -> Option<&Report> {
        match self.state(id) {
            Some(State::Finished(r)) => Some(r.as_ref()),
            _ => None,
        }
    }

    pub fn key(&self, id: InvocationId) -> Option<Uuid> {
        self.invs.get(&id).map(|i| i.key)
    }

    pub fn evidence(&self, id: InvocationId) -> Option<Evidence> {
        self.invs.get(&id).map(|i| i.evidence)
    }

    pub fn cancel_state(&self, id: InvocationId) -> Option<CancelState> {
        self.invs.get(&id).map(|i| i.cancel)
    }

    pub fn deadline(&self, id: InvocationId) -> Option<Duration> {
        self.invs.get(&id).map(|i| i.deadline)
    }

    /// Removes a finished invocation and returns its report; `None` while it waits.
    pub fn take(&mut self, id: InvocationId) -> Option<Report> {
        if self.invs.get(&id).is_some_and(Inv::waiting) {
            return None;
        }
        let inv = self.invs.remove(&id)?;
        self.by_key.remove(&inv.key);
        if let Some(job) = inv.evidence.job {
            if self.by_job.get(&job) == Some(&id) {
                self.by_job.remove(&job);
            }
        }
        match inv.state {
            State::Finished(r) => Some(*r),
            _ => None,
        }
    }

    /// Submits `command` with a fresh idempotency key and waits at most `timeout`
    /// (REQ-AUTO.SURF.JOBS-001). A send that certainly did not leave ends the invocation
    /// at once as `not_run`; read [`Tracker::state`].
    pub fn submit(
        &mut self,
        command: CommandId,
        params: CValue,
        timeout: Duration,
    ) -> Result<InvocationId, TrackerError> {
        if self.invs.len() >= MAX_INVOCATIONS {
            return Err(TrackerError::Full);
        }
        let key = self.keys.next_key();
        if self.by_key.contains_key(&key) {
            return Err(TrackerError::DuplicateKey);
        }
        let id = InvocationId(self.next_id);
        if self.invs.contains_key(&id) {
            return Err(TrackerError::Full);
        }
        self.next_id = self.next_id.wrapping_add(1);
        let deadline = self.now().saturating_add(timeout);
        self.invs.insert(
            id,
            Inv {
                key,
                command,
                params,
                state: State::Created,
                evidence: Evidence::default(),
                attempts: 0,
                deadline,
                cancel: CancelState::NotRequested,
                cancel_key: None,
                cancel_attempts: 0,
                refusal: None,
                violated: false,
            },
        );
        self.by_key.insert(key, id);
        self.attempt(id, timeout)?;
        Ok(id)
    }

    /// Sends the invocation again with the same key and a new deadline
    /// (REQ-AUTO.SURF.JOBS-001, -006). Allowed while unaccepted, or after `not_run` or
    /// `no_reply`. Knowledge of an earlier acceptance is kept.
    pub fn resend(&mut self, id: InvocationId, timeout: Duration) -> Result<(), TrackerError> {
        let inv = self.invs.get(&id).ok_or(TrackerError::UnknownInvocation)?;
        match &inv.state {
            State::Created | State::Sent => {}
            State::Accepted { .. } => return Err(TrackerError::AlreadyAccepted),
            State::Finished(r) => {
                if !matches!(r.envelope.outcome, Outcome::NotRun | Outcome::NoReply) {
                    return Err(TrackerError::AlreadyFinal);
                }
            }
        }
        self.attempt(id, timeout)
    }

    fn attempt(&mut self, id: InvocationId, timeout: Duration) -> Result<(), TrackerError> {
        let now = self.now();
        let inv = self.invs.get_mut(&id).ok_or(TrackerError::UnknownInvocation)?;
        if inv.attempts >= MAX_ATTEMPTS {
            return Err(TrackerError::TooManyAttempts);
        }
        let attempt = inv.attempts;
        inv.attempts += 1;
        let request = BusRequest {
            purpose: Purpose::Submit,
            key: inv.key,
            attempt,
            command: inv.command.clone(),
            params: inv.params.clone(),
        };
        match self.transport.send(&request) {
            Ok(()) | Err(SendError::Unknown) => inv.evidence.left |= bit(attempt),
            Err(SendError::NotSent) => {}
        }
        inv.deadline = now.saturating_add(timeout);
        if inv.evidence.never_accepted() {
            let reason = match (&inv.refusal, inv.evidence.refused) {
                (Some(why), r) if r != 0 => Reason::Refused(why.clone()),
                _ => Reason::NotSent,
            };
            inv.conclude(reason);
            return Ok(());
        }
        match inv.evidence.job {
            Some(job) => self.enter_accepted(id, job),
            None => inv.state = State::Sent,
        }
        Ok(())
    }

    /// Moves a waiting invocation to `Accepted { job }` and sends a deferred cancel.
    fn enter_accepted(&mut self, id: InvocationId, job: Uuid) {
        let Some(inv) = self.invs.get_mut(&id) else { return };
        inv.state = State::Accepted { job };
        self.by_job.entry(job).or_insert(id);
        if inv.cancel == CancelState::Deferred {
            // A failed send is kept in the cancel state; the caller may retry.
            let _ = self.send_cancel(id, job);
        }
    }

    /// Requests cancellation (REQ-AUTO.SURF.JOBS-005): sends `job.cancel_job@1` once for an
    /// accepted invocation, or defers it until the acceptance arrives. The invocation
    /// keeps waiting for the job's final state.
    pub fn request_cancel(&mut self, id: InvocationId) -> Result<CancelStatus, TrackerError> {
        let inv = self.invs.get_mut(&id).ok_or(TrackerError::UnknownInvocation)?;
        match inv.state {
            State::Finished(_) => Err(TrackerError::AlreadyFinal),
            State::Created | State::Sent => {
                inv.cancel = CancelState::Deferred;
                Ok(CancelStatus::Deferred)
            }
            State::Accepted { job } => {
                if inv.cancel == CancelState::Sent {
                    return Ok(CancelStatus::AlreadySent);
                }
                self.send_cancel(id, job)
            }
        }
    }

    fn send_cancel(&mut self, id: InvocationId, job: Uuid) -> Result<CancelStatus, TrackerError> {
        let cmd = command(CANCEL_JOB)?;
        let inv = self.invs.get_mut(&id).ok_or(TrackerError::UnknownInvocation)?;
        if inv.cancel_attempts >= MAX_ATTEMPTS {
            return Err(TrackerError::TooManyAttempts);
        }
        let key = match inv.cancel_key {
            Some(k) => k,
            None => {
                let k = self.keys.next_key();
                inv.cancel_key = Some(k);
                k
            }
        };
        let attempt = inv.cancel_attempts;
        inv.cancel_attempts += 1;
        let mut params = BTreeMap::new();
        params.insert("job".to_string(), CValue::Str(job.to_string()));
        let request = BusRequest { purpose: Purpose::Cancel, key, attempt, command: cmd, params: CValue::Obj(params) };
        match self.transport.send(&request) {
            Ok(()) => {
                inv.cancel = CancelState::Sent;
                Ok(CancelStatus::Sent)
            }
            Err(e) => {
                inv.cancel = if e == SendError::Unknown { CancelState::Unsure } else { CancelState::Failed };
                Err(TrackerError::Transport(e))
            }
        }
    }

    /// Re-attaches to the invocation's known job with `job.wait_job@1` (e.g. after a
    /// reconnect) and waits at most `timeout` again. Allowed while accepted or after
    /// `no_reply` with a job, unless the bus ever broke the protocol for this key. Sends a
    /// deferred cancel. On a failed send nothing changes.
    pub fn reattach(&mut self, id: InvocationId, timeout: Duration) -> Result<(), TrackerError> {
        let cmd = command(WAIT_JOB)?;
        let now = self.now();
        let inv = self.invs.get(&id).ok_or(TrackerError::UnknownInvocation)?;
        if inv.violated {
            return Err(TrackerError::Untrusted);
        }
        let job = inv.evidence.job.ok_or(TrackerError::NoJob)?;
        if let State::Finished(r) = &inv.state {
            if r.envelope.outcome != Outcome::NoReply {
                return Err(TrackerError::AlreadyFinal);
            }
        }
        let key = self.keys.next_key();
        let wait = if timeout > MAX_WAIT { MAX_WAIT } else { timeout };
        let mut params = BTreeMap::new();
        params.insert("job".to_string(), CValue::Str(job.to_string()));
        params.insert("timeout".to_string(), CValue::Num(wait.as_millis() as f64 / 1000.0));
        let request = BusRequest { purpose: Purpose::Wait, key, attempt: 0, command: cmd, params: CValue::Obj(params) };
        match self.transport.send(&request) {
            Ok(()) | Err(SendError::Unknown) => {}
            Err(e) => return Err(TrackerError::Transport(e)),
        }
        if let Some(inv) = self.invs.get_mut(&id) {
            inv.deadline = now.saturating_add(timeout);
        }
        self.enter_accepted(id, job);
        Ok(())
    }

    /// Reads pending events (at most [`MAX_EVENTS_PER_PUMP`]), then ends every waiting
    /// invocation whose deadline has passed. Events read before the deadline check win a
    /// race with it. Returns the number of events read.
    pub fn pump(&mut self, subscriber: &mut dyn ProgressSubscriber) -> usize {
        let mut n = 0;
        while n < MAX_EVENTS_PER_PUMP {
            let Some(event) = self.transport.recv() else {
                break;
            };
            n += 1;
            self.on_event(event, subscriber);
        }
        let now = self.now();
        for inv in self.invs.values_mut() {
            if inv.waiting() && inv.deadline <= now {
                inv.conclude(Reason::Deadline);
            }
        }
        n
    }

    fn on_event(&mut self, event: BusEvent, subscriber: &mut dyn ProgressSubscriber) {
        match event {
            BusEvent::Refused { key, attempt, reason } => self.on_refused(key, attempt, &reason),
            BusEvent::Accepted { key, job } => self.on_accepted(key, job),
            BusEvent::Progress { job, fraction, stage } => self.on_progress(job, fraction, &stage, subscriber),
            BusEvent::Finished { key, job, envelope } => self.on_finished(key, job, envelope),
            BusEvent::Disconnected => {
                bump(&mut self.counters.disconnects);
                for inv in self.invs.values_mut() {
                    if inv.waiting() {
                        inv.conclude(Reason::TransportClosed);
                    }
                }
            }
            BusEvent::Reconnected => bump(&mut self.counters.reconnects),
        }
    }

    fn lookup(&mut self, key: Uuid) -> Option<InvocationId> {
        let id = self.by_key.get(&key).copied();
        if id.is_none() {
            bump(&mut self.counters.unmatched);
        }
        id
    }

    fn on_refused(&mut self, key: Uuid, attempt: u32, reason: &str) {
        let Some(id) = self.lookup(key) else { return };
        let Some(inv) = self.invs.get_mut(&id) else { return };
        let b = bit(attempt);
        if b == 0 || inv.evidence.left & b == 0 {
            bump(&mut self.counters.bad_refusals);
            return;
        }
        inv.evidence.refused |= b;
        inv.refusal = Some(cut(reason, MAX_REASON_CHARS));
        match inv.state {
            State::Created | State::Sent => {
                if inv.evidence.never_accepted() {
                    inv.conclude(Reason::Refused(cut(reason, MAX_REASON_CHARS)));
                }
            }
            State::Accepted { .. } => {}
            State::Finished(_) => bump(&mut self.counters.late),
        }
    }

    fn on_accepted(&mut self, key: Uuid, job: Uuid) {
        let Some(id) = self.lookup(key) else { return };
        let owner = self.by_job.get(&job).copied();
        let Some(inv) = self.invs.get_mut(&id) else { return };
        inv.evidence.taken = true;
        match inv.evidence.job {
            Some(j) if j != job => {
                bump(&mut self.counters.violations);
                inv.violated = true;
                if inv.waiting() {
                    inv.conclude(Reason::ProtocolViolation(Violation::JobMismatch));
                }
                return;
            }
            Some(_) => {
                bump(&mut self.counters.duplicates);
                return;
            }
            None => {}
        }
        // An explicit acceptance: from here on `not_run` is unreachable for this invocation.
        inv.evidence.job = Some(job);
        if owner.is_some_and(|o| o != id) {
            bump(&mut self.counters.violations);
            inv.violated = true;
            if inv.waiting() {
                inv.conclude(Reason::ProtocolViolation(Violation::JobIdReused));
            }
            return;
        }
        self.by_job.insert(job, id);
        match inv.state {
            State::Created | State::Sent => self.enter_accepted(id, job),
            State::Accepted { .. } => {}
            State::Finished(_) => {
                bump(&mut self.counters.late);
                if inv.reported_not_run() {
                    // The bus contradicted its own refusal; the report cannot be recalled.
                    bump(&mut self.counters.violations);
                    inv.violated = true;
                }
            }
        }
    }

    fn on_progress(&mut self, job: Uuid, fraction: f64, stage: &str, subscriber: &mut dyn ProgressSubscriber) {
        let Some(id) = self.by_job.get(&job).copied() else {
            bump(&mut self.counters.unmatched);
            return;
        };
        let accepted = matches!(self.invs.get(&id).map(|i| &i.state), Some(State::Accepted { job: j }) if *j == job);
        if !accepted || !fraction.is_finite() || !(0.0..=1.0).contains(&fraction) {
            bump(&mut self.counters.ignored_progress);
            return;
        }
        subscriber.progress(id, &ProgressUpdate { fraction, stage: cut(stage, MAX_STAGE_CHARS) });
    }

    fn on_finished(&mut self, key: Uuid, job: Uuid, envelope: Box<Envelope>) {
        let Some(id) = self.lookup(key) else { return };
        let Some(inv) = self.invs.get_mut(&id) else { return };
        // Any final message naming the key is evidence the bus took it, even out of order.
        inv.evidence.taken = true;
        match inv.state {
            State::Finished(_) => bump(&mut self.counters.late),
            State::Created | State::Sent => {
                bump(&mut self.counters.violations);
                inv.conclude(Reason::ProtocolViolation(Violation::FinishBeforeAccept));
            }
            State::Accepted { job: accepted } => {
                if accepted != job {
                    bump(&mut self.counters.violations);
                    inv.conclude(Reason::ProtocolViolation(Violation::JobMismatch));
                    return;
                }
                match check_final(&envelope, &inv.command, accepted) {
                    Ok(()) => {
                        inv.state = State::Finished(Box::new(Report { envelope: *envelope, reason: Reason::Final }))
                    }
                    Err(why) => {
                        bump(&mut self.counters.violations);
                        inv.conclude(Reason::ProtocolViolation(Violation::BadFinalEnvelope(why)));
                    }
                }
            }
        }
    }
}
