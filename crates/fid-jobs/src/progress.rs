//! Progress sink and cancellation token (spec node `CORE.JOB.PROG`).
//!
//! A running handler gets two one-way handles:
//!
//! - [`ProgressSink`]: `report(fraction, stage)` writes into a last-value cell and returns unit;
//! - [`CancelToken`]: `is_cancelled()` reads an atomic flag. It is the only information a
//!   handler can learn (REQ-CORE.JOB.PROG-006), so its result cannot depend on listeners
//!   (REQ-CORE.JOB.PROG-007).
//!
//! The job side owns a [`Progress`]. [`Progress::tick`] is the publisher step: it reads the cell
//! and decides, from an injectable [`Clock`], whether to publish to the listeners:
//!
//! - a new report is published when at least [`MIN_PUBLISH_INTERVAL`] (50 ms) has passed since
//!   the previous publication, so no 1 s window holds more than 20 publications
//!   (REQ-CORE.JOB.PROG-003);
//! - otherwise the last value is republished as a heartbeat once [`HEARTBEAT_INTERVAL`] has
//!   passed (REQ-CORE.JOB.PROG-002).
//!
//! [`Progress::spawn_publisher`] runs `tick` on a background thread. Tests drive `tick` by hand
//! with a [`ManualClock`], which makes rate limits and heartbeats deterministic.
//!
//! Reports with a NaN, negative or greater-than-one fraction are dropped and recorded as a
//! [`ProgressWarning`] (REQ-CORE.JOB.PROG-004). Stage labels are cut to [`MAX_STAGE_CHARS`]
//! characters (REQ-CORE.JOB.PROG-001). Nothing here panics; a poisoned lock is recovered,
//! since every critical section leaves its data consistent.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// Longest stage label kept, in characters (Unicode scalar values) (REQ-CORE.JOB.PROG-001).
pub const MAX_STAGE_CHARS: usize = 128;
/// Shortest time between two publications: at most 20 per second (REQ-CORE.JOB.PROG-003),
/// counted over half-open 1 s windows `[t, t + 1 s)` of publication times.
pub const MIN_PUBLISH_INTERVAL: Duration = Duration::from_millis(50);
/// Time without a publication after which the last value is republished.
///
/// Set below 1 s so that, with the publisher's polling interval and thread scheduling on
/// top, listeners still see a publication at least once per second (REQ-CORE.JOB.PROG-002).
pub const HEARTBEAT_INTERVAL: Duration = Duration::from_millis(900);
/// Polling interval of the background publisher thread.
pub const DEFAULT_POLL_INTERVAL: Duration = Duration::from_millis(10);
/// Warnings kept per job; later ones are only counted.
pub const MAX_RECORDED_WARNINGS: usize = 64;

/// Shortest polling interval accepted by [`Progress::spawn_publisher`], so it cannot spin.
const MIN_POLL_INTERVAL: Duration = Duration::from_millis(1);

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

// ------------------------------------------------------------------ clocks

/// Monotonic time source for the publisher: time elapsed since an arbitrary origin.
pub trait Clock: Send + Sync {
    /// Current time since the clock's origin.
    fn now(&self) -> Duration;
}

/// Real monotonic time ([`Instant`]).
#[derive(Debug, Clone, Copy)]
pub struct SystemClock {
    origin: Instant,
}

impl SystemClock {
    /// A clock whose origin is now.
    pub fn new() -> Self {
        SystemClock { origin: Instant::now() }
    }
}

impl Default for SystemClock {
    fn default() -> Self {
        Self::new()
    }
}

impl Clock for SystemClock {
    fn now(&self) -> Duration {
        self.origin.elapsed()
    }
}

/// A clock that only moves when told to; for deterministic tests.
#[derive(Debug, Default)]
pub struct ManualClock {
    now: Mutex<Duration>,
}

impl ManualClock {
    /// A clock at time zero.
    pub fn new() -> Self {
        Self::default()
    }

    /// Moves the clock forward by `by` (saturating).
    pub fn advance(&self, by: Duration) {
        let mut now = lock(&self.now);
        *now = now.saturating_add(by);
    }

    /// Sets the clock to `to`.
    pub fn set(&self, to: Duration) {
        *lock(&self.now) = to;
    }
}

impl Clock for ManualClock {
    fn now(&self) -> Duration {
        *lock(&self.now)
    }
}

// ------------------------------------------------------------------ data

/// Why a report was ignored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WarningKind {
    /// The fraction was NaN.
    NotANumber,
    /// The fraction was below 0 (including negative infinity).
    Negative,
    /// The fraction was above 1 (including positive infinity).
    AboveOne,
}

/// A report the sink ignored (REQ-CORE.JOB.PROG-004).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ProgressWarning {
    pub kind: WarningKind,
    /// The rejected fraction, as reported.
    pub fraction: f64,
}

/// One publication to listeners.
#[derive(Debug, Clone, PartialEq)]
pub struct ProgressUpdate {
    /// Fraction complete, finite and in [0, 1].
    pub fraction: f64,
    /// Stage label, at most [`MAX_STAGE_CHARS`] characters.
    pub stage: String,
    /// Number of accepted reports so far; 0 before the handler's first report.
    pub report_seq: u64,
    /// True when this republishes a value listeners have already been sent.
    pub heartbeat: bool,
    /// Publisher clock time of the publication. Observational only, never recorded.
    pub at: Duration,
}

/// The latest accepted report, read without publishing (for job status queries).
#[derive(Debug, Clone, PartialEq)]
pub struct ProgressSnapshot {
    /// Fraction complete, finite and in [0, 1]; 0 before the handler's first report.
    pub fraction: f64,
    /// Stage label, at most [`MAX_STAGE_CHARS`] characters.
    pub stage: String,
    /// Number of accepted reports so far.
    pub report_seq: u64,
}

#[derive(Debug)]
struct Last {
    fraction: f64,
    stage: String,
    seq: u64,
}

#[derive(Debug)]
struct Shared {
    last: Mutex<Last>,
    cancelled: AtomicBool,
    warnings: Mutex<Vec<ProgressWarning>>,
    warning_count: AtomicU64,
}

impl Shared {
    fn warn(&self, warning: ProgressWarning) {
        self.warning_count.fetch_add(1, Ordering::Relaxed);
        let mut warnings = lock(&self.warnings);
        if warnings.len() < MAX_RECORDED_WARNINGS {
            warnings.push(warning);
        }
    }
}

// ------------------------------------------------------------------ handler handles

/// Handler-side progress handle. Cheap to clone; `report` returns nothing.
#[derive(Clone)]
pub struct ProgressSink {
    shared: Arc<Shared>,
}

impl ProgressSink {
    /// Records `fraction` (in [0, 1]) and `stage` (cut to [`MAX_STAGE_CHARS`] characters) as the
    /// latest progress. A NaN, negative or greater-than-one fraction is ignored and recorded as a
    /// warning; the previous value stays.
    pub fn report(&self, fraction: f64, stage: &str) {
        let rejected = if fraction.is_nan() {
            Some(WarningKind::NotANumber)
        } else if fraction < 0.0 {
            Some(WarningKind::Negative)
        } else if fraction > 1.0 {
            Some(WarningKind::AboveOne)
        } else {
            None
        };
        if let Some(kind) = rejected {
            self.shared.warn(ProgressWarning { kind, fraction });
            return;
        }
        // -0.0 + 0.0 is +0.0; listeners never see a negative zero.
        let fraction = fraction + 0.0;
        let mut last = lock(&self.shared.last);
        last.fraction = fraction;
        last.stage.clear();
        last.stage.extend(stage.chars().take(MAX_STAGE_CHARS));
        last.seq = last.seq.wrapping_add(1);
    }
}

/// Handler-side cancellation handle: an atomic flag the handler checks at its checkpoints.
#[derive(Clone)]
pub struct CancelToken {
    shared: Arc<Shared>,
}

impl CancelToken {
    /// True once cancellation has been requested for this job.
    pub fn is_cancelled(&self) -> bool {
        self.shared.cancelled.load(Ordering::Acquire)
    }
}

// Debug prints only the type name: the handles give a handler no information beyond the
// cancelled flag (REQ-CORE.JOB.PROG-006).
impl std::fmt::Debug for ProgressSink {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ProgressSink")
    }
}

impl std::fmt::Debug for CancelToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("CancelToken")
    }
}

// ------------------------------------------------------------------ job side

#[derive(Debug)]
struct PublisherState {
    last_publish: Option<Duration>,
    published_seq: u64,
    listeners: Vec<Sender<ProgressUpdate>>,
}

/// Job-side progress state of one job: hands out the handler's [`ProgressSink`] and
/// [`CancelToken`], requests cancellation, and publishes progress to listeners.
pub struct Progress {
    shared: Arc<Shared>,
    clock: Arc<dyn Clock>,
    publisher: Mutex<PublisherState>,
}

impl std::fmt::Debug for Progress {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Progress").field("shared", &self.shared).field("publisher", &self.publisher).finish()
    }
}

impl Progress {
    /// Progress state for one job, timed by `clock`. Before the handler's first report the
    /// published value is fraction 0 with an empty stage.
    pub fn new(clock: Arc<dyn Clock>) -> Self {
        Progress {
            shared: Arc::new(Shared {
                last: Mutex::new(Last { fraction: 0.0, stage: String::new(), seq: 0 }),
                cancelled: AtomicBool::new(false),
                warnings: Mutex::new(Vec::new()),
                warning_count: AtomicU64::new(0),
            }),
            clock,
            publisher: Mutex::new(PublisherState { last_publish: None, published_seq: 0, listeners: Vec::new() }),
        }
    }

    /// Progress state timed by a [`SystemClock`].
    pub fn with_system_clock() -> Self {
        Self::new(Arc::new(SystemClock::new()))
    }

    /// The handler's progress handle.
    pub fn sink(&self) -> ProgressSink {
        ProgressSink { shared: Arc::clone(&self.shared) }
    }

    /// The handler's cancellation handle.
    pub fn cancel_token(&self) -> CancelToken {
        CancelToken { shared: Arc::clone(&self.shared) }
    }

    /// Requests cancellation; the handler sees it at its next checkpoint.
    pub fn cancel(&self) {
        self.shared.cancelled.store(true, Ordering::Release);
    }

    /// True once [`Progress::cancel`] has been called.
    pub fn is_cancel_requested(&self) -> bool {
        self.shared.cancelled.load(Ordering::Acquire)
    }

    /// Adds a listener. Publications from now on are sent to it; a listener whose receiver
    /// has been dropped is removed at the next publication.
    pub fn subscribe(&self) -> Receiver<ProgressUpdate> {
        let (tx, rx) = channel();
        lock(&self.publisher).listeners.push(tx);
        rx
    }

    /// Number of listeners still attached (as of the last publication).
    pub fn listener_count(&self) -> usize {
        lock(&self.publisher).listeners.len()
    }

    /// The latest accepted report. Reading it publishes nothing and does not affect
    /// [`Progress::tick`].
    pub fn latest(&self) -> ProgressSnapshot {
        let last = lock(&self.shared.last);
        ProgressSnapshot { fraction: last.fraction, stage: last.stage.clone(), report_seq: last.seq }
    }

    /// Ignored reports kept so far (at most [`MAX_RECORDED_WARNINGS`]).
    pub fn warnings(&self) -> Vec<ProgressWarning> {
        lock(&self.shared.warnings).clone()
    }

    /// Total number of ignored reports, including those not kept.
    pub fn warning_count(&self) -> u64 {
        self.shared.warning_count.load(Ordering::Relaxed)
    }

    /// One publisher step at the clock's current time. Publishes, and returns, an update when
    /// none has been published yet; when a new report is waiting and [`MIN_PUBLISH_INTERVAL`]
    /// has passed since the last publication; or, as a heartbeat repeating the last value,
    /// when [`HEARTBEAT_INTERVAL`] has passed. Returns `None` otherwise.
    pub fn tick(&self) -> Option<ProgressUpdate> {
        // Read the time under the publisher lock, so publication times are ordered and spaced
        // exactly as the decisions below see them.
        let mut state = lock(&self.publisher);
        let now = self.clock.now();
        let update = {
            let last = lock(&self.shared.last);
            let fresh = last.seq != state.published_seq;
            let heartbeat = match state.last_publish {
                None => false,
                Some(prev) => {
                    let since = now.saturating_sub(prev);
                    if fresh && since >= MIN_PUBLISH_INTERVAL {
                        false
                    } else if since >= HEARTBEAT_INTERVAL {
                        !fresh
                    } else {
                        return None;
                    }
                }
            };
            ProgressUpdate {
                fraction: last.fraction,
                stage: last.stage.clone(),
                report_seq: last.seq,
                heartbeat,
                at: now,
            }
        };
        state.last_publish = Some(now);
        state.published_seq = update.report_seq;
        state.listeners.retain(|tx| tx.send(update.clone()).is_ok());
        Some(update)
    }

    /// Starts a background thread that calls [`Progress::tick`] every `poll` (at least 1 ms)
    /// until the returned [`Publisher`] is stopped or dropped.
    pub fn spawn_publisher(self: &Arc<Self>, poll: Duration) -> std::io::Result<Publisher> {
        let poll = poll.max(MIN_POLL_INTERVAL);
        let stop = Arc::new((Mutex::new(false), Condvar::new()));
        let thread_stop = Arc::clone(&stop);
        let progress = Arc::clone(self);
        let handle = std::thread::Builder::new().name("fid-jobs-progress".to_string()).spawn(move || {
            let (flag, cv) = &*thread_stop;
            loop {
                progress.tick();
                let stopped = lock(flag);
                if *stopped {
                    break;
                }
                let (stopped, _) = cv.wait_timeout(stopped, poll).unwrap_or_else(PoisonError::into_inner);
                if *stopped {
                    break;
                }
            }
        })?;
        Ok(Publisher { stop, handle: Some(handle) })
    }
}

/// Handle to a background publisher thread. Stopping (or dropping) it ends the thread and
/// waits for it.
#[derive(Debug)]
pub struct Publisher {
    stop: Arc<(Mutex<bool>, Condvar)>,
    handle: Option<JoinHandle<()>>,
}

impl Publisher {
    /// Stops the thread and waits for it to finish.
    pub fn stop(mut self) {
        self.shutdown();
    }

    fn shutdown(&mut self) {
        let (flag, cv) = &*self.stop;
        *lock(flag) = true;
        cv.notify_all();
        if let Some(handle) = self.handle.take() {
            // The thread body cannot panic; a join error would carry nothing to act on.
            let _ = handle.join();
        }
    }
}

impl Drop for Publisher {
    fn drop(&mut self) {
        self.shutdown();
    }
}
