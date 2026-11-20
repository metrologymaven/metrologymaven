//! Spec node CORE.JRN.STREAM.ALOG, AC-4: the Activity Log holds a job's lifecycle changes even
//! with recording off. The transition digests are checked against the independent Python
//! reference `fixtures/core/jrn/stream/alog/make_fixtures.py` (`lifecycle.json`).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::{parse, CValue, Digest};
use fid_jobs::activity::{job_change, ActivityLogSink};
use fid_jobs::{
    CancelToken, Execution, JobClass, JobOpts, JobState, Jobs, JobsConfig, LifecycleEvent, LifecycleSink,
    PrincipalKind, ProgressSink, WallClock,
};
use fid_journal::stream::{
    ActivityLog, ActivityLogConfig, JobChange, RecordingFlag, Streams, CURRENT_FILE, LIFECYCLE_COMMAND,
};
use fid_journal::{verify_header_value, Manifests};
use fid_types::CommandId;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;

const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/core/jrn/stream/alog");
const LONG: Duration = Duration::from_secs(60);

struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> TempDir {
        static N: AtomicU32 = AtomicU32::new(0);
        let name = format!("fid-jobs-alog-{tag}-{}-{}", std::process::id(), N.fetch_add(1, Ordering::SeqCst));
        let p = std::env::temp_dir().join(name);
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        TempDir(p)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A wall clock fixed at 2030-01-01T00:00:00Z.
struct FixedWall;

impl WallClock for FixedWall {
    fn unix_time(&self) -> Duration {
        Duration::from_secs(1_893_456_000)
    }
}

fn manifests() -> Manifests {
    Manifests { registry: Digest([1; 32]), kernel: Digest([2; 32]) }
}

type Obj = BTreeMap<String, CValue>;

fn obj(v: &CValue) -> &Obj {
    match v {
        CValue::Obj(o) => o,
        other => panic!("object expected, got {other:?}"),
    }
}

fn st<'a>(o: &'a Obj, k: &str) -> Option<&'a str> {
    match o.get(k) {
        Some(CValue::Str(s)) => Some(s),
        Some(CValue::Null) => None,
        other => panic!("field {k} is {other:?}"),
    }
}

fn lines(dir: &Path) -> Vec<CValue> {
    let text = std::fs::read_to_string(dir.join(CURRENT_FILE)).unwrap();
    text.lines().map(|l| parse(l.as_bytes()).unwrap()).collect()
}

fn exec() -> Execution<String> {
    Execution::new(
        CommandId::parse("test.sleep@1").unwrap(),
        |_: &ProgressSink, _: &CancelToken| Ok("done".to_string()),
    )
}

/// The lifecycle digest is the Python reference's SHA-256 over RFC 8785.
#[test]
fn lifecycle_digests_match_the_reference() {
    let doc = parse(&std::fs::read(Path::new(FIXTURES).join("lifecycle.json")).unwrap()).unwrap();
    let CValue::Arr(cases) = &obj(&doc)["cases"] else { panic!("cases") };
    assert!(!cases.is_empty());
    for case in cases {
        let c = obj(&obj(case)["change"]);
        let change = JobChange {
            job: st(c, "job").unwrap(),
            command: st(c, "command").unwrap(),
            principal_kind: st(c, "principal_kind").unwrap(),
            from: st(c, "from"),
            to: st(c, "to").unwrap(),
            error: st(c, "error"),
            at_ns: 0,
            wall: None,
        };
        assert_eq!(change.digest().unwrap().to_string(), st(obj(case), "digest").unwrap());
    }
}

/// AC-4: a test job that runs and succeeds leaves queued, running and succeeded in the
/// Activity Log, with recording off. `time.utc` is the scheduler's wall-clock time of each
/// change.
#[test]
fn ac4_lifecycle_is_logged_with_recording_off() {
    let t = TempDir::new("ac4");
    let log = Arc::new(ActivityLog::new(t.0.join("alog"), ActivityLogConfig::default(), manifests()));
    let flag = Arc::new(RecordingFlag::new());
    let streams = Streams::with_flag(log.clone(), flag.clone());
    assert!(!streams.recording().is_recording());

    let sink = Arc::new(ActivityLogSink::spawn(log.clone()).unwrap());
    let config = JobsConfig { wall_clock: Arc::new(FixedWall), ..JobsConfig::default() };
    let jobs: Jobs<String> = Jobs::new(config, sink.clone()).unwrap();
    let id = jobs
        .submit(exec(), JobOpts { class: JobClass::Batch, timeout: None, principal: PrincipalKind::Human })
        .unwrap();
    assert_eq!(jobs.wait(id, LONG).unwrap().state, JobState::Succeeded);
    sink.flush();
    log.close().unwrap();
    assert_eq!(log.warning_count(), 0);

    let job = id.to_string();
    let recorded = lines(&t.0.join("alog"));
    assert_eq!(recorded.len(), 3, "queued, running, succeeded");
    let expected = [(None, "queued"), (Some("queued"), "running"), (Some("running"), "succeeded")];
    for (i, (line, (from, to))) in recorded.iter().zip(expected).enumerate() {
        verify_header_value(line).unwrap();
        let h = obj(line);
        assert_eq!(st(h, "stream"), Some("activity"));
        assert_eq!(st(h, "seq").unwrap(), (i + 1).to_string());
        assert_eq!(st(h, "prev"), Some(format!("sha256:{}", "0".repeat(64)).as_str()));
        assert_eq!(st(obj(&h["time"]), "utc"), Some("2030-01-01T00:00:00.000000000Z"));
        assert_eq!(st(obj(&h["principal"]), "kind"), Some("system"));
        let c = obj(&h["command"]);
        assert_eq!(st(c, "id"), Some(LIFECYCLE_COMMAND));
        assert_eq!(st(c, "class"), Some("R0"));
        let change = JobChange {
            job: &job,
            command: "test.sleep@1",
            principal_kind: "human",
            from,
            to,
            error: None,
            at_ns: 0,
            wall: None,
        };
        let CValue::Arr(outputs) = &c["outputs"] else { panic!("outputs") };
        assert_eq!(outputs.len(), 1);
        assert_eq!(st(obj(&outputs[0]), "entity"), Some(job.as_str()));
        assert_eq!(st(obj(&outputs[0]), "digest").unwrap(), change.digest().unwrap().to_string());
    }
}

/// REQ-008 for the sink: an unwritable log never disturbs the scheduler; the failure is counted.
#[test]
fn an_unwritable_log_does_not_fail_the_job() {
    let t = TempDir::new("blocked");
    let blocker = t.0.join("not-a-dir");
    std::fs::write(&blocker, b"x").unwrap();
    let log = Arc::new(ActivityLog::new(blocker.join("alog"), ActivityLogConfig::default(), manifests()));
    let sink = Arc::new(ActivityLogSink::spawn(log.clone()).unwrap());
    let jobs: Jobs<String> = Jobs::new(JobsConfig::default(), sink.clone()).unwrap();
    let id = jobs
        .submit(exec(), JobOpts { class: JobClass::Interactive, timeout: None, principal: PrincipalKind::Agent })
        .unwrap();
    assert_eq!(jobs.wait(id, LONG).unwrap().state, JobState::Succeeded);
    sink.flush();
    assert_eq!(log.warning_count(), 3);
    assert_eq!(log.take_last_warning().unwrap().code, "activity_log_unavailable");
}

/// Dropping the sink writes what is still queued, in order.
#[test]
fn dropping_the_sink_drains_the_queue() {
    let t = TempDir::new("drain");
    let log = Arc::new(ActivityLog::new(t.0.join("alog"), ActivityLogConfig::default(), manifests()));
    let jobs: Jobs<String> = Jobs::new(JobsConfig::default(), Arc::new(fid_jobs::NullLifecycleSink)).unwrap();
    let id = jobs
        .submit(exec(), JobOpts { class: JobClass::Batch, timeout: None, principal: PrincipalKind::Script })
        .unwrap();
    let sink = ActivityLogSink::spawn(log.clone()).unwrap();
    for i in 0..50u64 {
        sink.emit(&LifecycleEvent {
            job: id,
            command: CommandId::parse("test.sleep@1").unwrap(),
            principal_kind: PrincipalKind::Script,
            from: None,
            to: JobState::Queued,
            error: None,
            at: Duration::from_nanos(i),
            wall: Duration::from_secs(1_893_456_000),
        });
    }
    drop(sink);
    log.close().unwrap();
    let recorded = lines(&t.0.join("alog"));
    assert_eq!(recorded.len(), 50);
    for (i, line) in recorded.iter().enumerate() {
        assert_eq!(st(obj(&obj(line)["time"]), "mono_ns").unwrap(), i.to_string());
    }
}

/// The event-to-change mapping keeps every field.
#[test]
fn job_change_maps_every_field() {
    let t = TempDir::new("map");
    let log = Arc::new(ActivityLog::new(t.0.join("alog"), ActivityLogConfig::default(), manifests()));
    let jobs: Jobs<String> = Jobs::new(JobsConfig::default(), Arc::new(fid_jobs::NullLifecycleSink)).unwrap();
    let id = jobs
        .submit(exec(), JobOpts { class: JobClass::Batch, timeout: None, principal: PrincipalKind::Script })
        .unwrap();
    jobs.wait(id, LONG).unwrap();
    let event = LifecycleEvent {
        job: id,
        command: CommandId::parse("test.sleep@1").unwrap(),
        principal_kind: PrincipalKind::Script,
        from: Some(JobState::Running),
        to: JobState::Failed,
        error: Some(fid_jobs::JobErrorCode::Timeout),
        at: Duration::from_nanos(42),
        wall: Duration::from_secs(951_827_696),
    };
    let job = id.to_string();
    let c = job_change(&event, &job);
    assert_eq!(
        c,
        JobChange {
            job: &job,
            command: "test.sleep@1",
            principal_kind: "script",
            from: Some("running"),
            to: "failed",
            error: Some("timeout"),
            at_ns: 42,
            wall: Some(Duration::from_secs(951_827_696)),
        }
    );
    let sink = ActivityLogSink::spawn(log.clone()).unwrap();
    sink.emit(&event);
    drop(sink);
    log.close().unwrap();
    let recorded = lines(&t.0.join("alog"));
    assert_eq!(recorded.len(), 1);
    let time = obj(&obj(&recorded[0])["time"]);
    assert_eq!(st(time, "mono_ns"), Some("42"));
    assert_eq!(st(time, "utc"), Some("2000-02-29T12:34:56.000000000Z"));
}
