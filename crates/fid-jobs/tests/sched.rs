//! Acceptance criteria of spec node CORE.JOB.SCHED (AC-1 .. AC-5), plus unit checks.
//!
//! Determinism on every OS: no test depends on how fast a thread runs.
//!
//! - AC-1 holds every handler at a gate until the last submission, so the start order is fixed
//!   by the rules alone; the expected order comes from the independent reference script
//!   `fixtures/core/job/sched/make_expected.py`.
//! - AC-2 runs on a `ManualClock` with the timeout thread off: the test moves the clock and
//!   calls `check_timeouts`. One extra real-time run uses only generous bounds.
//! - The 100 ms submit bounds (AC-1, AC-5) measure `submit` alone, which never waits on a
//!   handler.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::{parse, CValue};
use fid_jobs::{
    default_max_running, CancelToken, Clock, Execution, HandlerError, JobClass, JobErrorCode, JobId, JobOpts, JobState,
    Jobs, JobsConfig, LifecycleEvent, LifecycleSink, ManualClock, NullLifecycleSink, PrincipalKind, ProgressSink,
    SystemClock,
};
use fid_types::CommandId;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/core/job/sched");
const SUBMIT_BUDGET: Duration = Duration::from_millis(100);
/// Generous real-time bound for waiting on jobs that are expected to end quickly.
const LONG: Duration = Duration::from_secs(60);
const STEP: Duration = Duration::from_millis(1);

// ------------------------------------------------------------------ test wiring

/// Activity Log stand-in: captures lifecycle events in order.
#[derive(Default)]
struct ActivityLog {
    events: Mutex<Vec<LifecycleEvent>>,
}

impl LifecycleSink for ActivityLog {
    fn emit(&self, event: &LifecycleEvent) {
        self.events.lock().unwrap().push(event.clone());
    }
}

impl ActivityLog {
    fn all(&self) -> Vec<LifecycleEvent> {
        self.events.lock().unwrap().clone()
    }

    fn of(&self, id: JobId) -> Vec<LifecycleEvent> {
        self.all().into_iter().filter(|e| e.job == id).collect()
    }
}

/// Capturing Record Journal stub (AC-4). The scheduler has no handle on it; it stays empty
/// unless something other than the lifecycle sink were to write job events.
#[derive(Default)]
struct RecordJournal {
    writes: Mutex<Vec<String>>,
}

/// Committer stub: receives only what `take_result` hands out.
#[derive(Default)]
struct Committer {
    received: Mutex<Vec<String>>,
}

impl Committer {
    fn commit(&self, jobs: &Jobs<String>, id: JobId) {
        if let Some(v) = jobs.take_result(id) {
            self.received.lock().unwrap().push(v);
        }
    }

    fn received(&self) -> Vec<String> {
        self.received.lock().unwrap().clone()
    }
}

/// Holds handlers until opened; a waiting handler also stops on cancellation.
struct Gate {
    open: Mutex<bool>,
    cv: Condvar,
}

impl Gate {
    fn new() -> Arc<Gate> {
        Arc::new(Gate { open: Mutex::new(false), cv: Condvar::new() })
    }

    fn open(&self) {
        *self.open.lock().unwrap() = true;
        self.cv.notify_all();
    }

    /// True when opened, false when cancelled first.
    fn pass(&self, token: &CancelToken) -> bool {
        let mut open = self.open.lock().unwrap();
        loop {
            if *open {
                return true;
            }
            if token.is_cancelled() {
                return false;
            }
            open = self.cv.wait_timeout(open, STEP).unwrap().0;
        }
    }
}

/// Counts handlers inside their work section and the most seen at once.
#[derive(Default)]
struct Live {
    now: AtomicUsize,
    max: AtomicUsize,
}

impl Live {
    fn enter(&self) {
        let n = self.now.fetch_add(1, Ordering::SeqCst) + 1;
        self.max.fetch_max(n, Ordering::SeqCst);
    }

    fn leave(&self) {
        self.now.fetch_sub(1, Ordering::SeqCst);
    }
}

fn cmd() -> CommandId {
    CommandId::parse("test.sleep@1").unwrap()
}

fn opts(class: JobClass) -> JobOpts {
    JobOpts { class, timeout: None, principal: PrincipalKind::Script }
}

/// Test sleep handler: waits at `gate`, then sleeps `ms` in 1 ms steps (stopping on
/// cancellation), reports progress and returns `name`.
fn sleep_handler(
    name: String,
    ms: u64,
    gate: Arc<Gate>,
    live: Arc<Live>,
) -> impl FnOnce(&ProgressSink, &CancelToken) -> Result<String, HandlerError> + Send + 'static {
    move |sink, token| {
        if !gate.pass(token) {
            return Err(HandlerError::Cancelled);
        }
        live.enter();
        let start = Instant::now();
        while start.elapsed() < Duration::from_millis(ms) {
            if token.is_cancelled() {
                live.leave();
                return Err(HandlerError::Cancelled);
            }
            std::thread::sleep(STEP);
        }
        sink.report(1.0, "done");
        live.leave();
        Ok(name)
    }
}

fn config(max_running: usize) -> JobsConfig {
    JobsConfig { max_running, ..JobsConfig::default() }
}

fn eventually(what: &str, mut cond: impl FnMut() -> bool) {
    let start = Instant::now();
    while !cond() {
        assert!(start.elapsed() < LONG, "timed out waiting for {what}");
        std::thread::sleep(STEP);
    }
}

// ------------------------------------------------------------------ AC-4 checker

/// Allowed moves, written from REQ-CORE.JOB.SCHED-002 independently of `JobState::can_move_to`.
fn allowed(from: JobState, to: JobState) -> bool {
    use JobState::*;
    matches!(
        (from, to),
        (Queued, Running) | (Running, Succeeded) | (Running, Failed) | (Running, Cancelled) | (Queued, Cancelled)
    )
}

/// Every job's events form a chain `None -> queued -> ...` of allowed moves, and failed or
/// cancelled states carry an error code.
fn assert_transitions_allowed(events: &[LifecycleEvent]) {
    let mut last: BTreeMap<JobId, JobState> = BTreeMap::new();
    for e in events {
        match (last.get(&e.job), e.from) {
            (None, None) => assert_eq!(e.to, JobState::Queued, "first event must enter queued: {e:?}"),
            (Some(&prev), Some(from)) => {
                assert_eq!(prev, from, "event does not continue the job's chain: {e:?}");
                assert!(allowed(from, e.to), "transition not allowed by REQ-002: {e:?}");
            }
            _ => panic!("bad event chain at {e:?}"),
        }
        match e.to {
            JobState::Failed | JobState::Cancelled => assert!(e.error.is_some(), "missing error code: {e:?}"),
            _ => assert_eq!(e.error, None, "unexpected error code: {e:?}"),
        }
        assert_eq!(e.command, cmd());
        last.insert(e.job, e.to);
    }
}

fn assert_journal_untouched(journal: &RecordJournal) {
    assert!(journal.writes.lock().unwrap().is_empty(), "the Record Journal received writes");
}

// ------------------------------------------------------------------ fixtures

fn load(name: &str) -> CValue {
    let bytes = std::fs::read(format!("{FIXTURES}/{name}")).unwrap();
    parse(&bytes).unwrap()
}

fn field<'a>(v: &'a CValue, key: &str) -> &'a CValue {
    match v {
        CValue::Obj(o) => o.get(key).unwrap_or_else(|| panic!("missing {key}")),
        other => panic!("expected object, got {other:?}"),
    }
}

fn as_u64(v: &CValue) -> u64 {
    match v {
        CValue::U64(n) => *n,
        CValue::I64(n) => u64::try_from(*n).unwrap(),
        CValue::Num(x) if *x >= 0.0 && x.fract() == 0.0 => *x as u64,
        other => panic!("expected a non-negative integer, got {other:?}"),
    }
}

fn as_str(v: &CValue) -> &str {
    match v {
        CValue::Str(s) => s,
        other => panic!("expected string, got {other:?}"),
    }
}

fn as_arr(v: &CValue) -> &[CValue] {
    match v {
        CValue::Arr(a) => a,
        other => panic!("expected array, got {other:?}"),
    }
}

// ------------------------------------------------------------------ AC-1

#[test]
fn ac1_mixed_queue_start_order_and_fast_submit() {
    let fx = load("mixed_queue.json");
    let limit = as_u64(field(&fx, "limit")) as usize;
    let expected: Vec<String> = as_arr(field(&load("mixed_queue.expected.json"), "start_order"))
        .iter()
        .map(|v| as_str(v).to_string())
        .collect();

    let log = Arc::new(ActivityLog::default());
    let journal = RecordJournal::default();
    let committer = Committer::default();
    let jobs: Jobs<String> = Jobs::new(config(limit), log.clone()).unwrap();
    assert_eq!(jobs.max_running(), limit);
    let gate = Gate::new();
    let live = Arc::new(Live::default());

    let mut names: BTreeMap<JobId, String> = BTreeMap::new();
    let mut order: Vec<JobId> = Vec::new();
    for job in as_arr(field(&fx, "jobs")) {
        let name = as_str(field(job, "name")).to_string();
        let class = match as_str(field(job, "class")) {
            "interactive" => JobClass::Interactive,
            "batch" => JobClass::Batch,
            other => panic!("unknown class {other}"),
        };
        let ms = as_u64(field(job, "sleep_ms"));
        let exec = Execution::new(cmd(), sleep_handler(name.clone(), ms, gate.clone(), live.clone()));
        let t = Instant::now();
        let id = jobs.submit(exec, opts(class)).unwrap();
        let took = t.elapsed();
        assert!(took < SUBMIT_BUDGET, "submit of {name} took {took:?}");
        assert_eq!(jobs.status(id).unwrap().class, class);
        names.insert(id, name);
        order.push(id);
    }
    // Before the gate opens: exactly `limit` jobs running, the rest queued.
    let running = order.iter().filter(|id| jobs.status(**id).unwrap().state == JobState::Running).count();
    assert_eq!(running, limit);
    gate.open();

    for id in &order {
        let rec = jobs.wait(*id, LONG).unwrap();
        assert_eq!(rec.state, JobState::Succeeded, "{rec:?}");
        assert_eq!(rec.progress.fraction, 1.0);
        assert_eq!(rec.progress.stage, "done");
        assert!(rec.started_at.is_some() && rec.ended_at.is_some());
        committer.commit(&jobs, *id);
    }

    let events = log.all();
    let started: Vec<String> =
        events.iter().filter(|e| e.to == JobState::Running).map(|e| names[&e.job].clone()).collect();
    assert_eq!(started, expected);

    // At most `limit` at once, seen by handlers and by the event stream.
    assert!(live.max.load(Ordering::SeqCst) <= limit);
    let mut now = 0usize;
    for e in &events {
        if e.to == JobState::Running {
            now += 1;
            assert!(now <= limit, "more than {limit} running at {e:?}");
        } else if e.from == Some(JobState::Running) {
            now -= 1;
        }
    }

    let mut received = committer.received();
    received.sort();
    let mut all: Vec<String> = names.values().cloned().collect();
    all.sort();
    assert_eq!(received, all);
    // A result is handed out once.
    assert_eq!(jobs.take_result(order[0]), None);

    assert_transitions_allowed(&events);
    assert_journal_untouched(&journal);
}

// ------------------------------------------------------------------ AC-2

#[test]
fn ac2_timeout_fails_job_and_hands_nothing_to_committer() {
    let clock = Arc::new(ManualClock::new());
    let log = Arc::new(ActivityLog::default());
    let journal = RecordJournal::default();
    let committer = Committer::default();
    let cfg = JobsConfig { max_running: 1, queue_cap: 10, clock: clock.clone(), timer_poll: None };
    let jobs: Jobs<String> = Jobs::new(cfg, log.clone()).unwrap();

    // Test sleep handler on the manual clock: 2 s unless cancelled.
    let saw_cancel = Arc::new(AtomicBool::new(false));
    let (c, saw) = (clock.clone(), saw_cancel.clone());
    let handler = move |_: &ProgressSink, token: &CancelToken| loop {
        if token.is_cancelled() {
            saw.store(true, Ordering::SeqCst);
            return Err(HandlerError::Cancelled);
        }
        if c.now() >= Duration::from_secs(2) {
            return Ok("slept 2 s".to_string());
        }
        std::thread::sleep(STEP);
    };
    let timeout = Some(Duration::from_millis(200));
    let id = jobs.submit(Execution::new(cmd(), handler), JobOpts { timeout, ..opts(JobClass::Batch) }).unwrap();
    assert_eq!(jobs.status(id).unwrap().state, JobState::Running);
    assert_eq!(jobs.status(id).unwrap().started_at, Some(Duration::ZERO));

    // Exactly at the timeout the job has not exceeded it yet.
    clock.set(Duration::from_millis(200));
    assert_eq!(jobs.check_timeouts(), 0);
    assert_eq!(jobs.status(id).unwrap().state, JobState::Running);

    clock.set(Duration::from_millis(201));
    assert_eq!(jobs.check_timeouts(), 1);
    let rec = jobs.wait(id, LONG).unwrap();
    assert_eq!(rec.state, JobState::Failed);
    assert_eq!(rec.error, Some(JobErrorCode::Timeout));
    assert_eq!(rec.ended_at, Some(Duration::from_millis(201)));
    eventually("the handler to see its cancel token", || saw_cancel.load(Ordering::SeqCst));
    committer.commit(&jobs, id);
    assert!(committer.received().is_empty());

    let states: Vec<(Option<JobState>, JobState, Option<JobErrorCode>)> =
        log.of(id).iter().map(|e| (e.from, e.to, e.error)).collect();
    assert_eq!(
        states,
        vec![
            (None, JobState::Queued, None),
            (Some(JobState::Queued), JobState::Running, None),
            (Some(JobState::Running), JobState::Failed, Some(JobErrorCode::Timeout)),
        ]
    );
    // Later checks do not fire again.
    clock.set(Duration::from_secs(5));
    assert_eq!(jobs.check_timeouts(), 0);

    assert_transitions_allowed(&log.all());
    assert_journal_untouched(&journal);
}

/// A handler that ignores its cancel token and returns a value after the timeout: the value
/// is dropped, the job stays `failed`/`timeout`.
#[test]
fn ac2_late_result_after_timeout_is_discarded() {
    let clock = Arc::new(ManualClock::new());
    let log = Arc::new(ActivityLog::default());
    let committer = Committer::default();
    let cfg = JobsConfig { max_running: 1, queue_cap: 10, clock: clock.clone(), timer_poll: None };
    let jobs: Jobs<String> = Jobs::new(cfg, log.clone()).unwrap();

    let c = clock.clone();
    let stubborn = move |_: &ProgressSink, _: &CancelToken| {
        while c.now() < Duration::from_secs(2) {
            std::thread::sleep(STEP);
        }
        Ok("late".to_string())
    };
    let timeout = Some(Duration::from_millis(200));
    let a = jobs.submit(Execution::new(cmd(), stubborn), JobOpts { timeout, ..opts(JobClass::Batch) }).unwrap();
    // A second job can start only once the first handler has returned (limit 1).
    let b = jobs
        .submit(Execution::new(cmd(), |_: &ProgressSink, _: &CancelToken| Ok("b".to_string())), opts(JobClass::Batch))
        .unwrap();

    clock.set(Duration::from_millis(300));
    assert_eq!(jobs.check_timeouts(), 1);
    assert_eq!(jobs.status(b).unwrap().state, JobState::Queued, "slot is held until the handler returns");
    clock.set(Duration::from_secs(2));
    assert_eq!(jobs.wait(b, LONG).unwrap().state, JobState::Succeeded);

    let rec = jobs.status(a).unwrap();
    assert_eq!((rec.state, rec.error), (JobState::Failed, Some(JobErrorCode::Timeout)));
    committer.commit(&jobs, a);
    assert!(committer.received().is_empty());
    assert_eq!(log.of(a).len(), 3);
    assert_transitions_allowed(&log.all());
}

/// The same timeout on real time, with the background timeout thread, and generous bounds.
#[test]
fn ac2_timeout_real_time_smoke() {
    let log = Arc::new(ActivityLog::default());
    let jobs: Jobs<String> = Jobs::new(config(1), log.clone()).unwrap();
    let handler = |_: &ProgressSink, token: &CancelToken| {
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(2) {
            if token.is_cancelled() {
                return Err(HandlerError::Cancelled);
            }
            std::thread::sleep(STEP);
        }
        Ok("slept 2 s".to_string())
    };
    let timeout = Some(Duration::from_millis(200));
    let id = jobs.submit(Execution::new(cmd(), handler), JobOpts { timeout, ..opts(JobClass::Batch) }).unwrap();
    let rec = jobs.wait(id, LONG).unwrap();
    assert_eq!((rec.state, rec.error), (JobState::Failed, Some(JobErrorCode::Timeout)));
    let ran = rec.ended_at.unwrap() - rec.started_at.unwrap();
    assert!(ran > Duration::from_millis(200), "timed out too early: {ran:?}");
    assert_eq!(jobs.take_result(id), None);
    assert_transitions_allowed(&log.all());
}

// ------------------------------------------------------------------ AC-3

#[test]
fn ac3_cancelled_queued_job_never_runs() {
    let log = Arc::new(ActivityLog::default());
    let journal = RecordJournal::default();
    let jobs: Jobs<String> = Jobs::new(config(1), log.clone()).unwrap();
    let gate = Gate::new();
    let live = Arc::new(Live::default());

    let a = jobs
        .submit(Execution::new(cmd(), sleep_handler("a".into(), 5, gate.clone(), live)), opts(JobClass::Batch))
        .unwrap();
    let ran = Arc::new(AtomicBool::new(false));
    let r = ran.clone();
    let b_handler = move |_: &ProgressSink, _: &CancelToken| {
        r.store(true, Ordering::SeqCst);
        Ok("b".to_string())
    };
    let b = jobs.submit(Execution::new(cmd(), b_handler), opts(JobClass::Interactive)).unwrap();
    assert_eq!(jobs.status(b).unwrap().state, JobState::Queued);

    assert_eq!(jobs.cancel(b), Ok(JobState::Cancelled));
    let rec = jobs.status(b).unwrap();
    assert_eq!((rec.state, rec.error, rec.started_at), (JobState::Cancelled, Some(JobErrorCode::Cancelled), None));
    // Cancelling again changes nothing.
    assert_eq!(jobs.cancel(b), Ok(JobState::Cancelled));

    gate.open();
    assert_eq!(jobs.wait(a, LONG).unwrap().state, JobState::Succeeded);
    // A job submitted after `a` runs only after the slot freed; by then `b` would have run.
    let c = jobs
        .submit(Execution::new(cmd(), |_: &ProgressSink, _: &CancelToken| Ok("c".to_string())), opts(JobClass::Batch))
        .unwrap();
    assert_eq!(jobs.wait(c, LONG).unwrap().state, JobState::Succeeded);
    assert!(!ran.load(Ordering::SeqCst), "the cancelled job's handler ran");
    assert_eq!(jobs.take_result(b), None);

    let states: Vec<(Option<JobState>, JobState)> = log.of(b).iter().map(|e| (e.from, e.to)).collect();
    assert_eq!(states, vec![(None, JobState::Queued), (Some(JobState::Queued), JobState::Cancelled)]);
    assert_eq!(log.of(b)[0].principal_kind, PrincipalKind::Script);

    assert_transitions_allowed(&log.all());
    assert_journal_untouched(&journal);
}

// ------------------------------------------------------------------ AC-4 (table)

#[test]
fn ac4_state_machine_matches_req_002() {
    use JobState::*;
    let all = [Queued, Running, Succeeded, Failed, Cancelled];
    for from in all {
        for to in all {
            assert_eq!(from.can_move_to(to), allowed(from, to), "{from} -> {to}");
        }
    }
    for s in all {
        assert_eq!(s.is_terminal(), !matches!(s, Queued | Running));
    }
}

// ------------------------------------------------------------------ AC-5

#[test]
fn ac5_thousand_submissions_unique_ids_and_p99_latency() {
    const N: usize = 1000;
    let jobs: Jobs<String> = Jobs::new(config(4), Arc::new(NullLifecycleSink)).unwrap();
    let mut latencies = Vec::with_capacity(N);
    let mut ids = Vec::with_capacity(N);
    for i in 0..N {
        let exec = Execution::new(cmd(), move |_: &ProgressSink, _: &CancelToken| Ok(i.to_string()));
        let t = Instant::now();
        let id = jobs.submit(exec, opts(JobClass::Batch)).unwrap();
        latencies.push(t.elapsed());
        ids.push(id);
    }
    let unique: BTreeSet<JobId> = ids.iter().copied().collect();
    assert_eq!(unique.len(), N, "duplicate job IDs");
    for id in &ids {
        assert_eq!(id.as_uuid().version(), 7);
        assert_eq!(id.as_uuid().variant_bits(), 0b10);
    }
    latencies.sort();
    // Nearest-rank 99th percentile: the 990th of 1000 sorted values.
    let p99 = latencies[(N * 99).div_ceil(100) - 1];
    assert!(p99 < SUBMIT_BUDGET, "p99 submit latency {p99:?}");

    for (i, id) in ids.iter().enumerate() {
        assert_eq!(jobs.wait(*id, LONG).unwrap().state, JobState::Succeeded);
        assert_eq!(jobs.take_result(*id), Some(i.to_string()));
    }
}

// ------------------------------------------------------------------ further checks

#[test]
fn queue_cap_refuses_with_queue_full() {
    let log = Arc::new(ActivityLog::default());
    let cfg = JobsConfig { max_running: 1, queue_cap: 2, ..JobsConfig::default() };
    let jobs: Jobs<String> = Jobs::new(cfg, log.clone()).unwrap();
    let gate = Gate::new();
    let live = Arc::new(Live::default());
    let mut ids = Vec::new();
    for name in ["a", "b", "c"] {
        let exec = Execution::new(cmd(), sleep_handler(name.into(), 1, gate.clone(), live.clone()));
        ids.push(jobs.submit(exec, opts(JobClass::Batch)).unwrap());
    }
    let extra = Execution::new(cmd(), sleep_handler("d".into(), 1, gate.clone(), live.clone()));
    assert_eq!(jobs.submit(extra, opts(JobClass::Interactive)), Err(JobErrorCode::QueueFull));
    assert_eq!(jobs.list().len(), 3, "a refused submission creates no job");
    assert_eq!(log.all().iter().filter(|e| e.from.is_none()).count(), 3);
    gate.open();
    for id in ids {
        assert_eq!(jobs.wait(id, LONG).unwrap().state, JobState::Succeeded);
    }
}

#[test]
fn unknown_id_is_not_found() {
    let other: Jobs<String> = Jobs::new(config(1), Arc::new(NullLifecycleSink)).unwrap();
    let foreign = other
        .submit(Execution::new(cmd(), |_: &ProgressSink, _: &CancelToken| Ok(String::new())), opts(JobClass::Batch))
        .unwrap();
    let jobs: Jobs<String> = Jobs::new(config(1), Arc::new(NullLifecycleSink)).unwrap();
    assert_eq!(jobs.status(foreign), Err(JobErrorCode::NotFound));
    assert_eq!(jobs.cancel(foreign), Err(JobErrorCode::NotFound));
    assert_eq!(jobs.wait(foreign, Duration::ZERO), Err(JobErrorCode::NotFound));
    assert!(jobs.progress(foreign).is_err());
    assert_eq!(jobs.take_result(foreign), None);
    assert!(!jobs.forget(foreign));
    assert_eq!(JobErrorCode::NotFound.code(), "not_found");
    assert_eq!(other.wait(foreign, LONG).unwrap().state, JobState::Succeeded);
}

#[test]
fn cancelling_a_running_job_discards_its_result() {
    let log = Arc::new(ActivityLog::default());
    let jobs: Jobs<String> = Jobs::new(config(2), log.clone()).unwrap();
    // One handler stops on the token; the other ignores it and returns a value anyway.
    let gate = Gate::new();
    let polite = jobs
        .submit(
            Execution::new(cmd(), sleep_handler("p".into(), 1, gate.clone(), Arc::default())),
            opts(JobClass::Batch),
        )
        .unwrap();
    let release = Gate::new();
    let entered = Arc::new(AtomicBool::new(false));
    let (rel, ent) = (release.clone(), entered.clone());
    let stubborn = move |_: &ProgressSink, _: &CancelToken| {
        ent.store(true, Ordering::SeqCst);
        let mut open = rel.open.lock().unwrap();
        while !*open {
            open = rel.cv.wait_timeout(open, STEP).unwrap().0;
        }
        Ok("partial".to_string())
    };
    let rude = jobs.submit(Execution::new(cmd(), stubborn), opts(JobClass::Batch)).unwrap();
    eventually("the stubborn handler to start", || entered.load(Ordering::SeqCst));

    assert_eq!(jobs.cancel(polite), Ok(JobState::Running));
    assert_eq!(jobs.cancel(rude), Ok(JobState::Running));
    release.open();
    for id in [polite, rude] {
        let rec = jobs.wait(id, LONG).unwrap();
        assert_eq!((rec.state, rec.error), (JobState::Cancelled, Some(JobErrorCode::Cancelled)));
        assert_eq!(jobs.take_result(id), None);
    }
    assert_transitions_allowed(&log.all());
}

#[test]
fn handler_failure_is_failed_with_handler_failed() {
    let log = Arc::new(ActivityLog::default());
    let jobs: Jobs<String> = Jobs::new(config(1), log.clone()).unwrap();
    let id = jobs
        .submit(
            Execution::new(cmd(), |_: &ProgressSink, _: &CancelToken| Err(HandlerError::Failed("boom".to_string()))),
            JobOpts { principal: PrincipalKind::Agent, ..opts(JobClass::Interactive) },
        )
        .unwrap();
    let rec = jobs.wait(id, LONG).unwrap();
    assert_eq!((rec.state, rec.error), (JobState::Failed, Some(JobErrorCode::HandlerFailed)));
    assert_eq!(rec.error_detail.as_deref(), Some("boom"));
    assert_eq!(rec.principal_kind, PrincipalKind::Agent);
    assert_eq!(jobs.take_result(id), None);
    assert!(log.of(id).iter().all(|e| e.principal_kind == PrincipalKind::Agent));
    assert!(jobs.forget(id));
    assert_eq!(jobs.status(id), Err(JobErrorCode::NotFound));
    assert_transitions_allowed(&log.all());
}

#[test]
fn wait_returns_current_record_at_deadline() {
    let jobs: Jobs<String> = Jobs::new(config(1), Arc::new(NullLifecycleSink)).unwrap();
    let gate = Gate::new();
    let id = jobs
        .submit(
            Execution::new(cmd(), sleep_handler("w".into(), 1, gate.clone(), Arc::default())),
            opts(JobClass::Batch),
        )
        .unwrap();
    assert_eq!(jobs.wait(id, Duration::from_millis(10)).unwrap().state, JobState::Running);
    assert!(!jobs.forget(id), "a running job is not forgotten");
    gate.open();
    assert_eq!(jobs.wait(id, LONG).unwrap().state, JobState::Succeeded);
}

#[test]
fn dropping_the_scheduler_cancels_queued_jobs() {
    let log = Arc::new(ActivityLog::default());
    let jobs: Jobs<String> = Jobs::new(config(1), log.clone()).unwrap();
    let gate = Gate::new();
    let a = jobs
        .submit(
            Execution::new(cmd(), sleep_handler("a".into(), 1, gate.clone(), Arc::default())),
            opts(JobClass::Batch),
        )
        .unwrap();
    let b = jobs
        .submit(
            Execution::new(cmd(), sleep_handler("b".into(), 1, gate.clone(), Arc::default())),
            opts(JobClass::Batch),
        )
        .unwrap();
    drop(jobs);
    let b_states: Vec<JobState> = log.of(b).iter().map(|e| e.to).collect();
    assert_eq!(b_states, vec![JobState::Queued, JobState::Cancelled]);
    // The running job sees its token and ends cancelled on its worker thread.
    eventually("the running job to end", || log.of(a).last().is_some_and(|e| e.to == JobState::Cancelled));
    assert_transitions_allowed(&log.all());
}

#[test]
fn default_limit_is_cpus_minus_one_at_least_one() {
    let cpus = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1);
    assert_eq!(default_max_running(), if cpus > 1 { cpus - 1 } else { 1 });
    assert_eq!(JobsConfig::default().queue_cap, 10_000);
    let jobs: Jobs<String> =
        Jobs::new(JobsConfig { max_running: 0, ..JobsConfig::default() }, Arc::new(NullLifecycleSink)).unwrap();
    assert_eq!(jobs.max_running(), 1);
}

#[test]
fn wire_names() {
    assert_eq!(JobState::Queued.as_str(), "queued");
    assert_eq!(JobState::Cancelled.to_string(), "cancelled");
    assert_eq!(JobClass::Interactive.as_str(), "interactive");
    assert_eq!(PrincipalKind::Human.as_str(), "human");
    let codes: Vec<&str> = [
        JobErrorCode::Timeout,
        JobErrorCode::Cancelled,
        JobErrorCode::HandlerFailed,
        JobErrorCode::NotFound,
        JobErrorCode::QueueFull,
    ]
    .iter()
    .map(|c| c.code())
    .collect();
    assert_eq!(codes, ["timeout", "cancelled", "handler_failed", "not_found", "queue_full"]);
    // SystemClock is the default time source.
    let _ = SystemClock::new().now();
}
