//! Acceptance criteria of spec node CORE.JOB.CMD (AC-1 .. AC-6), plus checks of REQ-001 ..
//! REQ-007 against the hand-labelled fixtures in `fixtures/core/job/cmd/`.
//!
//! Surfaces are simulated: a test "bus" runs `JobService::invoke_json` (what the generated
//! CLI `--json` and MCP tools call) and appends every reported commit to a Record Journal
//! stub. Principals are built directly, as the test channel harness would supply them.
//! Held handlers wait at a gate, so states never depend on thread speed; only AC-4 uses
//! real time, with generous bounds.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_bus::BusError;
use fid_canon::{parse, to_canonical_bytes, CValue};
use fid_jobs::{
    command_defs, context_types, job_status_schema, Caller, CancelToken, Clock, Execution, HandlerError, JobClass,
    JobErrorCode, JobId, JobOpts, JobParams, JobRecord, JobService, JobState, JobStatus, Jobs, JobsConfig,
    JournalEffect, ManualClock, NullLifecycleSink, PrincipalId, PrincipalKind, ProgressSink, ProgressSnapshot,
    WaitParams, WaitResult, CANCEL_JOB, CANCEL_PRECONDITION, DEFAULT_WAIT_TIMEOUT, LIST_JOBS, MAX_PARAMS_BYTES,
    SHOW_STATUS, WAIT_JOB,
};
use fid_registry::compile_command;
use fid_types::CommandId;
use fid_uuid::Uuid;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

const LONG: Duration = Duration::from_secs(60);
const STEP: Duration = Duration::from_millis(1);
/// A well-formed UUIDv7 that no scheduler in these tests issues.
const UNKNOWN: &str = "01890a5d-ac96-774b-bcce-b302099a8057";

// ------------------------------------------------------------------ wiring

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn fixture_bytes(name: &str) -> Vec<u8> {
    std::fs::read(root().join("fixtures/core/job/cmd").join(name)).unwrap()
}

fn fixture(name: &str) -> CValue {
    parse(&fixture_bytes(name)).unwrap()
}

fn get<'a>(v: &'a CValue, k: &str) -> &'a CValue {
    match v {
        CValue::Obj(o) => o.get(k).unwrap_or_else(|| panic!("missing {k}")),
        _ => panic!("object expected for {k}"),
    }
}

fn s(v: &CValue) -> &str {
    match v {
        CValue::Str(s) => s,
        other => panic!("string expected, got {other:?}"),
    }
}

fn arr(v: &CValue) -> &[CValue] {
    match v {
        CValue::Arr(a) => a,
        other => panic!("array expected, got {other:?}"),
    }
}

fn kind(name: &str) -> PrincipalKind {
    match name {
        "human" => PrincipalKind::Human,
        "agent" => PrincipalKind::Agent,
        "script" => PrincipalKind::Script,
        other => panic!("unknown principal kind {other}"),
    }
}

fn caller(k: PrincipalKind, id: &str) -> Caller {
    Caller::new(k, PrincipalId::parse(id).unwrap())
}

fn caller_of(v: &CValue) -> Caller {
    caller(kind(s(get(v, "kind"))), s(get(v, "id")))
}

fn cmd() -> CommandId {
    CommandId::parse("test.sleep@1").unwrap()
}

fn opts() -> JobOpts {
    JobOpts { class: JobClass::Batch, timeout: None, principal: PrincipalKind::Script }
}

fn service(limit: usize, clock: Option<Arc<ManualClock>>) -> JobService<String> {
    let mut config = JobsConfig { max_running: limit, timer_poll: None, ..JobsConfig::default() };
    if let Some(c) = clock {
        config.clock = c as Arc<dyn Clock>;
    }
    JobService::new(Jobs::new(config, Arc::new(NullLifecycleSink)).unwrap()).unwrap()
}

/// Holds handlers until opened (or until their job is cancelled).
#[derive(Default)]
struct Gate {
    open: Mutex<bool>,
    cv: Condvar,
}

impl Gate {
    fn open(&self) {
        *self.open.lock().unwrap() = true;
        self.cv.notify_all();
    }

    fn pass(&self, token: &CancelToken) -> bool {
        let mut open = self.open.lock().unwrap();
        loop {
            if token.is_cancelled() {
                return false;
            }
            if *open {
                return true;
            }
            open = self.cv.wait_timeout(open, STEP).unwrap().0;
        }
    }
}

/// Test handler: reports progress, then waits at the gate.
fn held(gate: &Arc<Gate>) -> Execution<String> {
    let gate = Arc::clone(gate);
    Execution::new(cmd(), move |sink: &ProgressSink, token: &CancelToken| {
        sink.report(0.5, "held");
        if gate.pass(token) {
            Ok("done".to_string())
        } else {
            Err(HandlerError::Cancelled)
        }
    })
}

/// Test sleep handler: runs for `ms` in 1 ms steps, stopping on cancellation.
fn sleeper(ms: u64) -> Execution<String> {
    Execution::new(cmd(), move |sink: &ProgressSink, token: &CancelToken| {
        let start = Instant::now();
        while start.elapsed() < Duration::from_millis(ms) {
            if token.is_cancelled() {
                return Err(HandlerError::Cancelled);
            }
            std::thread::sleep(STEP);
        }
        sink.report(1.0, "done");
        Ok("slept".to_string())
    })
}

fn eventually(what: &str, mut cond: impl FnMut() -> bool) {
    let start = Instant::now();
    while !cond() {
        assert!(start.elapsed() < LONG, "timed out waiting for {what}");
        std::thread::sleep(STEP);
    }
}

fn state_of(svc: &JobService<String>, id: JobId) -> JobState {
    svc.jobs().status(id).unwrap().state
}

/// Record Journal stub, written only by the test bus from reported commits.
#[derive(Default)]
struct RecordJournal {
    entries: Mutex<Vec<JournalEffect>>,
}

impl RecordJournal {
    fn entries(&self) -> Vec<JournalEffect> {
        self.entries.lock().unwrap().clone()
    }
}

/// Test bus: runs one command from JSON and commits what it reports.
fn run(
    svc: &JobService<String>,
    journal: &RecordJournal,
    who: &Caller,
    command: &str,
    params: &str,
) -> Result<CValue, BusError> {
    let inv = svc.invoke_json(who, command, params.as_bytes())?;
    if let Some(c) = inv.commit {
        journal.entries.lock().unwrap().push(c);
    }
    Ok(inv.result)
}

fn job_param(id: JobId) -> String {
    format!("{{\"job\":\"{id}\"}}")
}

fn assert_err(r: Result<CValue, BusError>, code: &str, reason: &str) {
    match r {
        Err(e) => assert_eq!((e.code().as_str(), e.reason()), (code, reason), "error {e}"),
        Ok(v) => panic!("expected {code}/{reason}, got {v:?}"),
    }
}

// ------------------------------------------------------------------ schema checker

/// A lowercase hyphenated UUID, checked character by character.
fn is_lower_uuid(t: &str) -> bool {
    t.len() == 36
        && t.bytes().enumerate().all(|(i, c)| {
            if matches!(i, 8 | 13 | 18 | 23) {
                c == b'-'
            } else {
                c.is_ascii_digit() || (b'a'..=b'f').contains(&c)
            }
        })
}

fn is_type(v: &CValue, t: &str) -> bool {
    match t {
        "object" => matches!(v, CValue::Obj(_)),
        "array" => matches!(v, CValue::Arr(_)),
        "string" => matches!(v, CValue::Str(_)),
        "boolean" => matches!(v, CValue::Bool(_)),
        "null" => matches!(v, CValue::Null),
        "number" => matches!(v, CValue::Num(_) | CValue::I64(_) | CValue::U64(_)),
        other => panic!("schema type {other} not supported by the test checker"),
    }
}

fn number(v: &CValue) -> Option<f64> {
    match v {
        CValue::Num(n) => Some(*n),
        _ => None,
    }
}

/// Validates `v` against the subset of JSON Schema 2020-12 that job-status.schema.json
/// uses. Any other keyword fails, so the checker cannot silently ignore a rule.
fn validate(schema: &CValue, v: &CValue, at: &str) -> Result<(), String> {
    let CValue::Obj(sch) = schema else { return Err(format!("{at}: schema is not an object")) };
    for (k, rule) in sch {
        match k.as_str() {
            "$schema" | "$id" | "title" | "x-quantity" | "x-unit" => {}
            "type" => {
                let ok = match rule {
                    CValue::Str(t) => is_type(v, t),
                    CValue::Arr(ts) => ts.iter().any(|t| is_type(v, s(t))),
                    _ => return Err(format!("{at}: bad type rule")),
                };
                if !ok {
                    return Err(format!("{at}: wrong type {v:?}"));
                }
            }
            "enum" => {
                if !arr(rule).contains(v) {
                    return Err(format!("{at}: {v:?} not in enum"));
                }
            }
            "pattern" => {
                let CValue::Str(t) = v else { continue };
                let ok = match s(rule) {
                    p if p == fid_jobs::JOB_ID_PATTERN => is_lower_uuid(t),
                    p if p == CommandId::PATTERN => CommandId::is_valid(t),
                    p => return Err(format!("{at}: pattern {p} not supported by the test checker")),
                };
                if !ok {
                    return Err(format!("{at}: {t:?} does not match the pattern"));
                }
            }
            "minimum" | "maximum" => {
                let Some(n) = number(v) else { continue };
                let bound = number(rule).unwrap();
                if (k == "minimum" && n < bound) || (k == "maximum" && n > bound) {
                    return Err(format!("{at}: {n} violates {k} {bound}"));
                }
            }
            "maxLength" => {
                let CValue::Str(t) = v else { continue };
                if t.chars().count() as f64 > number(rule).unwrap() {
                    return Err(format!("{at}: too long"));
                }
            }
            "required" => {
                let CValue::Obj(o) = v else { continue };
                for r in arr(rule) {
                    if !o.contains_key(s(r)) {
                        return Err(format!("{at}: missing {}", s(r)));
                    }
                }
            }
            "additionalProperties" => {
                let (CValue::Obj(o), Some(CValue::Obj(props))) = (v, sch.get("properties")) else { continue };
                if *rule != CValue::Bool(false) {
                    return Err(format!("{at}: additionalProperties must be false"));
                }
                if let Some(extra) = o.keys().find(|k| !props.contains_key(*k)) {
                    return Err(format!("{at}: extra property {extra}"));
                }
            }
            "properties" => {
                let (CValue::Obj(o), CValue::Obj(props)) = (v, rule) else { continue };
                for (name, sub) in props {
                    if let Some(x) = o.get(name) {
                        validate(sub, x, &format!("{at}/{name}"))?;
                    }
                }
            }
            "items" => {
                let CValue::Arr(items) = v else { continue };
                for (i, x) in items.iter().enumerate() {
                    validate(rule, x, &format!("{at}/{i}"))?;
                }
            }
            other => return Err(format!("{at}: keyword {other} not supported by the test checker")),
        }
    }
    Ok(())
}

fn published_schema() -> CValue {
    parse(&std::fs::read(root().join("schema/job-status.schema.json")).unwrap()).unwrap()
}

fn assert_valid_status(v: &CValue) {
    validate(&published_schema(), v, "").unwrap_or_else(|e| panic!("job status does not validate: {e}\n{v:?}"));
}

// ------------------------------------------------------------------ the contract

/// The checked-in schema (written by `make_job_status_schema.py`) is what the code publishes.
#[test]
fn schema_file_matches() {
    let mut want = to_canonical_bytes(&job_status_schema()).unwrap();
    want.push(b'\n');
    let have = std::fs::read(root().join("schema/job-status.schema.json")).unwrap();
    assert!(
        have == want,
        "schema/job-status.schema.json differs from job_status_schema():\n{}",
        String::from_utf8_lossy(&want)
    );
}

fn record(state: JobState) -> JobRecord {
    JobRecord {
        id: JobId::from_uuid(Uuid::parse(UNKNOWN).unwrap()),
        command: cmd(),
        principal_kind: PrincipalKind::Script,
        class: JobClass::Batch,
        state,
        progress: ProgressSnapshot { fraction: 0.0, stage: String::new(), report_seq: 0 },
        submitted_at: Duration::ZERO,
        started_at: None,
        ended_at: None,
        error: None,
        error_detail: None,
    }
}

/// REQ-004: the status JSON matches hand-written canonical documents byte for byte.
#[test]
fn status_json_matches_fixtures() {
    let failed = JobRecord {
        progress: ProgressSnapshot { fraction: 0.25, stage: "probing".to_string(), report_seq: 3 },
        submitted_at: Duration::from_millis(1500),
        started_at: Some(Duration::from_secs(2)),
        ended_at: Some(Duration::from_millis(2250)),
        error: Some(JobErrorCode::Timeout),
        error_detail: Some("handler text is not part of the contract".to_string()),
        ..record(JobState::Failed)
    };
    let st = JobStatus::from_record(&failed);
    assert_eq!(st.to_json().unwrap(), fixture_bytes("status_failed.json"));
    assert_valid_status(&st.to_value());

    let queued = JobStatus::from_record(&record(JobState::Queued));
    assert_eq!(queued.to_json().unwrap(), fixture_bytes("status_queued.json"));
    assert_valid_status(&queued.to_value());

    let running = JobRecord {
        command: CommandId::parse("probe.run_scan@2").unwrap(),
        progress: ProgressSnapshot { fraction: 0.5, stage: "scan".to_string(), report_seq: 1 },
        submitted_at: Duration::from_millis(1),
        started_at: Some(Duration::from_millis(125)),
        ..record(JobState::Running)
    };
    let waited = WaitResult { status: JobStatus::from_record(&running), timed_out: true };
    assert_eq!(waited.to_json().unwrap(), fixture_bytes("wait_timed_out.json"));
    // The wait result is a job status plus `timed_out`.
    let CValue::Obj(mut o) = waited.to_value() else { panic!() };
    assert_eq!(o.remove("timed_out"), Some(CValue::Bool(true)));
    assert_valid_status(&CValue::Obj(o));
}

/// REQ-001, REQ-002: classes, idempotency, undo, effects and the precondition, against
/// `defs.expected.json`.
#[test]
fn definitions_match_fixture() {
    let want = fixture("defs.expected.json");
    let CValue::Obj(want) = get(&want, "commands").clone() else { panic!() };
    let defs = command_defs().unwrap();
    let names: Vec<String> = defs.iter().map(|d| format!("{}@{}", d.id, d.major)).collect();
    assert_eq!(names, [LIST_JOBS, SHOW_STATUS, WAIT_JOB, CANCEL_JOB]);
    assert_eq!(want.len(), defs.len());
    for d in &defs {
        let name = format!("{}@{}", d.id, d.major);
        let w = want.get(&name).unwrap_or_else(|| panic!("no fixture row for {name}"));
        let strs = |k: &str| arr(get(w, k)).iter().map(|x| s(x).to_string()).collect::<Vec<_>>();
        assert_eq!(d.side_effect.as_str(), s(get(w, "side_effect")), "{name}");
        assert_eq!(d.determinism.as_str(), s(get(w, "determinism")), "{name}");
        assert_eq!(d.idempotency.as_str(), s(get(w, "idempotency")), "{name}");
        assert_eq!(d.undo.as_str(), s(get(w, "undo")), "{name}");
        assert_eq!(d.mcp_projection.as_str(), s(get(w, "mcp_projection")), "{name}");
        assert_eq!(d.fs_effect.as_str(), s(get(w, "fs_effect")), "{name}");
        assert_eq!(d.network.as_str(), s(get(w, "network")), "{name}");
        assert_eq!(CValue::Bool(d.human_only), *get(w, "human_only"), "{name}");
        assert_eq!(d.exposure.iter().map(|x| x.as_str().to_string()).collect::<Vec<_>>(), strs("exposure"), "{name}");
        assert_eq!(d.effects, strs("effects"), "{name}");
        let keys: Vec<String> = d.preconditions.iter().map(|p| p.message_key.clone()).collect();
        assert_eq!(keys, strs("preconditions"), "{name}");
        let CValue::Obj(params) = &d.params_schema else { panic!() };
        let required = params.get("required").map(strs_of).unwrap_or_default();
        assert_eq!(required, strs("params_required"), "{name}");
        let Some(CValue::Obj(props)) = params.get("properties") else { panic!() };
        assert_eq!(props.keys().cloned().collect::<Vec<_>>(), strs("params_properties"), "{name}");
        // Every definition survives the registry's own round trip.
        assert_eq!(fid_registry::CommandDef::from_value(d.to_value()).unwrap(), *d);
    }
    // The cancel precondition is FPL that the registry build accepts.
    let cancel = defs.iter().find(|d| d.id == "job.cancel_job").unwrap();
    assert_eq!(cancel.preconditions[0].expr, CANCEL_PRECONDITION);
    assert_eq!(compile_command(cancel, &context_types()).unwrap().len(), 1);
}

fn strs_of(v: &CValue) -> Vec<String> {
    match v {
        CValue::Arr(a) => a.iter().map(|x| s(x).to_string()).collect(),
        _ => Vec::new(),
    }
}

#[test]
fn list_jobs_has_no_required_params() {
    let defs = command_defs().unwrap();
    let CValue::Obj(o) = &defs[0].params_schema else { panic!() };
    assert!(!o.contains_key("required"));
}

// ------------------------------------------------------------------ acceptance criteria

/// AC-1: a running job of script principal S; S's `show-status --json` validates against
/// job-status.schema.json and says `running`.
#[test]
fn ac1_show_status_running() {
    let svc = service(2, None);
    let journal = RecordJournal::default();
    let gate = Arc::new(Gate::default());
    let s_ = caller(PrincipalKind::Script, "S");
    let id = svc.submit_as(&s_, held(&gate), opts()).unwrap();
    eventually("job running", || state_of(&svc, id) == JobState::Running);
    let out = run(&svc, &journal, &s_, SHOW_STATUS, &job_param(id)).unwrap();
    assert_valid_status(&out);
    assert_eq!(s(get(&out, "state")), "running");
    assert_eq!(s(get(&out, "id")), id.to_string());
    assert_eq!(s(get(&out, "command")), "test.sleep@1");
    assert!(journal.entries().is_empty(), "R0 commands commit nothing");
    gate.open();
}

/// AC-2: S cancels its own running job; it ends `cancelled` and the journal holds exactly
/// one `job.cancel_job@1` entry, also after a repeated cancel (idempotency natural).
#[test]
fn ac2_cancel_own_job() {
    let svc = service(2, None);
    let journal = RecordJournal::default();
    let gate = Arc::new(Gate::default());
    let s_ = caller(PrincipalKind::Script, "S");
    let id = svc.submit_as(&s_, held(&gate), opts()).unwrap();
    eventually("job running", || state_of(&svc, id) == JobState::Running);

    let out = run(&svc, &journal, &s_, CANCEL_JOB, &job_param(id)).unwrap();
    assert_valid_status(&out);
    assert!(matches!(s(get(&out, "state")), "running" | "cancelled"));
    let waited = run(&svc, &journal, &s_, WAIT_JOB, &format!("{{\"job\":\"{id}\",\"timeout\":60}}")).unwrap();
    assert_eq!(s(get(&waited, "state")), "cancelled");
    assert_eq!(*get(&waited, "timed_out"), CValue::Bool(false));
    assert_eq!(s(get(&waited, "error")), "cancelled");

    let again = run(&svc, &journal, &s_, CANCEL_JOB, &job_param(id)).unwrap();
    assert_eq!(s(get(&again, "state")), "cancelled");
    assert_eq!(journal.entries(), vec![JournalEffect { event_type: "job.cancel_job@1", job: id }]);
}

/// A repeated cancel while the handler has not yet returned commits nothing more.
#[test]
fn cancel_twice_while_running_commits_once() {
    let svc = service(2, None);
    // This handler ignores the token until the gate opens, so the job stays `running`.
    let gate = Arc::new(Gate::default());
    let g = Arc::clone(&gate);
    let deaf = Execution::new(cmd(), move |_: &ProgressSink, _: &CancelToken| {
        let mut open = g.open.lock().unwrap();
        while !*open {
            open = g.cv.wait_timeout(open, STEP).unwrap().0;
        }
        Ok("late".to_string())
    });
    let s_ = caller(PrincipalKind::Script, "S");
    let id = svc.submit_as(&s_, deaf, opts()).unwrap();
    eventually("job running", || state_of(&svc, id) == JobState::Running);
    let first = svc.cancel_job(&s_, &JobParams { job: id }).unwrap();
    let second = svc.cancel_job(&s_, &JobParams { job: id }).unwrap();
    assert_eq!(first.commit, Some(JournalEffect { event_type: CANCEL_JOB, job: id }));
    assert_eq!(second.commit, None);
    assert_eq!(second.status.state, JobState::Running);
    gate.open();
    let end = svc.wait_job(&s_, &WaitParams { job: id, timeout: LONG }).unwrap();
    assert_eq!(end.status.state, JobState::Cancelled, "cancellation wins over a late result");
}

/// Cancelling a queued job ends it at once and commits one entry.
#[test]
fn cancel_queued_job() {
    let svc = service(1, None);
    let journal = RecordJournal::default();
    let gate = Arc::new(Gate::default());
    let s_ = caller(PrincipalKind::Script, "S");
    let _first = svc.submit_as(&s_, held(&gate), opts()).unwrap();
    let queued = svc.submit_as(&s_, held(&gate), opts()).unwrap();
    assert_eq!(state_of(&svc, queued), JobState::Queued);
    let out = run(&svc, &journal, &s_, CANCEL_JOB, &job_param(queued)).unwrap();
    assert_eq!(s(get(&out, "state")), "cancelled");
    assert_eq!(journal.entries().len(), 1);
    gate.open();
}

/// AC-3: agent A cannot cancel human H's job over MCP: `policy`, and the job keeps running.
#[test]
fn ac3_agent_cannot_cancel_human_job() {
    let svc = service(2, None);
    let journal = RecordJournal::default();
    let gate = Arc::new(Gate::default());
    let h = caller(PrincipalKind::Human, "H");
    let a = caller(PrincipalKind::Agent, "A");
    let id = svc.submit_as(&h, held(&gate), opts()).unwrap();
    eventually("job running", || state_of(&svc, id) == JobState::Running);
    assert_err(run(&svc, &journal, &a, CANCEL_JOB, &job_param(id)), "policy", "not_job_owner");
    assert_eq!(state_of(&svc, id), JobState::Running);
    assert!(!svc.jobs().progress(id).unwrap().is_cancel_requested(), "no cancellation was requested");
    assert!(journal.entries().is_empty());
    // The agent cannot read it either (fail closed), but its owner can.
    assert_err(run(&svc, &journal, &a, SHOW_STATUS, &job_param(id)), "policy", "not_job_owner");
    assert_err(run(&svc, &journal, &a, WAIT_JOB, &job_param(id)), "policy", "not_job_owner");
    assert!(run(&svc, &journal, &h, SHOW_STATUS, &job_param(id)).is_ok());
    gate.open();
}

/// REQ-002: the owner-or-human rule over the hand-labelled matrix, for cancel and status.
#[test]
fn cancel_rule_matrix() {
    let cases = fixture("cancel_rule.json");
    let svc = service(64, None);
    let gate = Arc::new(Gate::default());
    for (i, case) in arr(get(&cases, "cases")).iter().enumerate() {
        let id = match get(case, "owner") {
            CValue::Null => svc.jobs().submit(held(&gate), opts()).unwrap(),
            owner => svc.submit_as(&caller_of(owner), held(&gate), opts()).unwrap(),
        };
        eventually("job running", || state_of(&svc, id) == JobState::Running);
        let who = caller_of(get(case, "caller"));
        let allowed = *get(case, "allowed") == CValue::Bool(true);
        let shown = svc.show_status(&who, &JobParams { job: id });
        let cancelled = svc.cancel_job(&who, &JobParams { job: id });
        if allowed {
            assert!(shown.is_ok(), "case {i}: status refused");
            let out = cancelled.unwrap_or_else(|e| panic!("case {i}: cancel refused: {e}"));
            assert_eq!(out.commit, Some(JournalEffect { event_type: CANCEL_JOB, job: id }), "case {i}");
        } else {
            for e in [shown.map(|_| ()).unwrap_err(), cancelled.map(|_| ()).unwrap_err()] {
                assert_eq!((e.code().as_str(), e.reason()), ("policy", "not_job_owner"), "case {i}");
            }
            assert!(!svc.jobs().progress(id).unwrap().is_cancel_requested(), "case {i}: job was touched");
        }
    }
    gate.open();
}

/// AC-4: on a 2 s job, `--timeout 0.2` returns after about 0.2 s with `timed_out: true`;
/// `--timeout 5` returns the terminal status.
#[test]
fn ac4_wait_timeout_and_terminal() {
    let svc = service(2, None);
    let journal = RecordJournal::default();
    let s_ = caller(PrincipalKind::Script, "S");
    let id = svc.submit_as(&s_, sleeper(2000), opts()).unwrap();

    let start = Instant::now();
    let out = run(&svc, &journal, &s_, WAIT_JOB, &format!("{{\"job\":\"{id}\",\"timeout\":0.2}}")).unwrap();
    let took = start.elapsed();
    assert_eq!(*get(&out, "timed_out"), CValue::Bool(true));
    assert_eq!(s(get(&out, "state")), "running");
    assert!(took >= Duration::from_millis(200), "returned early after {took:?}");
    assert!(took < Duration::from_millis(1900), "returned late after {took:?}");

    let out = run(&svc, &journal, &s_, WAIT_JOB, &format!("{{\"job\":\"{id}\",\"timeout\":5}}")).unwrap();
    assert_eq!(*get(&out, "timed_out"), CValue::Bool(false));
    assert_eq!(s(get(&out, "state")), "succeeded");
    assert!(journal.entries().is_empty());

    // A terminal job returns at once; timeout 0 never blocks.
    let start = Instant::now();
    let out = run(&svc, &journal, &s_, WAIT_JOB, &format!("{{\"job\":\"{id}\",\"timeout\":0}}")).unwrap();
    assert_eq!(*get(&out, "timed_out"), CValue::Bool(false));
    assert!(start.elapsed() < Duration::from_secs(1));
}

/// AC-5 and REQ-003: visibility, state filter and submission order, against
/// `visibility.json`.
#[test]
fn ac5_list_jobs_visibility_and_order() {
    let fx = fixture("visibility.json");
    let clock = Arc::new(ManualClock::new());
    let limit = number(get(&fx, "limit")).unwrap() as usize;
    let svc = service(limit, Some(Arc::clone(&clock)));
    let journal = RecordJournal::default();
    let gate = Arc::new(Gate::default());
    let mut ids = Vec::new();
    for j in arr(get(&fx, "jobs")) {
        clock.advance(Duration::from_millis(1));
        ids.push(svc.submit_as(&caller_of(j), held(&gate), opts()).unwrap());
    }
    for (i, case) in arr(get(&fx, "cases")).iter().enumerate() {
        let who = caller_of(get(case, "caller"));
        let params = match get(case, "state") {
            CValue::Null => "{}".to_string(),
            st => format!("{{\"state\":\"{}\"}}", s(st)),
        };
        let out = run(&svc, &journal, &who, LIST_JOBS, &params).unwrap();
        let got: Vec<String> = arr(get(&out, "jobs")).iter().map(|j| s(get(j, "id")).to_string()).collect();
        let want: Vec<String> =
            arr(get(case, "expect")).iter().map(|n| ids[number(n).unwrap() as usize].to_string()).collect();
        assert_eq!(got, want, "case {i}");
        for j in arr(get(&out, "jobs")) {
            assert_valid_status(j);
        }
    }
    assert!(journal.entries().is_empty());
    gate.open();
}

/// REQ-003: equal submission times are ordered by job ID.
#[test]
fn list_ties_ordered_by_id() {
    let clock = Arc::new(ManualClock::new());
    let svc = service(1, Some(clock));
    let gate = Arc::new(Gate::default());
    let h = caller(PrincipalKind::Human, "H");
    let mut ids: Vec<JobId> = (0..20).map(|_| svc.submit_as(&h, held(&gate), opts()).unwrap()).collect();
    ids.sort();
    let got: Vec<JobId> = svc.list_jobs(&h, &Default::default()).unwrap().jobs.iter().map(|j| j.id).collect();
    assert_eq!(got, ids);
    gate.open();
}

/// AC-6: an unknown job ID fails each command with `not_found` and journals nothing.
#[test]
fn ac6_unknown_job_not_found() {
    let svc = service(2, None);
    let journal = RecordJournal::default();
    for who in [caller(PrincipalKind::Script, "S"), caller(PrincipalKind::Human, "H")] {
        for command in [SHOW_STATUS, WAIT_JOB, CANCEL_JOB] {
            assert_err(
                run(&svc, &journal, &who, command, &format!("{{\"job\":\"{UNKNOWN}\"}}")),
                "not_found",
                "unknown_job",
            );
        }
    }
    // list_jobs takes no job ID; an empty scheduler lists nothing.
    let out = run(&svc, &journal, &caller(PrincipalKind::Human, "H"), LIST_JOBS, "{}").unwrap();
    assert!(arr(get(&out, "jobs")).is_empty());
    assert!(journal.entries().is_empty());
}

/// REQ-007: cancelling a job already terminal returns its status unchanged and commits
/// nothing.
#[test]
fn cancel_terminal_job_commits_nothing() {
    let svc = service(2, None);
    let journal = RecordJournal::default();
    let s_ = caller(PrincipalKind::Script, "S");
    let id = svc.submit_as(&s_, sleeper(0), opts()).unwrap();
    let end = svc.wait_job(&s_, &WaitParams { job: id, timeout: LONG }).unwrap();
    assert_eq!(end.status.state, JobState::Succeeded);
    let before = run(&svc, &journal, &s_, SHOW_STATUS, &job_param(id)).unwrap();
    let out = run(&svc, &journal, &s_, CANCEL_JOB, &job_param(id)).unwrap();
    assert_eq!(out, before);
    assert!(journal.entries().is_empty());
    assert_eq!(svc.jobs().take_result(id), Some("slept".to_string()), "the result is untouched");
}

// ------------------------------------------------------------------ fail closed

#[test]
fn invalid_params_are_refused() {
    let svc = service(2, None);
    let journal = RecordJournal::default();
    let h = caller(PrincipalKind::Human, "H");
    let cases = fixture("params_invalid.json");
    for (i, case) in arr(get(&cases, "cases")).iter().enumerate() {
        let r = run(&svc, &journal, &h, s(get(case, "command")), s(get(case, "params")));
        match r {
            Err(e) => assert_eq!(
                (e.code().as_str(), e.reason()),
                (s(get(case, "code")), s(get(case, "reason"))),
                "case {i}: {e}"
            ),
            Ok(v) => panic!("case {i} accepted: {v:?}"),
        }
    }
    let big = format!("{{\"state\":\"queued\",\"pad\":\"{}\"}}", "x".repeat(MAX_PARAMS_BYTES));
    assert_err(run(&svc, &journal, &h, LIST_JOBS, &big), "validation", "input_limit");
    assert_err(run(&svc, &journal, &h, "job.delete_job@1", "{}"), "not_found", "unknown_command");
    assert!(journal.entries().is_empty());
}

#[test]
fn wait_timeout_bounds_and_default() {
    let job = format!("\"job\":\"{UNKNOWN}\"");
    let p = |extra: &str| WaitParams::from_value(&parse(format!("{{{job}{extra}}}").as_bytes()).unwrap());
    assert_eq!(p("").unwrap().timeout, DEFAULT_WAIT_TIMEOUT);
    assert_eq!(DEFAULT_WAIT_TIMEOUT, Duration::from_secs(30));
    assert_eq!(p(",\"timeout\":0").unwrap().timeout, Duration::ZERO);
    assert_eq!(p(",\"timeout\":3600").unwrap().timeout, Duration::from_secs(3600));
    assert_eq!(p(",\"timeout\":0.2").unwrap().timeout, Duration::from_millis(200));
    // Values a JSON parser cannot produce are refused too.
    for bad in [f64::NAN, f64::INFINITY, -0.5] {
        let mut o = BTreeMap::new();
        o.insert("job".to_string(), CValue::Str(UNKNOWN.to_string()));
        o.insert("timeout".to_string(), CValue::Num(bad));
        let e = WaitParams::from_value(&CValue::Obj(o)).unwrap_err();
        assert_eq!(e.reason(), "literal_out_of_range");
    }
}

#[test]
fn principal_ids_are_bounded_and_case_insensitive() {
    let long = "x".repeat(257);
    for bad in ["", "a b", "caf\u{e9}", "tab\there", long.as_str()] {
        let e = PrincipalId::parse(bad).unwrap_err();
        assert_eq!((e.code().as_str(), e.reason()), ("validation", "name"), "{bad:?}");
    }
    assert!(PrincipalId::parse(&"x".repeat(256)).is_ok());
    assert_eq!(PrincipalId::parse("Jo@Example.ORG").unwrap(), PrincipalId::parse("jo@example.org").unwrap());
}

/// The job's principal kind is the submitting caller's.
#[test]
fn submit_as_records_owner_and_kind() {
    let svc = service(1, None);
    let gate = Arc::new(Gate::default());
    let a = caller(PrincipalKind::Agent, "A");
    let id = svc.submit_as(&a, held(&gate), JobOpts { principal: PrincipalKind::Human, ..opts() }).unwrap();
    assert_eq!(svc.jobs().status(id).unwrap().principal_kind, PrincipalKind::Agent);
    assert_eq!(svc.owner(id), Some(a));
    gate.open();
}

/// Status and list over 1,000 jobs (the budget itself is unverified; this checks the result).
#[test]
fn list_thousand_jobs() {
    let clock = Arc::new(ManualClock::new());
    let svc = service(1, Some(Arc::clone(&clock)));
    let gate = Arc::new(Gate::default());
    let s_ = caller(PrincipalKind::Script, "S");
    let t = caller(PrincipalKind::Script, "T");
    let mut mine = Vec::new();
    for i in 0..1000 {
        clock.advance(Duration::from_millis(1));
        let who = if i % 2 == 0 { &s_ } else { &t };
        let id = svc.submit_as(who, held(&gate), opts()).unwrap();
        if i % 2 == 0 {
            mine.push(id);
        }
    }
    let start = Instant::now();
    let listed = svc.list_jobs(&s_, &Default::default()).unwrap();
    println!("list of 1000 jobs took {:?}", start.elapsed());
    assert_eq!(listed.jobs.iter().map(|j| j.id).collect::<Vec<_>>(), mine);
    let all = svc.list_jobs(&caller(PrincipalKind::Human, "H"), &Default::default()).unwrap();
    assert_eq!(all.jobs.len(), 1000);
    gate.open();
}
