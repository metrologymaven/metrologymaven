//! Acceptance criteria of spec node CORE.JRN.STREAM.ALOG (AC-1, AC-2, AC-3, AC-5, AC-6) and
//! the pinned rules of module `stream`. AC-4 (job lifecycle) is in `fid-jobs/tests/activity.rs`,
//! next to the `LifecycleSink` implementation.
//!
//! The routing table of AC-1 comes from the independent Python reference
//! `fixtures/core/jrn/stream/alog/make_fixtures.py`, written from CONSTITUTION §3 (D-031).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::{parse, to_canonical_bytes, CValue, Digest};
use fid_journal::stream::{
    activity_settings, rotated_file_name, route, ActivityLog, ActivityLogConfig, CallError, Locality, ProgramStepRef,
    RecordingFlag, RouteContext, StreamId, Streams, CURRENT_FILE, DEFAULT_KEEP, DEFAULT_MAX_BYTES, KEEP_KEY,
    MAX_BYTES_KEY, UNAVAILABLE,
};
use fid_journal::{
    session_start, validate_header_value, verify_header_value, AiAssistance, CommandDraft, DraftBody, EntryDraft,
    EntryTime, Journal, Manifests, PrincipalDraft, PrincipalKind, SessionInfo, TaggedDigest,
};
use fid_store::settings::{resolve, FileStore, Scope, SettingsRegistry, Sources};
use fid_store::ProjectLocation;
use fid_types::{Determinism, SideEffect};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/core/jrn/stream/alog");

// ------------------------------------------------------------------ scaffolding

struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> TempDir {
        static N: AtomicU32 = AtomicU32::new(0);
        let name = format!("fid-alog-{tag}-{}-{}", std::process::id(), N.fetch_add(1, Ordering::SeqCst));
        let p = std::path::absolute(std::env::temp_dir().join(name)).unwrap();
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        TempDir(p)
    }

    fn p(&self, rel: &str) -> PathBuf {
        self.0.join(rel)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn manifests() -> Manifests {
    Manifests { registry: Digest([1; 32]), kernel: Digest([2; 32]) }
}

/// A completed call of `class` by a local human; `n` varies the arguments.
fn call(class: SideEffect, n: u64) -> EntryDraft {
    EntryDraft {
        stream: "whatever".to_string(),
        branch: "main".to_string(),
        logical_parent: None,
        time: EntryTime { utc: "2026-11-19T08:00:00Z".to_string(), mono_ns: n, sync: "ntp".to_string() },
        principal: PrincipalDraft {
            kind: PrincipalKind::Human,
            identity: CValue::Str("user-1".to_string()),
            on_behalf_of: None,
        },
        ai_assistance: AiAssistance::None,
        manifests: manifests(),
        reason: None,
        human_assertion: None,
        body: DraftBody::Command(CommandDraft {
            id: "test.read@1".to_string(),
            major: 1,
            class,
            determinism: Determinism::D0,
            seed: 0,
            args: CValue::Obj(BTreeMap::from([("n".to_string(), CValue::U64(n))])),
            events: Vec::new(),
            inputs: vec![TaggedDigest { tag: "param".to_string(), digest: Digest([3; 32]) }],
            consumed_r1: Vec::new(),
            outputs: Vec::new(),
            algorithms: Vec::new(),
        }),
    }
}

fn read_lines(path: &Path) -> Vec<String> {
    match std::fs::read(path) {
        Ok(bytes) => {
            assert!(bytes.is_empty() || bytes.ends_with(b"\n"), "{} ends mid-line", path.display());
            String::from_utf8(bytes).unwrap().lines().map(str::to_string).collect()
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(e) => panic!("{}: {e}", path.display()),
    }
}

fn header(line: &str) -> BTreeMap<String, CValue> {
    match parse(line.as_bytes()).unwrap() {
        CValue::Obj(o) => o,
        other => panic!("object expected, got {other:?}"),
    }
}

fn s<'a>(o: &'a BTreeMap<String, CValue>, k: &str) -> &'a str {
    match o.get(k) {
        Some(CValue::Str(v)) => v,
        other => panic!("field {k} is {other:?}"),
    }
}

/// Every file name in `dir`, sorted.
fn names(dir: &Path) -> Vec<String> {
    let mut v: Vec<String> =
        std::fs::read_dir(dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    v.sort();
    v
}

const ZERO: &str = "sha256:0000000000000000000000000000000000000000000000000000000000000000";

/// Checks one Activity Log line against the entry schema and the leaf's line rules.
fn check_line(line: &str, seq: u64) {
    let v = parse(line.as_bytes()).unwrap();
    validate_header_value(&v).unwrap();
    verify_header_value(&v).unwrap();
    // One canonical JSON line.
    assert_eq!(to_canonical_bytes(&v).unwrap(), line.as_bytes());
    let h = header(line);
    assert_eq!(s(&h, "stream"), "activity");
    assert_eq!(s(&h, "seq"), seq.to_string());
    assert_eq!(s(&h, "prev"), ZERO, "no previous-hash links");
}

// ------------------------------------------------------------------ AC-1

fn parse_class(s: &str) -> SideEffect {
    SideEffect::parse(s).unwrap()
}

fn parse_bool(s: &str) -> bool {
    match s {
        "true" => true,
        "false" => false,
        other => panic!("bool expected, got {other}"),
    }
}

/// AC-1: every class x locality x recording flag x program step (x Access Ledger present)
/// routes to the stream the reference table names.
#[test]
fn ac1_routing_table() {
    let text = std::fs::read_to_string(Path::new(FIXTURES).join("routing.csv")).unwrap();
    let mut rows = text.lines();
    assert_eq!(rows.next(), Some("class,principal,recording,program_step,access_ledger,expected"));
    let mut n = 0usize;
    let mut classes = std::collections::BTreeSet::new();
    for row in rows {
        let f: Vec<&str> = row.split(',').collect();
        assert_eq!(f.len(), 6, "{row}");
        let class = parse_class(f[0]);
        classes.insert(class);
        let principal = match f[1] {
            "local" => Locality::Local,
            "external" => Locality::External,
            other => panic!("locality {other}"),
        };
        let ctx = RouteContext {
            recording: parse_bool(f[2]),
            program_step: parse_bool(f[3])
                .then(|| ProgramStepRef { program: "prog-1".to_string(), step: "step-1".to_string() }),
            access_ledger: parse_bool(f[4]),
        };
        let want = match f[5] {
            "none" => None,
            "record" => Some(StreamId::Record),
            "activity" => Some(StreamId::Activity),
            "access_ledger" => Some(StreamId::AccessLedger),
            other => panic!("stream {other}"),
        };
        assert_eq!(route(class, principal, &ctx), want, "{row}");
        // D0: the same answer every time.
        assert_eq!(route(class, principal, &ctx), want, "{row}");
        n += 1;
    }
    assert_eq!(n, SideEffect::ALL.len() * 16);
    assert_eq!(classes.len(), SideEffect::ALL.len(), "every class is in the table");
}

/// REQ-002: a read never goes to the Record Journal, whatever the context.
#[test]
fn reads_never_reach_the_record_journal() {
    for class in [SideEffect::R0, SideEffect::R1] {
        for principal in [Locality::Local, Locality::External] {
            for bits in 0..8u8 {
                let ctx = RouteContext {
                    recording: bits & 1 != 0,
                    program_step: (bits & 2 != 0)
                        .then(|| ProgramStepRef { program: "p".to_string(), step: "s".to_string() }),
                    access_ledger: bits & 4 != 0,
                };
                assert_ne!(route(class, principal, &ctx), Some(StreamId::Record));
            }
        }
    }
}

// ------------------------------------------------------------------ AC-2

/// AC-2: without a recording session, 100 R0 calls change neither the Activity Log nor the
/// Record Journal; after `set_recording(true)` the next R0 call adds one line that validates
/// against `entry.schema.json` with stream `activity`.
#[test]
fn ac2_recording_gates_r0_calls() {
    let t = TempDir::new("ac2");
    let (loc, lock) = ProjectLocation::create(&t.p("project")).unwrap();
    let journal = Journal::open(loc, lock, "record").unwrap();
    let alog_dir = t.p("alog");
    let streams = Streams::new(Arc::new(ActivityLog::new(&alog_dir, ActivityLogConfig::default(), manifests())));

    for i in 0..100 {
        let out = streams.complete_call(Locality::Local, None, &call(SideEffect::R0, i)).unwrap();
        assert_eq!(out.stream, None);
        assert_eq!(out.written, None);
        assert_eq!(out.warning, None);
    }
    assert!(!alog_dir.exists(), "the Activity Log was touched without recording");
    assert_eq!(journal.tail().unwrap(), None, "the Record Journal changed");

    streams.set_recording(true, Some("rec-1"));
    assert_eq!(streams.recording().recording_id().as_deref(), Some("rec-1"));
    let out = streams.complete_call(Locality::Local, None, &call(SideEffect::R0, 100)).unwrap();
    assert_eq!(out.stream, Some(StreamId::Activity));
    assert_eq!(out.warning, None);
    let lines = read_lines(&alog_dir.join(CURRENT_FILE));
    assert_eq!(lines.len(), 1);
    check_line(&lines[0], 1);
    let h = header(&lines[0]);
    assert_eq!(s(&h, "hash"), out.written.unwrap().to_string());
    match &h["command"] {
        CValue::Obj(c) => {
            assert_eq!(s(c, "class"), "R0");
            assert_eq!(s(c, "id"), "test.read@1");
        }
        other => panic!("command {other:?}"),
    }
    assert_eq!(journal.tail().unwrap(), None, "the Record Journal changed");

    // Turning recording off stops the writes again and clears the ID.
    streams.set_recording(false, Some("ignored"));
    assert_eq!(streams.recording().recording_id(), None);
    let out = streams.complete_call(Locality::Local, None, &call(SideEffect::R0, 101)).unwrap();
    assert_eq!(out.stream, None);
    assert_eq!(read_lines(&alog_dir.join(CURRENT_FILE)).len(), 1);
}

/// Committed (non-read) calls are returned for the Record Journal and never written to the
/// Activity Log; external reads go to the Access Ledger only when one exists.
#[test]
fn writes_and_external_reads_are_not_written_here() {
    let t = TempDir::new("route");
    let alog_dir = t.p("alog");
    let mut streams = Streams::new(Arc::new(ActivityLog::new(&alog_dir, ActivityLogConfig::default(), manifests())));
    streams.set_recording(true, None);
    let out = streams.complete_call(Locality::Local, None, &call(SideEffect::W2, 1)).unwrap();
    assert_eq!((out.stream, out.written), (Some(StreamId::Record), None));
    let out = streams.complete_call(Locality::External, None, &call(SideEffect::R0, 2)).unwrap();
    assert_eq!((out.stream, out.written), (None, None));
    streams.set_access_ledger(true);
    let out = streams.complete_call(Locality::External, None, &call(SideEffect::R1, 3)).unwrap();
    assert_eq!((out.stream, out.written), (Some(StreamId::AccessLedger), None));
    assert!(!alog_dir.exists());

    // A draft that is not a command is not a call.
    let session = session_start(
        "activity",
        "main",
        EntryTime { utc: "2026-11-19T08:00:00Z".to_string(), mono_ns: 0, sync: "ntp".to_string() },
        PrincipalDraft { kind: PrincipalKind::Human, identity: CValue::Str("u".to_string()), on_behalf_of: None },
        AiAssistance::None,
        manifests(),
        SessionInfo { session_id: "s".to_string(), app_version: "0.0.1".to_string() },
    );
    assert_eq!(streams.complete_call(Locality::Local, None, &session).unwrap_err(), CallError::NotACommand);
}

// ------------------------------------------------------------------ AC-3

/// AC-3: an R1 call that carries a Command Program step reference is written without recording.
#[test]
fn ac3_program_step_is_logged_without_recording() {
    let t = TempDir::new("ac3");
    let alog_dir = t.p("alog");
    let streams = Streams::new(Arc::new(ActivityLog::new(&alog_dir, ActivityLogConfig::default(), manifests())));
    assert!(!streams.recording().is_recording());
    let step = ProgramStepRef { program: "prog-7".to_string(), step: "step-3".to_string() };
    let out = streams.complete_call(Locality::Local, Some(&step), &call(SideEffect::R1, 1)).unwrap();
    assert_eq!(out.stream, Some(StreamId::Activity));
    assert!(out.written.is_some() && out.warning.is_none());
    let lines = read_lines(&alog_dir.join(CURRENT_FILE));
    assert_eq!(lines.len(), 1);
    check_line(&lines[0], 1);
}

// ------------------------------------------------------------------ AC-5

/// The Activity Log settings: declared valid, defaults 64 MiB and 8 (REQ-006).
#[test]
fn settings_are_declared_with_the_spec_defaults() {
    let registry = SettingsRegistry::build(&activity_settings()).unwrap();
    assert_eq!(registry.len(), 2);
    let t = TempDir::new("defaults");
    let user = FileStore::new(Scope::User, t.p("user/settings.json")).unwrap();
    let machine = FileStore::new(Scope::Machine, t.p("machine/settings.json")).unwrap();
    let (values, warnings) = resolve(&registry, &Sources { project: None, user: &user, machine: &machine }, |_| true);
    assert!(warnings.is_empty(), "{warnings:?}");
    let config = ActivityLogConfig::from_resolved(&values);
    assert_eq!(config, ActivityLogConfig::default());
    assert_eq!((config.max_bytes, config.keep), (64 * 1024 * 1024, 8));
    assert_eq!((DEFAULT_MAX_BYTES, DEFAULT_KEEP), (64 * 1024 * 1024, 8));
}

/// AC-5: with a 1 MiB rotation size set in settings and 5,000 records, the log rotates, at
/// most 8 rotated files remain, and no line is split across files.
#[test]
fn ac5_rotation_from_settings() {
    let t = TempDir::new("ac5");
    let registry = SettingsRegistry::build(&activity_settings()).unwrap();
    let user_path = t.p("cfg/user/settings.json");
    let doc = CValue::Obj(BTreeMap::from([
        ("format".to_string(), CValue::Str("fid-settings/1".to_string())),
        ("values".to_string(), CValue::Obj(BTreeMap::from([(MAX_BYTES_KEY.to_string(), CValue::Num(1_048_576.0))]))),
    ]));
    std::fs::create_dir_all(user_path.parent().unwrap()).unwrap();
    std::fs::write(&user_path, to_canonical_bytes(&doc).unwrap()).unwrap();
    let user = FileStore::new(Scope::User, user_path).unwrap();
    let machine = FileStore::new(Scope::Machine, t.p("cfg/machine/settings.json")).unwrap();
    let (values, warnings) = resolve(&registry, &Sources { project: None, user: &user, machine: &machine }, |_| true);
    assert!(warnings.is_empty(), "{warnings:?}");
    let config = ActivityLogConfig::from_resolved(&values);
    assert_eq!(config, ActivityLogConfig { max_bytes: 1_048_576, keep: 8 });
    assert!(values.iter().any(|r| r.key == KEEP_KEY));

    let dir = t.p("alog");
    // A file of another stream in the same directory is never touched (REQ-007).
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("record.db"), b"keep me").unwrap();
    let log = ActivityLog::new(&dir, config, manifests());
    // Records carry a 2 KiB argument so 5,000 of them span well over 9 files.
    let pad = "x".repeat(2048);
    let mut hashes = Vec::new();
    for i in 0..5000u64 {
        let mut d = call(SideEffect::R0, i);
        if let DraftBody::Command(c) = &mut d.body {
            c.outputs.push(fid_journal::OutputDigest { entity: Some(format!("{pad}-{i}")), digest: Digest([4; 32]) });
        }
        hashes.push(log.append(&d).unwrap());
    }
    log.close().unwrap();
    assert_eq!(log.warning_count(), 0);

    let files = names(&dir);
    assert!(files.contains(&"record.db".to_string()));
    assert_eq!(std::fs::read(dir.join("record.db")).unwrap(), b"keep me");
    let mut rotated: Vec<u64> =
        files.iter().filter_map(|f| f.strip_prefix("activity.")?.strip_suffix(".log")?.parse().ok()).collect();
    rotated.sort_unstable();
    assert!(!rotated.is_empty(), "the log rotated");
    assert!(rotated.len() <= 8, "{} rotated files remain", rotated.len());
    // The oldest were deleted, so the numbers are the newest, contiguous.
    let newest = *rotated.last().unwrap();
    assert!(newest > 8, "more than 8 rotations happened, so deletion was exercised");
    assert_eq!(rotated, ((newest - 7)..=newest).collect::<Vec<_>>());

    // Every remaining file holds whole lines below the limit, and sequence numbers run on
    // across files without a gap, ending at 5000.
    let mut seqs = Vec::new();
    for n in &rotated {
        let path = dir.join(rotated_file_name(*n));
        assert!(std::fs::metadata(&path).unwrap().len() <= 1_048_576);
        for line in read_lines(&path) {
            let h = header(&line);
            seqs.push(s(&h, "seq").parse::<u64>().unwrap());
        }
    }
    let current = read_lines(&dir.join(CURRENT_FILE));
    assert!(std::fs::metadata(dir.join(CURRENT_FILE)).unwrap().len() <= 1_048_576);
    for line in &current {
        seqs.push(s(&header(line), "seq").parse::<u64>().unwrap());
    }
    let first = seqs[0];
    assert_eq!(seqs, (first..=5000).collect::<Vec<_>>());
    // The last line is the last record, intact.
    let last = current.last().unwrap();
    check_line(last, 5000);
    assert_eq!(s(&header(last), "hash"), hashes[4999].to_string());
}

/// Reopening continues the sequence after the last line and repairs a torn last line.
#[test]
fn reopen_resumes_the_sequence_and_repairs_a_torn_line() {
    let t = TempDir::new("reopen");
    let dir = t.p("alog");
    {
        let log = ActivityLog::new(&dir, ActivityLogConfig::default(), manifests());
        for i in 0..3 {
            log.append(&call(SideEffect::R0, i)).unwrap();
        }
    }
    // A torn write: half a line without its newline.
    let mut bytes = std::fs::read(dir.join(CURRENT_FILE)).unwrap();
    bytes.extend_from_slice(b"{\"schema_version\":1,\"str");
    std::fs::write(dir.join(CURRENT_FILE), &bytes).unwrap();

    let log = ActivityLog::new(&dir, ActivityLogConfig::default(), manifests());
    log.append(&call(SideEffect::R0, 3)).unwrap();
    log.close().unwrap();
    let lines = read_lines(&dir.join(CURRENT_FILE));
    assert_eq!(lines.len(), 5);
    assert!(parse(lines[3].as_bytes()).is_err(), "the torn line stays on its own line");
    check_line(&lines[4], 4);
}

// ------------------------------------------------------------------ AC-6

/// AC-6: an Activity Log directory that cannot be written makes the R0 call succeed with a
/// warning. Portable form: the log directory's parent is a regular file. Unix also runs the
/// read-only directory form (skipped when the process can write anyway, e.g. as root).
#[test]
fn ac6_unwritable_directory_is_a_warning() {
    let t = TempDir::new("ac6");
    let blocker = t.p("file");
    std::fs::write(&blocker, b"x").unwrap();
    let streams =
        Streams::new(Arc::new(ActivityLog::new(blocker.join("alog"), ActivityLogConfig::default(), manifests())));
    streams.set_recording(true, Some("rec"));
    let out = streams.complete_call(Locality::Local, None, &call(SideEffect::R0, 1)).unwrap();
    assert_eq!(out.stream, Some(StreamId::Activity));
    assert_eq!(out.written, None);
    assert_eq!(out.warning.as_ref().unwrap().code, UNAVAILABLE);
    assert_eq!(streams.activity().warning_count(), 1);
    assert_eq!(streams.activity().take_last_warning(), out.warning);

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let ro = t.p("ro");
        std::fs::create_dir_all(&ro).unwrap();
        std::fs::set_permissions(&ro, std::fs::Permissions::from_mode(0o555)).unwrap();
        let can_write = std::fs::write(ro.join("probe"), b"").is_ok();
        if !can_write {
            let streams = Streams::new(Arc::new(ActivityLog::new(&ro, ActivityLogConfig::default(), manifests())));
            streams.set_recording(true, None);
            let out = streams.complete_call(Locality::Local, None, &call(SideEffect::R0, 2)).unwrap();
            assert_eq!(out.warning.unwrap().code, UNAVAILABLE);
            assert_eq!(out.written, None);
        }
        std::fs::set_permissions(&ro, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
}

/// The recording flag on its own: off by default, ID only while active.
#[test]
fn recording_flag() {
    let f = RecordingFlag::new();
    assert!(!f.is_recording() && f.recording_id().is_none());
    f.set_recording(true, None);
    assert!(f.is_recording() && f.recording_id().is_none());
    f.set_recording(true, Some("r2"));
    assert_eq!(f.recording_id().as_deref(), Some("r2"));
    f.set_recording(false, None);
    assert!(!f.is_recording() && f.recording_id().is_none());
}
