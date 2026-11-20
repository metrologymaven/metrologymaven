//! Job model, queue, priorities, limits and timeouts (spec node `CORE.JOB.SCHED`).
//!
//! [`Jobs`] runs submitted [`Execution`]s as jobs:
//!
//! - [`Jobs::submit`] assigns a UUIDv7 [`JobId`] at once, records the job as `queued` and
//!   returns; the handler runs on its own worker thread (REQ-CORE.JOB.SCHED-001);
//! - at most `max_running` handlers run at once; the rest wait in two FIFO queues, and a free
//!   slot always goes to the oldest `interactive` job before any `batch` job
//!   (REQ-CORE.JOB.SCHED-003, -004). Running jobs are never pre-empted;
//! - a running job past its timeout has its [`CancelToken`] set and ends `failed` with
//!   [`JobErrorCode::Timeout`] (REQ-CORE.JOB.SCHED-005);
//! - a result is kept only when the job ends `succeeded`; [`Jobs::take_result`] hands it out
//!   once. A failed or cancelled job's output is dropped (REQ-CORE.JOB.SCHED-006);
//! - every state change, including submission (`None -> queued`), is emitted as one
//!   [`LifecycleEvent`] to the [`LifecycleSink`] (wired by the journal to the Activity Log).
//!   This module has no handle on the Record Journal (REQ-CORE.JOB.SCHED-008).
//!
//! Dispatch decisions are made under one lock, on submission and when a worker returns, so
//! the start order is a pure function of the submission order and the completion order.
//! Times (`submitted_at`, timeouts) come from an injectable [`Clock`]; with a
//! [`crate::ManualClock`] and [`JobsConfig::timer_poll`] set to `None`, tests drive timeouts
//! with [`Jobs::check_timeouts`] and are free of wall-clock races.
//!
//! A slot stays taken until the handler returns, also after a timeout: cancellation is
//! cooperative, so "running at most N" counts handler threads, not job states.

use crate::progress::{CancelToken, Clock, Progress, ProgressSink, ProgressSnapshot, SystemClock};
use fid_types::CommandId;
use fid_uuid::Uuid;
use std::collections::hash_map::RandomState;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fmt;
use std::hash::{BuildHasher, Hasher};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Default cap on queued (not yet running) jobs; one more submission gets `queue_full`.
pub const DEFAULT_QUEUE_CAP: usize = 10_000;
/// Default number of ended jobs whose records (and untaken results) are kept; older ones are
/// dropped first.
pub const DEFAULT_RETAIN_ENDED: usize = 10_000;
/// Default polling interval of the timeout thread.
pub const DEFAULT_TIMER_POLL: Duration = Duration::from_millis(5);
/// Longest handler failure message kept in a record, in characters.
pub const MAX_ERROR_DETAIL_CHARS: usize = 256;

const MIN_TIMER_POLL: Duration = Duration::from_millis(1);
/// Longest time [`Jobs::wait_cancellable`] sleeps before it looks at the caller's token again.
pub const WAIT_CANCEL_POLL: Duration = Duration::from_millis(5);

/// Wall-clock time source: time since the Unix epoch (UTC). Used only for the timestamps
/// shown to people; ordering and timeouts use the monotonic [`Clock`].
pub trait WallClock: Send + Sync {
    /// Time since 1970-01-01T00:00:00Z; zero if the system clock is before the epoch.
    fn unix_time(&self) -> Duration;
}

/// The system's real-time clock.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemWallClock;

impl WallClock for SystemWallClock {
    fn unix_time(&self) -> Duration {
        SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or(Duration::ZERO)
    }
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

// ------------------------------------------------------------------ vocabulary

/// Priority class of a job (REQ-CORE.JOB.SCHED-004).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum JobClass {
    /// Started before any batch job.
    Interactive,
    /// Started only when no interactive job is queued.
    Batch,
}

impl JobClass {
    /// The stable wire name.
    pub fn as_str(self) -> &'static str {
        match self {
            JobClass::Interactive => "interactive",
            JobClass::Batch => "batch",
        }
    }
}

impl fmt::Display for JobClass {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// State of a job (REQ-CORE.JOB.SCHED-002).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum JobState {
    Queued,
    Running,
    Succeeded,
    Failed,
    Cancelled,
}

impl JobState {
    /// The stable wire name.
    pub fn as_str(self) -> &'static str {
        match self {
            JobState::Queued => "queued",
            JobState::Running => "running",
            JobState::Succeeded => "succeeded",
            JobState::Failed => "failed",
            JobState::Cancelled => "cancelled",
        }
    }

    /// True for `succeeded`, `failed` and `cancelled`.
    pub fn is_terminal(self) -> bool {
        matches!(self, JobState::Succeeded | JobState::Failed | JobState::Cancelled)
    }

    /// Whether `self -> to` is one of the allowed moves: queued -> running,
    /// running -> succeeded | failed | cancelled, queued -> cancelled.
    pub fn can_move_to(self, to: JobState) -> bool {
        matches!(
            (self, to),
            (JobState::Queued, JobState::Running)
                | (JobState::Queued, JobState::Cancelled)
                | (JobState::Running, JobState::Succeeded)
                | (JobState::Running, JobState::Failed)
                | (JobState::Running, JobState::Cancelled)
        )
    }
}

impl fmt::Display for JobState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Kind of principal that submitted a job (CONSTITUTION §3, principals).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PrincipalKind {
    Human,
    Agent,
    Script,
}

impl PrincipalKind {
    /// The stable wire name.
    pub fn as_str(self) -> &'static str {
        match self {
            PrincipalKind::Human => "human",
            PrincipalKind::Agent => "agent",
            PrincipalKind::Script => "script",
        }
    }
}

impl fmt::Display for PrincipalKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Job error codes: why a job ended `failed` or `cancelled`, or why a call was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum JobErrorCode {
    /// The job ran past its timeout (REQ-CORE.JOB.SCHED-005).
    Timeout,
    /// The job was cancelled.
    Cancelled,
    /// The handler returned an error (or its worker thread could not start).
    HandlerFailed,
    /// No job with that ID.
    NotFound,
    /// The queue already holds the configured maximum of queued jobs.
    QueueFull,
}

impl JobErrorCode {
    /// Stable machine-readable error code.
    pub fn code(self) -> &'static str {
        match self {
            JobErrorCode::Timeout => "timeout",
            JobErrorCode::Cancelled => "cancelled",
            JobErrorCode::HandlerFailed => "handler_failed",
            JobErrorCode::NotFound => "not_found",
            JobErrorCode::QueueFull => "queue_full",
        }
    }
}

impl fmt::Display for JobErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code())
    }
}

impl std::error::Error for JobErrorCode {}

/// A job ID: a UUIDv7 (RFC 9562 §5.7) with a Unix-millisecond timestamp and 74 random bits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct JobId(Uuid);

impl JobId {
    /// The job ID with this UUID, e.g. one parsed from a command parameter. Only the
    /// scheduler issues IDs; a UUID it never issued names no job, so lookups give `not_found`.
    pub fn from_uuid(uuid: Uuid) -> JobId {
        JobId(uuid)
    }

    /// The UUID.
    pub fn as_uuid(&self) -> Uuid {
        self.0
    }
}

impl fmt::Display for JobId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}

// ------------------------------------------------------------------ executions and options

/// How a handler ends without a result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HandlerError {
    /// The handler saw its [`CancelToken`] set and stopped.
    Cancelled,
    /// The handler failed; the message is kept (cut to [`MAX_ERROR_DETAIL_CHARS`]).
    Failed(String),
}

/// A job's work: runs once on a worker thread with the job's progress and cancel handles.
pub type Handler<T> = Box<dyn FnOnce(&ProgressSink, &CancelToken) -> Result<T, HandlerError> + Send + 'static>;

/// One command execution to run as a job.
pub struct Execution<T> {
    /// The registry command being executed.
    pub command: CommandId,
    /// The work.
    pub handler: Handler<T>,
}

impl<T> Execution<T> {
    /// An execution of `command` running `handler`.
    pub fn new(
        command: CommandId,
        handler: impl FnOnce(&ProgressSink, &CancelToken) -> Result<T, HandlerError> + Send + 'static,
    ) -> Self {
        Execution { command, handler: Box::new(handler) }
    }
}

impl<T> fmt::Debug for Execution<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Execution").field("command", &self.command).finish_non_exhaustive()
    }
}

/// Per-job options.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JobOpts {
    pub class: JobClass,
    /// Longest time the job may run, counted from its start; `None` for no limit.
    pub timeout: Option<Duration>,
    pub principal: PrincipalKind,
}

impl Default for JobOpts {
    /// A batch job without timeout, submitted by a script.
    fn default() -> Self {
        JobOpts { class: JobClass::Batch, timeout: None, principal: PrincipalKind::Script }
    }
}

/// Scheduler configuration.
#[derive(Clone)]
pub struct JobsConfig {
    /// Most jobs running at once (at least 1 is used).
    pub max_running: usize,
    /// Most jobs waiting for a slot; a submission that would wait beyond this gets
    /// `queue_full`. A submission that finds a free slot is never refused.
    pub queue_cap: usize,
    /// Most ended jobs kept (at least 1 is used). When more have ended, the oldest-ended
    /// record and any untaken result are dropped, as by [`Jobs::forget`].
    pub retain_ended: usize,
    /// Time source for timestamps and timeouts.
    pub clock: Arc<dyn Clock>,
    /// Polling interval of the background timeout thread (at least 1 ms); `None` starts no
    /// thread, and timeouts fire only when [`Jobs::check_timeouts`] is called.
    pub timer_poll: Option<Duration>,
    /// Wall-clock source for the `*_wall` timestamps of records.
    pub wall_clock: Arc<dyn WallClock>,
}

impl Default for JobsConfig {
    fn default() -> Self {
        JobsConfig {
            max_running: default_max_running(),
            queue_cap: DEFAULT_QUEUE_CAP,
            retain_ended: DEFAULT_RETAIN_ENDED,
            clock: Arc::new(SystemClock::new()),
            timer_poll: Some(DEFAULT_TIMER_POLL),
            wall_clock: Arc::new(SystemWallClock),
        }
    }
}

impl fmt::Debug for JobsConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("JobsConfig")
            .field("max_running", &self.max_running)
            .field("queue_cap", &self.queue_cap)
            .field("retain_ended", &self.retain_ended)
            .field("timer_poll", &self.timer_poll)
            .finish_non_exhaustive()
    }
}

/// Default concurrency limit: logical CPU count minus one, at least one
/// (REQ-CORE.JOB.SCHED-003).
pub fn default_max_running() -> usize {
    std::thread::available_parallelism().map(|n| n.get().saturating_sub(1)).unwrap_or(1).max(1)
}

// ------------------------------------------------------------------ records and events

/// Public view of a job.
#[derive(Debug, Clone, PartialEq)]
pub struct JobRecord {
    pub id: JobId,
    pub command: CommandId,
    pub principal_kind: PrincipalKind,
    pub class: JobClass,
    pub state: JobState,
    /// Latest accepted progress report.
    pub progress: ProgressSnapshot,
    /// Clock time of submission (monotonic; the ordering key).
    pub submitted_at: Duration,
    pub started_at: Option<Duration>,
    pub ended_at: Option<Duration>,
    /// Wall-clock time of submission, since the Unix epoch (UTC); for display only.
    pub submitted_wall: Duration,
    /// Wall-clock time of the start, since the Unix epoch (UTC).
    pub started_wall: Option<Duration>,
    /// Wall-clock time of the end, since the Unix epoch (UTC).
    pub ended_wall: Option<Duration>,
    /// Set when the state is `failed` or `cancelled`.
    pub error: Option<JobErrorCode>,
    /// The handler's failure message, for `handler_failed`.
    pub error_detail: Option<String>,
}

/// What [`Jobs::cancel_request`] did, decided under the scheduler's lock.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CancelRequest {
    /// The job's state afterwards.
    pub state: JobState,
    /// True only when this call cancelled a queued job or set the cancellation flag of a
    /// running one; false for an ended job or a running job already asked to cancel.
    pub newly_requested: bool,
}

/// One job state change (REQ-CORE.JOB.SCHED-008).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LifecycleEvent {
    pub job: JobId,
    pub command: CommandId,
    pub principal_kind: PrincipalKind,
    /// `None` for the submission event (the job enters `queued`).
    pub from: Option<JobState>,
    pub to: JobState,
    pub error: Option<JobErrorCode>,
    /// Clock time of the change.
    pub at: Duration,
    /// Wall-clock time of the change, since the Unix epoch (UTC); the same reading the job
    /// record stores.
    pub wall: Duration,
}

/// Receives job lifecycle events, in order. The journal wires it to the Activity Log.
///
/// `emit` is called while the scheduler holds its lock, so events arrive in the order the
/// changes happened. It must return quickly and must not call back into [`Jobs`].
pub trait LifecycleSink: Send + Sync {
    fn emit(&self, event: &LifecycleEvent);
}

/// A sink that drops every event.
#[derive(Debug, Clone, Copy, Default)]
pub struct NullLifecycleSink;

impl LifecycleSink for NullLifecycleSink {
    fn emit(&self, _event: &LifecycleEvent) {}
}

// ------------------------------------------------------------------ internals

struct Entry<T> {
    record: JobRecord,
    progress: Arc<Progress>,
    timeout: Option<Duration>,
    handler: Option<Handler<T>>,
    result: Option<T>,
}

impl<T> Entry<T> {
    fn view(&self) -> JobRecord {
        let mut record = self.record.clone();
        record.progress = self.progress.latest();
        record
    }
}

struct State<T> {
    jobs: BTreeMap<JobId, Entry<T>>,
    interactive: VecDeque<JobId>,
    batch: VecDeque<JobId>,
    running: BTreeSet<JobId>,
    /// Worker threads whose handler has not returned yet.
    occupied: usize,
    /// Ended jobs, oldest first, for the retention cap (may hold forgotten IDs).
    ended: VecDeque<JobId>,
}

impl<T> State<T> {
    fn queued(&self) -> usize {
        self.interactive.len() + self.batch.len()
    }
}

struct Inner<T> {
    state: Mutex<State<T>>,
    changed: Condvar,
    sink: Arc<dyn LifecycleSink>,
    clock: Arc<dyn Clock>,
    wall_clock: Arc<dyn WallClock>,
    max_running: usize,
    queue_cap: usize,
    retain_ended: usize,
    id_seq: AtomicU64,
}

impl<T: Send + 'static> Inner<T> {
    fn emit(&self, record: &JobRecord, from: Option<JobState>, at: Duration, wall: Duration) {
        self.sink.emit(&LifecycleEvent {
            job: record.id,
            command: record.command.clone(),
            principal_kind: record.principal_kind,
            from,
            to: record.state,
            error: record.error,
            at,
            wall,
        });
    }

    /// Moves `entry` to `to` (with `error`), stamps the time and emits the event; the record's
    /// timestamp and the event's `at` are the same clock reading. An ended job is noted in
    /// `ended` for the retention cap.
    fn transition(&self, ended: &mut VecDeque<JobId>, entry: &mut Entry<T>, to: JobState, error: Option<JobErrorCode>) {
        let from = entry.record.state;
        let now = self.clock.now();
        let wall = self.wall_clock.unix_time();
        entry.record.state = to;
        entry.record.error = error;
        if to == JobState::Running {
            entry.record.started_at = Some(now);
            entry.record.started_wall = Some(wall);
        }
        if to.is_terminal() {
            entry.record.ended_at = Some(now);
            entry.record.ended_wall = Some(wall);
            ended.push_back(entry.record.id);
        }
        self.emit(&entry.record, Some(from), now, wall);
    }

    /// Drops the oldest-ended records beyond the retention cap.
    fn prune(&self, st: &mut State<T>) {
        while st.ended.len() > self.retain_ended {
            if let Some(old) = st.ended.pop_front() {
                st.jobs.remove(&old);
            }
        }
    }

    /// A fresh UUIDv7 not already used by a job of this scheduler.
    fn fresh_id(&self, jobs: &BTreeMap<JobId, Entry<T>>) -> JobId {
        loop {
            let unix_ms = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
            let unix_ms = unix_ms & ((1u128 << 48) - 1);
            let seq = self.id_seq.fetch_add(1, Ordering::Relaxed);
            let hi = random_u64(seq, 0x6a6f_625f_6869);
            let lo = random_u64(seq, 0x6a6f_625f_6c6f);
            let r = ((u128::from(hi) << 64) | u128::from(lo)) & ((1u128 << 74) - 1);
            let rand_a = r >> 62;
            let rand_b = r & ((1u128 << 62) - 1);
            let n = (unix_ms << 80) | (0x7u128 << 76) | (rand_a << 64) | (0b10u128 << 62) | rand_b;
            let id = JobId(Uuid::from_u128(n));
            if !jobs.contains_key(&id) {
                return id;
            }
        }
    }

    /// Starts queued jobs while a slot is free: interactive first, FIFO within a class.
    fn dispatch(self: &Arc<Self>, st: &mut State<T>) {
        while st.occupied < self.max_running {
            let Some(id) = st.interactive.pop_front().or_else(|| st.batch.pop_front()) else {
                break;
            };
            let Some(entry) = st.jobs.get_mut(&id) else {
                continue;
            };
            if entry.record.state != JobState::Queued {
                continue;
            }
            let Some(handler) = entry.handler.take() else {
                continue;
            };
            self.transition(&mut st.ended, entry, JobState::Running, None);
            let sink = entry.progress.sink();
            let token = entry.progress.cancel_token();
            let inner = Arc::clone(self);
            let spawned = std::thread::Builder::new().name("fid-jobs-worker".to_string()).spawn(move || {
                // A panicking handler must still free its slot and end its job.
                let outcome = catch_unwind(AssertUnwindSafe(|| handler(&sink, &token)))
                    .unwrap_or_else(|_| Err(HandlerError::Failed("handler panicked".to_string())));
                inner.finish(id, outcome);
            });
            match spawned {
                Ok(_detached) => {
                    st.running.insert(id);
                    st.occupied += 1;
                }
                Err(e) => {
                    entry.record.error_detail = Some(cut(&format!("worker thread could not start: {e}")));
                    self.transition(&mut st.ended, entry, JobState::Failed, Some(JobErrorCode::HandlerFailed));
                }
            }
        }
        self.prune(st);
    }

    /// Called by a worker when its handler returns.
    fn finish(self: &Arc<Self>, id: JobId, outcome: Result<T, HandlerError>) {
        let mut guard = lock(&self.state);
        let st = &mut *guard;
        st.occupied = st.occupied.saturating_sub(1);
        st.running.remove(&id);
        if let Some(entry) = st.jobs.get_mut(&id) {
            // A job that already ended (timeout) keeps its state; its output is dropped here.
            if entry.record.state == JobState::Running {
                if entry.progress.is_cancel_requested() {
                    // Cancellation wins over a late result: nothing partial reaches the committer.
                    self.transition(&mut st.ended, entry, JobState::Cancelled, Some(JobErrorCode::Cancelled));
                } else {
                    match outcome {
                        Ok(value) => {
                            entry.result = Some(value);
                            self.transition(&mut st.ended, entry, JobState::Succeeded, None);
                        }
                        Err(HandlerError::Failed(message)) => {
                            entry.record.error_detail = Some(cut(&message));
                            self.transition(&mut st.ended, entry, JobState::Failed, Some(JobErrorCode::HandlerFailed));
                        }
                        Err(HandlerError::Cancelled) => {
                            entry.record.error_detail =
                                Some("handler reported cancellation without a request".to_string());
                            self.transition(&mut st.ended, entry, JobState::Failed, Some(JobErrorCode::HandlerFailed));
                        }
                    }
                }
            }
        }
        self.dispatch(st);
        drop(guard);
        self.changed.notify_all();
    }

    fn check_timeouts(&self) -> usize {
        let mut st = lock(&self.state);
        let now = self.clock.now();
        let st = &mut *st;
        let mut fired = 0;
        for id in &st.running {
            let Some(entry) = st.jobs.get_mut(id) else {
                continue;
            };
            if entry.record.state != JobState::Running {
                continue;
            }
            let (Some(timeout), Some(started)) = (entry.timeout, entry.record.started_at) else {
                continue;
            };
            if now.saturating_sub(started) > timeout {
                // A job already asked to cancel ends `cancelled` (cancellation was first); any
                // other ends `failed`/`timeout`. Either way it stops being `running` here, even if
                // its handler ignores the token; the slot frees when the handler returns.
                let (to, code) = if entry.progress.is_cancel_requested() {
                    (JobState::Cancelled, JobErrorCode::Cancelled)
                } else {
                    (JobState::Failed, JobErrorCode::Timeout)
                };
                entry.progress.cancel();
                self.transition(&mut st.ended, entry, to, Some(code));
                fired += 1;
            }
        }
        self.prune(st);
        if fired > 0 {
            self.changed.notify_all();
        }
        fired
    }
}

fn random_u64(seq: u64, salt: u64) -> u64 {
    // RandomState keys are seeded from the OS once per thread and differ for every
    // RandomState; job IDs need uniqueness, not secrecy. Duplicates are also checked.
    let mut h = RandomState::new().build_hasher();
    h.write_u64(seq);
    h.write_u64(salt);
    h.finish()
}

fn cut(s: &str) -> String {
    s.chars().take(MAX_ERROR_DETAIL_CHARS).collect()
}

// ------------------------------------------------------------------ public scheduler

/// The job scheduler. `T` is the handlers' result type.
pub struct Jobs<T: Send + 'static> {
    inner: Arc<Inner<T>>,
    timer_stop: Arc<(Mutex<bool>, Condvar)>,
    timer: Option<JoinHandle<()>>,
}

impl<T: Send + 'static> fmt::Debug for Jobs<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Jobs")
            .field("max_running", &self.inner.max_running)
            .field("queue_cap", &self.inner.queue_cap)
            .finish_non_exhaustive()
    }
}

impl<T: Send + 'static> Jobs<T> {
    /// A scheduler emitting lifecycle events to `sink`. Fails only if the timeout thread
    /// cannot be started.
    pub fn new(config: JobsConfig, sink: Arc<dyn LifecycleSink>) -> std::io::Result<Self> {
        let inner = Arc::new(Inner {
            state: Mutex::new(State {
                jobs: BTreeMap::new(),
                interactive: VecDeque::new(),
                batch: VecDeque::new(),
                running: BTreeSet::new(),
                occupied: 0,
                ended: VecDeque::new(),
            }),
            changed: Condvar::new(),
            sink,
            clock: config.clock,
            wall_clock: config.wall_clock,
            max_running: config.max_running.max(1),
            queue_cap: config.queue_cap,
            retain_ended: config.retain_ended.max(1),
            id_seq: AtomicU64::new(0),
        });
        let timer_stop = Arc::new((Mutex::new(false), Condvar::new()));
        let timer = match config.timer_poll {
            None => None,
            Some(poll) => {
                let poll = poll.max(MIN_TIMER_POLL);
                let stop = Arc::clone(&timer_stop);
                let inner = Arc::clone(&inner);
                Some(std::thread::Builder::new().name("fid-jobs-timer".to_string()).spawn(move || {
                    let (flag, cv) = &*stop;
                    loop {
                        inner.check_timeouts();
                        let stopped = lock(flag);
                        if *stopped {
                            break;
                        }
                        let (stopped, _) = cv.wait_timeout(stopped, poll).unwrap_or_else(PoisonError::into_inner);
                        if *stopped {
                            break;
                        }
                    }
                })?)
            }
        };
        Ok(Jobs { inner, timer_stop, timer })
    }

    /// The concurrency limit in force.
    pub fn max_running(&self) -> usize {
        self.inner.max_running
    }

    /// Submits `exec` and returns its job ID at once; the job starts when a slot is free.
    /// Refused with `queue_full` (and no job created) when no slot is free and the queue is
    /// at its cap.
    pub fn submit(&self, exec: Execution<T>, opts: JobOpts) -> Result<JobId, JobErrorCode> {
        let mut st = lock(&self.inner.state);
        let would_wait = st.occupied >= self.inner.max_running || st.queued() > 0;
        if would_wait && st.queued() >= self.inner.queue_cap {
            return Err(JobErrorCode::QueueFull);
        }
        let id = self.inner.fresh_id(&st.jobs);
        let progress = Arc::new(Progress::new(Arc::clone(&self.inner.clock)));
        let now = self.inner.clock.now();
        let record = JobRecord {
            id,
            command: exec.command,
            principal_kind: opts.principal,
            class: opts.class,
            state: JobState::Queued,
            progress: progress.latest(),
            submitted_at: now,
            started_at: None,
            ended_at: None,
            submitted_wall: self.inner.wall_clock.unix_time(),
            started_wall: None,
            ended_wall: None,
            error: None,
            error_detail: None,
        };
        self.inner.emit(&record, None, now, record.submitted_wall);
        st.jobs
            .insert(id, Entry { record, progress, timeout: opts.timeout, handler: Some(exec.handler), result: None });
        match opts.class {
            JobClass::Interactive => st.interactive.push_back(id),
            JobClass::Batch => st.batch.push_back(id),
        }
        self.inner.dispatch(&mut st);
        drop(st);
        self.inner.changed.notify_all();
        Ok(id)
    }

    /// The job's current record.
    pub fn status(&self, id: JobId) -> Result<JobRecord, JobErrorCode> {
        lock(&self.inner.state).jobs.get(&id).map(Entry::view).ok_or(JobErrorCode::NotFound)
    }

    /// Records of all known jobs, in ID order (which is submission time order to the ms).
    pub fn list(&self) -> Vec<JobRecord> {
        lock(&self.inner.state).jobs.values().map(Entry::view).collect()
    }

    /// The job's progress state, to subscribe listeners or spawn a publisher.
    pub fn progress(&self, id: JobId) -> Result<Arc<Progress>, JobErrorCode> {
        lock(&self.inner.state).jobs.get(&id).map(|e| Arc::clone(&e.progress)).ok_or(JobErrorCode::NotFound)
    }

    /// Requests cancellation and returns the job's state afterwards.
    ///
    /// A queued job becomes `cancelled` at once and its handler never runs
    /// (REQ-CORE.JOB.SCHED-007). A running job has its [`CancelToken`] set and becomes
    /// `cancelled` when its handler returns. An ended job is left as it is.
    pub fn cancel(&self, id: JobId) -> Result<JobState, JobErrorCode> {
        self.cancel_request(id).map(|r| r.state)
    }

    /// [`Jobs::cancel`], also reporting whether this call newly requested the cancellation.
    /// Decided atomically under the scheduler's lock (and, for a running job, by an atomic
    /// swap of its cancellation flag), so of any number of concurrent requests at most one
    /// reports `newly_requested`, and an ended job never does.
    pub fn cancel_request(&self, id: JobId) -> Result<CancelRequest, JobErrorCode> {
        let mut st = lock(&self.inner.state);
        let st = &mut *st;
        let entry = st.jobs.get_mut(&id).ok_or(JobErrorCode::NotFound)?;
        match entry.record.state {
            JobState::Queued => {
                entry.progress.cancel();
                entry.handler = None;
                self.inner.transition(&mut st.ended, entry, JobState::Cancelled, Some(JobErrorCode::Cancelled));
                match entry.record.class {
                    JobClass::Interactive => st.interactive.retain(|j| *j != id),
                    JobClass::Batch => st.batch.retain(|j| *j != id),
                }
                self.inner.prune(st);
                self.inner.changed.notify_all();
                Ok(CancelRequest { state: JobState::Cancelled, newly_requested: true })
            }
            JobState::Running => {
                let newly_requested = entry.progress.request_cancel();
                Ok(CancelRequest { state: JobState::Running, newly_requested })
            }
            ended => Ok(CancelRequest { state: ended, newly_requested: false }),
        }
    }

    /// Waits until the job has ended or `deadline` (real time) has passed, and returns its
    /// record either way; the caller checks `state`.
    pub fn wait(&self, id: JobId, deadline: Duration) -> Result<JobRecord, JobErrorCode> {
        let start = Instant::now();
        let mut st = lock(&self.inner.state);
        loop {
            let record = st.jobs.get(&id).map(Entry::view).ok_or(JobErrorCode::NotFound)?;
            let elapsed = start.elapsed();
            if record.state.is_terminal() || elapsed >= deadline {
                return Ok(record);
            }
            let (next, _) =
                self.inner.changed.wait_timeout(st, deadline - elapsed).unwrap_or_else(PoisonError::into_inner);
            st = next;
        }
    }

    /// [`Jobs::wait`] that also returns, within [`WAIT_CANCEL_POLL`], once `call` is
    /// cancelled (the caller's own request was cancelled). The record is returned either
    /// way; the caller checks `call` and the state.
    pub fn wait_cancellable(
        &self,
        id: JobId,
        deadline: Duration,
        call: &CancelToken,
    ) -> Result<JobRecord, JobErrorCode> {
        let start = Instant::now();
        let mut st = lock(&self.inner.state);
        loop {
            let record = st.jobs.get(&id).map(Entry::view).ok_or(JobErrorCode::NotFound)?;
            let elapsed = start.elapsed();
            if record.state.is_terminal() || elapsed >= deadline || call.is_cancelled() {
                return Ok(record);
            }
            let slice = (deadline - elapsed).min(WAIT_CANCEL_POLL);
            let (next, _) = self.inner.changed.wait_timeout(st, slice).unwrap_or_else(PoisonError::into_inner);
            st = next;
        }
    }

    /// The result of a `succeeded` job, handed out once. `None` for any other state, for a
    /// result already taken, and for an unknown ID: a failed or cancelled job hands nothing
    /// to the committer (REQ-CORE.JOB.SCHED-006).
    pub fn take_result(&self, id: JobId) -> Option<T> {
        let mut st = lock(&self.inner.state);
        let entry = st.jobs.get_mut(&id)?;
        if entry.record.state == JobState::Succeeded {
            entry.result.take()
        } else {
            None
        }
    }

    /// Ends every running job past its timeout, at the clock's current time: `failed` with
    /// `timeout`, or `cancelled` if cancellation was already requested. Returns how many.
    /// The timeout thread calls this; tests with a manual clock call it
    /// themselves.
    pub fn check_timeouts(&self) -> usize {
        self.inner.check_timeouts()
    }

    /// Drops the record of an ended job (and any untaken result); returns whether it did.
    pub fn forget(&self, id: JobId) -> bool {
        let mut st = lock(&self.inner.state);
        let ended = st.jobs.get(&id).is_some_and(|e| e.record.state.is_terminal());
        if ended {
            st.jobs.remove(&id);
            st.ended.retain(|j| *j != id);
        }
        ended
    }
}

impl<T: Send + 'static> Drop for Jobs<T> {
    /// Stops the timeout thread, cancels queued jobs and requests cancellation of running ones.
    /// Does not wait for running handlers.
    fn drop(&mut self) {
        {
            let (flag, cv) = &*self.timer_stop;
            *lock(flag) = true;
            cv.notify_all();
        }
        if let Some(handle) = self.timer.take() {
            // The timer body cannot panic; a join error would carry nothing to act on.
            let _ = handle.join();
        }
        let mut st = lock(&self.inner.state);
        let st = &mut *st;
        let queued: Vec<JobId> = st.interactive.drain(..).chain(st.batch.drain(..)).collect();
        for id in queued {
            if let Some(entry) = st.jobs.get_mut(&id) {
                if entry.record.state == JobState::Queued {
                    entry.progress.cancel();
                    entry.handler = None;
                    self.inner.transition(&mut st.ended, entry, JobState::Cancelled, Some(JobErrorCode::Cancelled));
                }
            }
        }
        for id in &st.running {
            if let Some(entry) = st.jobs.get(id) {
                entry.progress.cancel();
            }
        }
        self.inner.changed.notify_all();
    }
}
