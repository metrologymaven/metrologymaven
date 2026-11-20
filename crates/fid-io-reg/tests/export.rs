//! Acceptance criteria of spec node IO.REG.ENV.EXPORT (AC-1 .. AC-6) and the rules behind
//! them: root confinement, no clobbering, cancel, the bus error mapping against catalogue v3,
//! the command definitions with real params and results against their schemas, and the
//! shared conformance kit. Expected bytes, digests, reports and the journal record come from
//! the independent stdlib-Python reference `fixtures/io/reg/env/export/make_fixtures.py`;
//! other expected outcomes are hand-labelled from the spec's rules. Every root is a fresh
//! temporary folder.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::CValue;
use fid_io_reg::env::export::command::{
    export_file_def, format_def, params_schema, result_schema, CODE_PATTERN, DIGEST_PATTERN, FORMAT_ID_PATTERN,
    TYPE_NAME_PATTERN, U64_PATTERN,
};
use fid_io_reg::env::export::conformance::{self, FailAfter, Fixture};
use fid_io_reg::env::export::{
    format_command_id, ByteSink, CountingSink, ExportCommand, ExportEnv, ExportError, ExportEvent, ExportFile,
    ExportParams, ExportRun, Fault, SourceRef, Writer, ENTITY_ID_PATTERN, EXPORT_FILE,
};
use fid_io_reg::{
    Direction, FormatDescriptor, FormatId, IoError, LimitKind, Origin, Phase, Position, Principal, Registry,
    ReportBuilder, RootAccess, RootId, RootSet, TypeName, VersionRange, VersionToken,
};
use fid_replay::ReplayOutcome;
use fid_types::Digest;
use sha2::{Digest as _, Sha256};
use std::cell::Cell;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

const POINTS: &[u8] = include_bytes!("../../../fixtures/io/reg/env/export/points.json");
const EXPECTED: &[u8] = include_bytes!("../../../fixtures/io/reg/env/export/expected.json");
const BUS_ERRORS_V3: &[u8] = include_bytes!("../../../fixtures/core/bus/pipe/err/bus-errors.v3.json");

// ---------------------------------------------------------------------------------------
// The test format `fid-test-points`

/// A point set: integer nanometre coordinates; some points carry a colour.
struct Points {
    pts: Vec<[i64; 3]>,
    colored: BTreeSet<usize>,
}

fn num_i64(v: &CValue) -> i64 {
    match v {
        CValue::Num(n) if n.fract() == 0.0 => *n as i64,
        other => panic!("not an integer: {other:?}"),
    }
}

fn points() -> Points {
    let v = fid_canon::parse(POINTS).unwrap();
    let CValue::Obj(o) = v else { panic!() };
    let CValue::Arr(pts) = &o["points"] else { panic!() };
    let CValue::Arr(colored) = &o["colored"] else { panic!() };
    Points {
        pts: pts
            .iter()
            .map(|p| {
                let CValue::Arr(c) = p else { panic!() };
                [num_i64(&c[0]), num_i64(&c[1]), num_i64(&c[2])]
            })
            .collect(),
        colored: colored.iter().map(|i| num_i64(i) as usize).collect(),
    }
}

/// The writer of the test format (rule written out in the fixture script).
#[derive(Debug)]
struct TestPoints {
    format: FormatId,
    version: VersionToken,
}

fn test_points() -> TestPoints {
    TestPoints { format: FormatId::new("fid-test-points").unwrap(), version: VersionToken::new("1.0.0").unwrap() }
}

impl Writer for TestPoints {
    type Source = Points;

    fn format(&self) -> &FormatId {
        &self.format
    }

    fn version(&self) -> &VersionToken {
        &self.version
    }

    fn options(&self, given: &CValue) -> Result<CValue, &'static str> {
        let CValue::Obj(o) = given else { return Err("options must be an object") };
        if o.keys().any(|k| k != "stride") {
            return Err("unknown option");
        }
        let stride = match o.get("stride") {
            None => 1.0,
            Some(CValue::Num(n)) if n.fract() == 0.0 && (1.0..=1000.0).contains(n) => *n,
            Some(_) => return Err("stride must be an integer 1..=1000"),
        };
        Ok(CValue::Obj(BTreeMap::from([("stride".to_string(), CValue::Num(stride))])))
    }

    fn write(
        &self,
        src: &Points,
        opts: &CValue,
        sink: &mut dyn ByteSink,
        report: &mut ReportBuilder,
    ) -> Result<(), IoError> {
        let CValue::Obj(o) = opts else { return Err(IoError::Malformed { at: Position::default(), reason: "opts" }) };
        let stride = match o.get("stride") {
            Some(CValue::Num(n)) => *n as usize,
            _ => 1,
        };
        let chosen: Vec<usize> = (0..src.pts.len()).step_by(stride).collect();
        sink.write_bytes(format!("FTP 1\n{}\n", chosen.len()).as_bytes())?;
        for &i in &chosen {
            let [x, y, z] = src.pts[i];
            sink.write_bytes(format!("{x} {y} {z}\n").as_bytes())?;
            if src.colored.contains(&i) {
                report.lossy(&TypeName::new("point_set.color").unwrap());
            }
        }
        Ok(())
    }
}

fn descriptor(id: &str, ext: &str, direction: Direction) -> FormatDescriptor {
    // The test format declares its one option, `stride`.
    let options = if id == "fid-test-points" {
        fid_canon::parse(
            b"{\"properties\":{\"stride\":{\"maximum\":1000,\"minimum\":1,\"type\":\"integer\"}},\"type\":\"object\"}",
        )
        .unwrap()
    } else {
        CValue::Obj(BTreeMap::new())
    };
    FormatDescriptor::new(
        FormatId::new(id).unwrap(),
        VersionRange::new("1", "1").unwrap(),
        Vec::new(),
        vec![ext.to_string()],
        options,
        Phase::new(0).unwrap(),
        Origin::FirstParty,
        direction,
    )
    .unwrap()
}

/// Export descriptors for `fid-test-points` (`.ftp`) and `fid-test-stl` (`.stl`), and an
/// import descriptor that must not count for the export extension check.
fn registry() -> Registry {
    let mut r = Registry::new();
    r.register(descriptor("fid-test-points", "ftp", Direction::Export)).unwrap();
    r.register(descriptor("fid-test-stl", "stl", Direction::Export)).unwrap();
    r.register(descriptor("fid-test-import", "txt", Direction::Import)).unwrap();
    r
}

// ---------------------------------------------------------------------------------------
// Fixtures and helpers

struct Expected(BTreeMap<String, CValue>);

impl Expected {
    fn load() -> Expected {
        let CValue::Obj(o) = fid_canon::parse(EXPECTED).unwrap() else { panic!() };
        Expected(o)
    }

    fn get(&self, case: &str, key: &str) -> String {
        let CValue::Obj(c) = &self.0[case] else { panic!() };
        let CValue::Str(s) = &c[key] else { panic!() };
        s.clone()
    }

    fn source(&self) -> SourceRef {
        SourceRef::from_value(&self.0["source"]).unwrap()
    }
}

/// A temporary folder holding the export root `out`; removed on drop.
struct Tmp {
    raw: PathBuf,
}

impl Tmp {
    fn new(name: &str) -> Tmp {
        let raw = std::env::temp_dir().join(format!("fid-io-export-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&raw);
        fs::create_dir_all(raw.join("out")).unwrap();
        Tmp { raw }
    }

    fn out(&self) -> PathBuf {
        self.raw.join("out")
    }

    fn roots(&self) -> RootSet {
        let mut s = RootSet::new();
        s.insert(RootId::new("out").unwrap(), RootAccess::Export, self.out());
        s.insert(RootId::new("in").unwrap(), RootAccess::Import, self.raw.join("in"));
        s
    }
}

impl Drop for Tmp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.raw);
    }
}

fn listing(dir: &Path) -> Vec<String> {
    let mut v: Vec<String> =
        fs::read_dir(dir).unwrap().map(|e| e.unwrap().file_name().into_string().unwrap()).collect();
    v.sort();
    v
}

fn sha_of(bytes: &[u8]) -> String {
    let d = Sha256::digest(bytes);
    format!("sha256:{}", d.iter().map(|b| format!("{b:02x}")).collect::<String>())
}

fn params_json(extra: &str, target: &str) -> CValue {
    let exp = Expected::load();
    let src = match exp.source() {
        SourceRef::Selection(ids) => ids[0].clone(),
        SourceRef::One(id) => id,
    };
    let text = format!("{{\"source\":[\"{src}\"],\"format\":\"fid-test-points\",\"target\":\"{target}\"{extra}}}");
    fid_canon::parse(text.as_bytes()).unwrap()
}

fn generic<'w>(w: &'w TestPoints) -> ExportFile<'w, Points> {
    let mut f = ExportFile::new();
    f.add(Box::new(w)).unwrap();
    f
}

fn io_err(e: ExportError) -> IoError {
    match e {
        ExportError::Io(e) => e,
        other => panic!("expected an IoError, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------------------
// AC-1 .. AC-6

#[test]
fn ac1_export_writes_the_file_and_one_event_holds_its_digest() {
    let exp = Expected::load();
    let t = Tmp::new("ac1");
    let (reg, roots, src, w) = (registry(), t.roots(), points(), test_points());
    let env = ExportEnv::new(&reg, &roots, Principal::Script);
    let out = generic(&w).run_value(&env, &params_json("", "root://out/a.ftp"), &src).unwrap();

    assert_eq!(listing(&t.out()), ["a.ftp"]);
    let file = fs::read(t.out().join("a.ftp")).unwrap();
    assert_eq!(sha_of(&file), exp.get("stride1", "sha256"));
    assert_eq!(file.len().to_string(), exp.get("stride1", "bytes"));
    assert_eq!(out.report.sha256().as_str(), sha_of(&file));
    assert_eq!(out.report.canonical_bytes().unwrap(), exp.get("stride1", "report").as_bytes());

    let event = out.event.unwrap();
    assert_eq!(event.command.as_str(), EXPORT_FILE);
    assert_eq!(event.sha256.as_str(), sha_of(&file));
    assert_eq!(event.to_json().unwrap(), exp.get("stride1", "event").as_bytes());
    assert_eq!(ExportEvent::from_json(exp.get("stride1", "event").as_bytes()).unwrap(), event);

    // Options are recorded as effective options: stride 3 (from the reference too).
    let t3 = Tmp::new("ac1s3");
    let roots3 = t3.roots();
    let env3 = ExportEnv::new(&reg, &roots3, Principal::Script);
    let out3 =
        generic(&w).run_value(&env3, &params_json(",\"options\":{\"stride\":3}", "root://out/a.ftp"), &src).unwrap();
    assert_eq!(sha_of(&fs::read(t3.out().join("a.ftp")).unwrap()), exp.get("stride3", "sha256"));
    assert_eq!(out3.event.unwrap().to_json().unwrap(), exp.get("stride3", "event").as_bytes());
}

#[test]
fn ac2_dry_run_reports_the_same_digest_and_leaves_the_root_unchanged() {
    let exp = Expected::load();
    let t = Tmp::new("ac2");
    fs::write(t.out().join("keep.txt"), b"keep").unwrap();
    let (reg, roots, src, w) = (registry(), t.roots(), points(), test_points());
    let env = ExportEnv::new(&reg, &roots, Principal::Agent);
    let out = generic(&w).run_value(&env, &params_json(",\"dry_run\":true", "root://out/a.ftp"), &src).unwrap();
    assert_eq!(out.report.sha256().as_str(), exp.get("stride1", "sha256"));
    assert!(out.report.dry_run());
    assert_eq!(out.report.canonical_bytes().unwrap(), exp.get("stride1", "dry_report").as_bytes());
    assert!(out.event.is_none(), "a dry run journals nothing");
    assert_eq!(listing(&t.out()), ["keep.txt"]);
    assert_eq!(fs::read(t.out().join("keep.txt")).unwrap(), b"keep");

    // A dry run runs the same checks: an existing target without overwrite is refused.
    let e = generic(&w).run_value(&env, &params_json(",\"dry_run\":true", "root://out/keep.txt"), &src).unwrap_err();
    assert_eq!(io_err(e), IoError::TargetExists);
    // ... and over the size bound it fails like the atomic writer, writing nothing.
    let small = ExportEnv { max_bytes: 100, ..env };
    let e = generic(&w).run_value(&small, &params_json(",\"dry_run\":true", "root://out/a.ftp"), &src).unwrap_err();
    assert!(matches!(io_err(e.clone()), IoError::LimitExceeded { limit: LimitKind::Bytes, value: 100, .. }));
    assert_eq!(e.bus_code(), ("validation", "effect_limit"));
    assert_eq!(listing(&t.out()), ["keep.txt"]);
}

#[test]
fn ac3_a_writer_failing_after_10_kib_leaves_nothing_and_journals_nothing() {
    let t = Tmp::new("ac3");
    let (reg, roots, src, w) = (registry(), t.roots(), points(), test_points());
    let env = ExportEnv::new(&reg, &roots, Principal::Script);
    let mut f = ExportFile::new();
    f.add(Box::new(FailAfter { inner: &w, n: 10 * 1024 })).unwrap();
    let r = f.run_value(&env, &params_json("", "root://out/a.ftp"), &src);
    let e = r.unwrap_err();
    assert!(matches!(e, ExportError::Writer(IoError::Io { .. })), "{e:?}");
    assert_eq!(e.bus_code(), ("internal", "handler_failed"));
    assert!(listing(&t.out()).is_empty(), "no file, no temp file: {:?}", listing(&t.out()));

    // A failing sink (disk) after 10 KiB is an I/O fault, not a writer failure.
    let faulted = ExportEnv { fault: Some(Fault::FailAfter(10 * 1024)), ..env };
    let e = generic(&w).run_value(&faulted, &params_json("", "root://out/a.ftp"), &src).unwrap_err();
    assert!(matches!(e, ExportError::Io(IoError::Io { .. })), "{e:?}");
    assert_eq!(e.bus_code(), ("internal", "storage"));
    assert!(listing(&t.out()).is_empty());

    // The same through the per-format command, and for a dry run.
    let cmd = ExportCommand::new(FailAfter { inner: &w, n: 10 * 1024 }, &[]).unwrap();
    let mut p = ExportParams::from_value(&params_json("", "root://out/a.ftp"), true).unwrap();
    p.format = None;
    assert!(cmd.run(&env, &p, &src).is_err());
    p.dry_run = true;
    assert!(cmd.run(&env, &p, &src).is_err());
    assert!(listing(&t.out()).is_empty());
}

/// Sets the cancel flag once `after` bytes have been written.
struct CancelAfter<'a> {
    inner: &'a TestPoints,
    flag: &'a AtomicBool,
    after: u64,
}

struct CancelSink<'a> {
    inner: &'a mut dyn ByteSink,
    flag: &'a AtomicBool,
    after: u64,
}

impl ByteSink for CancelSink<'_> {
    fn write_bytes(&mut self, buf: &[u8]) -> Result<(), IoError> {
        if self.inner.bytes_written() >= self.after {
            self.flag.store(true, Ordering::SeqCst);
        }
        self.inner.write_bytes(buf)
    }

    fn bytes_written(&self) -> u64 {
        self.inner.bytes_written()
    }
}

impl Writer for CancelAfter<'_> {
    type Source = Points;

    fn format(&self) -> &FormatId {
        self.inner.format()
    }

    fn version(&self) -> &VersionToken {
        self.inner.version()
    }

    fn write(
        &self,
        src: &Points,
        opts: &CValue,
        sink: &mut dyn ByteSink,
        r: &mut ReportBuilder,
    ) -> Result<(), IoError> {
        let mut s = CancelSink { inner: sink, flag: self.flag, after: self.after };
        self.inner.write(src, opts, &mut s, r)
    }
}

#[test]
fn ac3_a_cancel_mid_write_leaves_nothing_and_journals_nothing() {
    let t = Tmp::new("cancel");
    let (reg, roots, src, w) = (registry(), t.roots(), points(), test_points());
    let flag = AtomicBool::new(false);
    let env = ExportEnv { cancel: Some(&flag), ..ExportEnv::new(&reg, &roots, Principal::Script) };
    let mut f = ExportFile::new();
    f.add(Box::new(CancelAfter { inner: &w, flag: &flag, after: 5000 })).unwrap();
    let e = f.run_value(&env, &params_json("", "root://out/a.ftp"), &src).unwrap_err();
    assert_eq!(e, ExportError::Cancelled);
    assert_eq!(e.bus_code(), ("cancelled", "requested"));
    assert!(listing(&t.out()).is_empty());
}

#[test]
fn ac4_an_extension_of_another_format_fails_format_mismatch() {
    let t = Tmp::new("ac4");
    let (reg, roots, src, w) = (registry(), t.roots(), points(), test_points());
    let script = ExportEnv::new(&reg, &roots, Principal::Script);
    let e = generic(&w).run_value(&script, &params_json("", "root://out/a.stl"), &src).unwrap_err();
    assert_eq!(e, ExportError::Io(IoError::FormatMismatch));
    assert_eq!(e.bus_code(), ("policy", "extension_refused"));
    // Also for a dry run, and case-insensitively.
    let e = generic(&w).run_value(&script, &params_json(",\"dry_run\":true", "root://out/a.STL"), &src).unwrap_err();
    assert_eq!(e, ExportError::Io(IoError::FormatMismatch));
    assert!(listing(&t.out()).is_empty());

    // force_extension is human-only.
    let e = generic(&w).run_value(&script, &params_json(",\"force_extension\":true", "root://out/a.stl"), &src);
    let e = e.unwrap_err();
    assert_eq!(e, ExportError::ForceExtensionHumanOnly);
    assert_eq!(e.bus_code(), ("policy", "human_only"));
    let agent = ExportEnv::new(&reg, &roots, Principal::Agent);
    let e = generic(&w).run_value(&agent, &params_json(",\"force_extension\":true", "root://out/a.ftp"), &src);
    assert_eq!(e.unwrap_err(), ExportError::ForceExtensionHumanOnly);
    assert!(listing(&t.out()).is_empty());

    // A human may force it.
    let human = ExportEnv::new(&reg, &roots, Principal::Human);
    generic(&w).run_value(&human, &params_json(",\"force_extension\":true", "root://out/a.stl"), &src).unwrap();
    assert_eq!(listing(&t.out()), ["a.stl"]);

    // An extension that only an import format declares, or none, is not a mismatch.
    generic(&w).run_value(&script, &params_json("", "root://out/b.txt"), &src).unwrap();
    generic(&w).run_value(&script, &params_json("", "root://out/noext"), &src).unwrap();
    assert_eq!(listing(&t.out()), ["a.stl", "b.txt", "noext"]);
}

#[test]
fn ac5_an_agent_with_a_raw_path_gets_raw_path_forbidden() {
    let t = Tmp::new("ac5");
    let (reg, roots, src, w) = (registry(), t.roots(), points(), test_points());
    let raw = t.out().join("a.ftp").to_string_lossy().replace('\\', "/");
    for principal in [Principal::Agent, Principal::Script] {
        let env = ExportEnv::new(&reg, &roots, principal);
        let e = generic(&w).run_value(&env, &params_json("", &raw), &src).unwrap_err();
        assert_eq!(e, ExportError::Io(IoError::RawPathForbidden));
        assert_eq!(e.bus_code(), ("policy", "raw_path_refused"));
        // Even for a dry run.
        let e = generic(&w).run_value(&env, &params_json(",\"dry_run\":true", &raw), &src).unwrap_err();
        assert_eq!(e, ExportError::Io(IoError::RawPathForbidden));
    }
    assert!(listing(&t.out()).is_empty());

    // A human may pass a raw path; its record is not replayable into a root.
    let human = ExportEnv::new(&reg, &roots, Principal::Human);
    let out = generic(&w).run_value(&human, &params_json("", &raw), &src).unwrap();
    assert_eq!(listing(&t.out()), ["a.ftp"]);
    let event = out.event.unwrap();
    assert!(event.target.as_handle().is_none());
    assert_eq!(ExportEvent::from_json(&event.to_json().unwrap()).unwrap(), event);
    fs::remove_file(t.out().join("a.ftp")).unwrap();
    assert_eq!(generic(&w).replay(&human, &event, &src).unwrap(), ReplayOutcome::NotReplayable);
}

#[test]
fn ac6_replay_into_an_empty_root_is_identical() {
    let exp = Expected::load();
    let (reg, src, w) = (registry(), points(), test_points());
    let event = ExportEvent::from_json(exp.get("stride1", "event").as_bytes()).unwrap();

    let t = Tmp::new("ac6");
    let roots = t.roots();
    let env = ExportEnv::new(&reg, &roots, Principal::Script);
    assert_eq!(generic(&w).replay(&env, &event, &src).unwrap(), ReplayOutcome::Identical);
    assert_eq!(sha_of(&fs::read(t.out().join("a.ftp")).unwrap()), exp.get("stride1", "sha256"));

    // Another writer version is DIVERGENT (and still writes nothing it should not).
    let t2 = Tmp::new("ac6b");
    let roots2 = t2.roots();
    let env2 = ExportEnv::new(&reg, &roots2, Principal::Script);
    let w2 = TestPoints { version: VersionToken::new("1.0.1").unwrap(), ..test_points() };
    assert_eq!(generic(&w2).replay(&env2, &event, &src).unwrap(), ReplayOutcome::Divergent);
    // Other input points: DIVERGENT.
    let t3 = Tmp::new("ac6c");
    let roots3 = t3.roots();
    let env3 = ExportEnv::new(&reg, &roots3, Principal::Script);
    let mut other = points();
    other.pts[0][0] += 1;
    assert_eq!(generic(&w).replay(&env3, &event, &other).unwrap(), ReplayOutcome::Divergent);
    // A record of another command is not replayed by the per-format command.
    let t4 = Tmp::new("ac6d");
    let roots4 = t4.roots();
    let env4 = ExportEnv::new(&reg, &roots4, Principal::Script);
    let cmd = ExportCommand::new(&w, &[]).unwrap();
    assert_eq!(cmd.replay(&env4, &event, &src).unwrap(), ReplayOutcome::NotReplayable);
    assert!(listing(&t4.out()).is_empty());
    // Replaying into a root that already holds the file does not clobber it.
    let e = generic(&w).replay(&env, &event, &src).unwrap_err();
    assert_eq!(e, ExportError::Io(IoError::TargetExists));
}

// ---------------------------------------------------------------------------------------
// Confinement, clobbering, parameters

#[test]
fn targets_stay_inside_their_export_root() {
    let t = Tmp::new("conf");
    let (reg, roots, src, w) = (registry(), t.roots(), points(), test_points());
    let env = ExportEnv::new(&reg, &roots, Principal::Agent);
    let cases = [
        ("root://out/../a.ftp", IoError::PathOutsideRoot, ("policy", "outside_root")),
        ("root://out/sub/../../a.ftp", IoError::PathOutsideRoot, ("policy", "outside_root")),
        ("root://out/C:/a.ftp", IoError::PathOutsideRoot, ("policy", "outside_root")),
        ("root://nope/a.ftp", IoError::RootNotFound, ("policy", "bad_root")),
        ("root://in/a.ftp", IoError::RootNotFound, ("policy", "bad_root")),
    ];
    for (target, want, bus) in cases {
        for dry in [false, true] {
            let extra = if dry { ",\"dry_run\":true" } else { "" };
            let e = generic(&w).run_value(&env, &params_json(extra, target), &src).unwrap_err();
            assert_eq!(e, ExportError::Io(want.clone()), "{target}");
            assert_eq!(e.bus_code(), bus, "{target}");
        }
    }
    // A missing folder below the root is a precondition, not a fault.
    let e = generic(&w).run_value(&env, &params_json("", "root://out/missing/a.ftp"), &src).unwrap_err();
    assert_eq!(e.bus_code(), ("precondition", "false"));
    assert!(listing(&t.out()).is_empty());
    assert_eq!(listing(&t.raw), ["out"]);
}

#[cfg(unix)]
#[test]
fn a_link_out_of_the_root_is_refused() {
    let t = Tmp::new("link");
    let outside = t.raw.join("outside");
    fs::create_dir_all(&outside).unwrap();
    std::os::unix::fs::symlink(&outside, t.out().join("esc")).unwrap();
    std::os::unix::fs::symlink(outside.join("a.ftp"), t.out().join("b.ftp")).unwrap();
    let (reg, roots, src, w) = (registry(), t.roots(), points(), test_points());
    let env = ExportEnv::new(&reg, &roots, Principal::Agent);
    for target in ["root://out/esc/a.ftp", "root://out/b.ftp"] {
        for dry in [false, true] {
            let extra = if dry { ",\"dry_run\":true" } else { "" };
            let e = generic(&w).run_value(&env, &params_json(extra, target), &src).unwrap_err();
            assert_eq!(e, ExportError::Io(IoError::PathOutsideRoot), "{target}");
        }
    }
    assert!(listing(&outside).is_empty());
}

#[test]
fn an_existing_target_is_never_clobbered_without_overwrite() {
    let t = Tmp::new("clobber");
    fs::write(t.out().join("a.ftp"), b"old").unwrap();
    let (reg, roots, src, w) = (registry(), t.roots(), points(), test_points());
    let env = ExportEnv::new(&reg, &roots, Principal::Script);
    let e = generic(&w).run_value(&env, &params_json("", "root://out/a.ftp"), &src).unwrap_err();
    assert_eq!(e, ExportError::Io(IoError::TargetExists));
    assert_eq!(e.bus_code(), ("policy", "would_overwrite"));
    assert_eq!(fs::read(t.out().join("a.ftp")).unwrap(), b"old");
    // With overwrite the file is replaced atomically.
    let out = generic(&w).run_value(&env, &params_json(",\"overwrite\":true", "root://out/a.ftp"), &src).unwrap();
    assert_eq!(sha_of(&fs::read(t.out().join("a.ftp")).unwrap()), out.report.sha256().as_str());
    assert_eq!(listing(&t.out()), ["a.ftp"]);
    // A folder at the target is protected even with overwrite.
    fs::create_dir(t.out().join("d.ftp")).unwrap();
    let e = generic(&w).run_value(&env, &params_json(",\"overwrite\":true", "root://out/d.ftp"), &src).unwrap_err();
    assert!(matches!(e, ExportError::TargetProtected(_)), "{e:?}");
    assert_eq!(e.bus_code(), ("policy", "target_protected"));
    // ... and so is a read-only file; the dry run says the same.
    fs::write(t.out().join("r.ftp"), b"ro").unwrap();
    let mut perm = fs::metadata(t.out().join("r.ftp")).unwrap().permissions();
    perm.set_readonly(true);
    fs::set_permissions(t.out().join("r.ftp"), perm).unwrap();
    for extra in [",\"overwrite\":true", ",\"overwrite\":true,\"dry_run\":true"] {
        let e = generic(&w).run_value(&env, &params_json(extra, "root://out/r.ftp"), &src).unwrap_err();
        assert_eq!(e.bus_code(), ("policy", "target_protected"), "{extra}");
    }
    assert_eq!(fs::read(t.out().join("r.ftp")).unwrap(), b"ro");
    let mut perm = fs::metadata(t.out().join("r.ftp")).unwrap().permissions();
    #[allow(clippy::permissions_set_readonly_false)]
    perm.set_readonly(false);
    fs::set_permissions(t.out().join("r.ftp"), perm).unwrap();
}

#[test]
fn parameters_are_read_strictly() {
    let t = Tmp::new("params");
    let (reg, roots, src, w) = (registry(), t.roots(), points(), test_points());
    let env = ExportEnv::new(&reg, &roots, Principal::Script);
    let id = "0190f3a2-7c41-7d2e-9b10-3c5e8f6a4d21";
    let bad = [
        format!("{{\"source\":\"{id}\",\"format\":\"fid-test-points\",\"target\":\"root://out/a.ftp\",\"x\":1}}"),
        format!("{{\"source\":\"{id}\",\"target\":\"root://out/a.ftp\"}}"),
        "{\"source\":\"not-an-id\",\"format\":\"fid-test-points\",\"target\":\"root://out/a.ftp\"}".to_string(),
        "{\"source\":[],\"format\":\"fid-test-points\",\"target\":\"root://out/a.ftp\"}".to_string(),
        format!("{{\"source\":\"{id}\",\"format\":\"fid-test-points\",\"target\":\"\"}}"),
        format!("{{\"source\":\"{id}\",\"format\":\"fid-test-points\",\"target\":\"root://out/a.ftp\",\"dry_run\":1}}"),
        format!("{{\"source\":\"{id}\",\"format\":\"fid-test-points\",\"target\":\"root://out/a.ftp\",\"options\":[]}}"),
        format!("{{\"source\":\"{id}\",\"source\":\"{id}\",\"format\":\"fid-test-points\",\"target\":\"root://out/a.ftp\"}}"),
    ];
    for b in &bad {
        let e = ExportParams::from_json(b.as_bytes(), true).unwrap_err();
        assert_eq!(e.bus_code(), ("validation", "schema"), "{b}");
    }
    // The per-format command takes no `format`.
    let with = format!("{{\"source\":\"{id}\",\"format\":\"fid-test-points\",\"target\":\"root://out/a.ftp\"}}");
    assert!(ExportParams::from_json(with.as_bytes(), false).is_err());
    let one = ExportParams::from_json(with.as_bytes(), true).unwrap();
    assert_eq!(one.source, SourceRef::One(id.to_string()));
    assert_eq!(ExportParams::from_value(&one.to_value(), true).unwrap(), one);

    // Bad options are refused by the writer before any file is touched.
    let e = generic(&w).run_value(&env, &params_json(",\"options\":{\"stride\":0}", "root://out/a.ftp"), &src);
    assert_eq!(e.unwrap_err().bus_code(), ("validation", "schema"));
    // An unregistered or writer-less format.
    let p = fid_canon::parse(
        format!("{{\"source\":\"{id}\",\"format\":\"fid-test-stl\",\"target\":\"root://out/a.stl\"}}").as_bytes(),
    )
    .unwrap();
    let e = generic(&w).run_value(&env, &p, &src).unwrap_err();
    assert_eq!(e, ExportError::Io(IoError::FormatUnknown));
    assert_eq!(e.bus_code(), ("validation", "schema"));
    let empty = Registry::new();
    let env_empty = ExportEnv::new(&empty, &roots, Principal::Script);
    let e = generic(&w).run_value(&env_empty, &params_json("", "root://out/a.ftp"), &src).unwrap_err();
    assert_eq!(e, ExportError::Io(IoError::FormatUnknown));
    assert!(listing(&t.out()).is_empty());

    // Two writers for one format.
    let mut f = generic(&w);
    assert_eq!(f.add(Box::new(&w)).unwrap_err(), ExportError::Io(IoError::FormatDuplicate));
}

#[test]
fn journal_records_are_read_back_strictly() {
    let exp = Expected::load();
    let good = exp.get("stride1", "event");
    let event = ExportEvent::from_json(good.as_bytes()).unwrap();
    assert_eq!(event.to_json().unwrap(), good.as_bytes());
    let bad = [
        good.replacen("{", "{\"extra\":1,", 1),
        good.replacen("\"bytes\":\"26755\",", "", 1),
        good.replacen("\"bytes\":\"26755\"", "\"bytes\":26755", 1),
        good.replacen("\"bytes\":\"26755\"", "\"bytes\":\"026755\"", 1),
        good.replacen("\"lossy\":[\"point_set.color\"]", "\"lossy\":[\"point_set.color\",\"point_set.color\"]", 1),
        good.replacen("\"command\":\"export.file@1\"", "\"command\":\"export.file\"", 1),
        good.replacen("\"writer_version\":\"1.0.0\"", "\"writer_version\":\"\"", 1),
        good.replacen("{", "{\"bytes\":\"1\",", 1),
        // Not canonical: whitespace, or keys out of order.
        good.replacen(",", ", ", 1),
        format!("{} ", good),
        good.replacen(
            "\"bytes\":\"26755\",\"command\":\"export.file@1\"",
            "\"command\":\"export.file@1\",\"bytes\":\"26755\"",
            1,
        ),
        good.replacen("\"principal\":\"script\"", "\"principal\":\"root\"", 1),
        good.replacen("\"overwrite\":false", "\"overwrite\":0", 1),
    ];
    for b in &bad {
        assert_ne!(b, &good);
        let e = ExportEvent::from_json(b.as_bytes()).unwrap_err();
        assert_eq!(e.bus_code(), ("validation", "schema"), "{b}");
    }
    // `lossy` is capped at MAX_LOSSY names.
    let names = |n: usize| (0..n).map(|i| format!("\"n{i:05}\"")).collect::<Vec<_>>().join(",");
    let at_cap = good.replacen("[\"point_set.color\"]", &format!("[{}]", names(fid_io_reg::report::MAX_LOSSY)), 1);
    assert_eq!(ExportEvent::from_json(at_cap.as_bytes()).unwrap().lossy.len(), fid_io_reg::report::MAX_LOSSY);
    let over = good.replacen("[\"point_set.color\"]", &format!("[{}]", names(fid_io_reg::report::MAX_LOSSY + 1)), 1);
    assert!(ExportEvent::from_json(over.as_bytes()).is_err());
}

#[test]
fn counting_sink_matches_sha256() {
    let mut s = CountingSink::new(10);
    s.write_bytes(b"abc").unwrap();
    s.write_bytes(b"").unwrap();
    assert_eq!(s.bytes_written(), 3);
    let r = s.finish().unwrap();
    // Hand-checked: SHA-256("abc") from FIPS 180-2 appendix B.1.
    assert_eq!(
        r.sha256,
        Digest::parse("sha256:ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad").unwrap()
    );
    let mut s = CountingSink::new(2);
    assert!(s.write_bytes(b"abc").is_err());
    assert!(s.write_bytes(b"").is_err(), "the first error sticks");
    assert!(s.finish().is_err());
}

// ---------------------------------------------------------------------------------------
// Bus error mapping

fn catalogue_v3() -> BTreeMap<String, BTreeSet<String>> {
    let CValue::Obj(o) = fid_canon::parse(BUS_ERRORS_V3).unwrap() else { panic!() };
    let CValue::Obj(codes) = &o["codes"] else { panic!() };
    codes
        .iter()
        .map(|(c, rs)| {
            let CValue::Arr(rs) = rs else { panic!() };
            (c.clone(), rs.iter().map(|r| if let CValue::Str(s) = r { s.clone() } else { panic!() }).collect())
        })
        .collect()
}

#[test]
fn every_failure_maps_into_bus_catalogue_v3_and_refusals_are_never_internal() {
    use std::io::ErrorKind;
    let cat = catalogue_v3();
    let at = Position::byte(7);
    let refusals = vec![
        ExportError::Io(IoError::RawPathForbidden),
        ExportError::Io(IoError::PathOutsideRoot),
        ExportError::Io(IoError::RootNotFound),
        ExportError::Io(IoError::TargetExists),
        ExportError::Io(IoError::FormatMismatch),
        ExportError::Io(IoError::FormatUnknown),
        ExportError::Io(IoError::FormatAmbiguous),
        ExportError::Io(IoError::UnitRequired),
        ExportError::Io(IoError::UnitConflict),
        ExportError::Io(IoError::LimitExceeded { limit: LimitKind::Bytes, value: 1, at }),
        ExportError::Io(IoError::LimitExceeded { limit: LimitKind::Wall, value: 1, at }),
        ExportError::Io(IoError::LimitRaiseForbidden { limit: LimitKind::Bytes, requested: 2, default: 1 }),
        ExportError::Io(IoError::Io { kind: ErrorKind::NotFound, at }),
        ExportError::TargetProtected(IoError::Io { kind: ErrorKind::PermissionDenied, at }),
        ExportError::Cancelled,
        ExportError::ForceExtensionHumanOnly,
        ExportError::BadParams("x"),
        ExportError::BadOptions("x"),
        ExportError::BadEvent("x"),
    ];
    let faults = vec![
        ExportError::Io(IoError::Io { kind: ErrorKind::Other, at }),
        ExportError::Io(IoError::Io { kind: ErrorKind::PermissionDenied, at }),
        ExportError::Io(IoError::Io { kind: ErrorKind::InvalidInput, at }),
        ExportError::Writer(IoError::Io { kind: ErrorKind::Other, at }),
        ExportError::Writer(IoError::Malformed { at, reason: "r" }),
        ExportError::CommandIdTaken,
        ExportError::Io(IoError::Malformed { at, reason: "r" }),
        ExportError::Io(IoError::TruncatedFile { at }),
        ExportError::Io(IoError::ReaderCrashed),
        ExportError::Io(IoError::FormatDuplicate),
        ExportError::Internal("x"),
    ];
    for e in refusals.iter().chain(&faults) {
        let (code, reason) = e.bus_code();
        assert!(cat.get(code).is_some_and(|rs| rs.contains(reason)), "{e:?} -> {code}/{reason} not in catalogue v3");
    }
    for e in &refusals {
        assert_ne!(e.bus_code().0, "internal", "{e:?} is a refusal");
    }
    for e in &faults {
        assert_eq!(e.bus_code().0, "internal", "{e:?} is a fault");
    }
    // An OS permission error while writing is a storage fault, never the policy refusal.
    assert_eq!(
        ExportError::Io(IoError::Io { kind: ErrorKind::PermissionDenied, at }).bus_code(),
        ("internal", "storage")
    );
    assert_eq!(ExportError::Writer(IoError::TargetExists).bus_code(), ("internal", "handler_failed"));
    // Every IoError code is covered by the two lists.
    let covered: BTreeSet<&str> = refusals.iter().chain(&faults).map(|e| e.code()).collect();
    for c in fid_io_reg::CODES {
        assert!(covered.contains(c), "{c} not covered");
    }
}

// ---------------------------------------------------------------------------------------
// Command definitions against real params and results

/// A report as surfaces print it: its canonical JSON, parsed back.
fn wire(r: &fid_io_reg::ExportReport) -> CValue {
    fid_canon::parse(&r.canonical_bytes().unwrap()).unwrap()
}

/// Patterns are evaluated with a real regex engine, so the published strings are what is tested.
fn matches_pattern(p: &str, x: &str) -> bool {
    regex::Regex::new(p).unwrap().is_match(x)
}

fn type_ok(t: &str, v: &CValue) -> bool {
    match (t, v) {
        ("object", CValue::Obj(_)) | ("array", CValue::Arr(_)) | ("string", CValue::Str(_)) => true,
        ("boolean", CValue::Bool(_)) | ("null", CValue::Null) | ("number", CValue::Num(_)) => true,
        ("integer", CValue::Num(n)) => n.fract() == 0.0,
        _ => false,
    }
}

/// A checker for the JSON Schema subset the definitions use (after fid-geo-cmd's tests). An
/// unknown keyword fails the test, so a schema change must extend this checker on purpose.
fn conforms(schema: &CValue, v: &CValue, at: &str) -> Result<(), String> {
    let CValue::Obj(s) = schema else { return Err(format!("{at}: schema is not an object")) };
    for (k, kv) in s {
        let ok = match (k.as_str(), kv) {
            ("description", _) | ("default", _) | ("x-ref", _) | ("x-selector", _) | ("x-untrusted", _) => true,
            ("path_like", _) | ("x-extensions", _) => true,
            ("type", CValue::Str(t)) => type_ok(t, v),
            ("type", CValue::Arr(ts)) => ts.iter().any(|t| matches!(t, CValue::Str(t) if type_ok(t, v))),
            ("pattern", CValue::Str(p)) => match v {
                CValue::Str(x) => matches_pattern(p, x),
                _ => true,
            },
            ("minLength", CValue::Num(n)) => !matches!(v, CValue::Str(x) if (x.chars().count() as f64) < *n),
            ("maxLength", CValue::Num(n)) => !matches!(v, CValue::Str(x) if (x.chars().count() as f64) > *n),
            ("minimum", CValue::Num(m)) => !matches!(v, CValue::Num(n) if n < m),
            ("required", CValue::Arr(r)) => match v {
                CValue::Obj(o) => r.iter().all(|k| matches!(k, CValue::Str(k) if o.contains_key(k))),
                _ => true,
            },
            ("additionalProperties", CValue::Bool(false)) => match (v, s.get("properties")) {
                (CValue::Obj(o), Some(CValue::Obj(props))) => o.keys().all(|k| props.contains_key(k)),
                (CValue::Obj(_), _) => return Err(format!("{at}: additionalProperties without properties")),
                _ => true,
            },
            ("properties", CValue::Obj(props)) => {
                if let CValue::Obj(o) = v {
                    for (pk, ps) in props {
                        if let Some(pv) = o.get(pk) {
                            conforms(ps, pv, &format!("{at}/{pk}"))?;
                        }
                    }
                }
                true
            }
            ("items", item) => {
                if let CValue::Arr(a) = v {
                    for (i, x) in a.iter().enumerate() {
                        conforms(item, x, &format!("{at}/{i}"))?;
                    }
                }
                true
            }
            ("anyOf", CValue::Arr(branches)) => branches.iter().any(|b| conforms(b, v, at).is_ok()),
            ("minItems", CValue::Num(n)) => !matches!(v, CValue::Arr(a) if (a.len() as f64) < *n),
            ("maxItems", CValue::Num(n)) => !matches!(v, CValue::Arr(a) if (a.len() as f64) > *n),
            (other, _) => return Err(format!("{at}: keyword {other} not supported by the test checker")),
        };
        if !ok {
            return Err(format!("{at}: fails {k}"));
        }
    }
    Ok(())
}

#[test]
fn command_definitions_are_valid_w1_write_root_registry_records() {
    let reg = registry();
    let geo = fid_registry::Catalogue::geo_frame().unwrap();
    let file = export_file_def(&reg).unwrap();
    let fmt = FormatId::new("fid-test-points").unwrap();
    let per = format_def(&reg, &fmt, &[EXPORT_FILE, "export.list_formats@1"]).unwrap();
    assert_eq!(file.id, "export.file");
    assert_eq!(per.id, "export.fid_test_points");
    assert_eq!(format_command_id(&fmt, &[]).unwrap().as_str(), "export.fid_test_points@1");
    assert_eq!(ExportCommand::new(test_points(), &[]).unwrap().id().as_str(), "export.fid_test_points@1");
    for def in [&file, &per] {
        assert_eq!(def.side_effect.as_str(), "W1");
        assert_eq!(def.fs_effect.as_str(), "write_root");
        assert_eq!(def.determinism.as_str(), "D0");
        assert_eq!(def.effects, [format!("{}@1", def.id)]);
        assert_eq!(fid_registry::check_schema(&def.params_schema, &geo), vec![]);
        assert_eq!(fid_registry::check_schema(&def.result_schema, &geo), vec![]);
        let decl = fid_registry::validate::CommandDecl::from_def(def);
        assert_eq!(fid_registry::validate(&[decl]), vec![], "{}", def.id);
    }
    // x-extensions lists every export extension (not the import-only `txt`).
    let CValue::Obj(p) = &file.params_schema else { panic!() };
    let CValue::Obj(props) = &p["properties"] else { panic!() };
    let CValue::Obj(target) = &props["target"] else { panic!() };
    assert_eq!(target["x-extensions"], CValue::Arr(vec![CValue::Str("ftp".into()), CValue::Str("stl".into())]));
    assert_eq!(target["path_like"], CValue::Bool(true));
    // No export format with an extension: no definition (the root guard would refuse every target).
    assert!(params_schema(&Registry::new(), true).is_err());
}

#[test]
fn real_params_and_results_conform_to_the_published_schemas() {
    let exp = Expected::load();
    let reg = registry();
    let file = export_file_def(&reg).unwrap();
    let per = format_def(&reg, &FormatId::new("fid-test-points").unwrap(), &[]).unwrap();
    assert_eq!(per.result_schema, result_schema());

    // Results: the reference reports (real and dry run) and a report produced here with a
    // warning-free human raw path target.
    for key in ["report", "dry_report"] {
        let v = fid_canon::parse(exp.get("stride1", key).as_bytes()).unwrap();
        conforms(&file.result_schema, &v, key).unwrap();
    }
    let t = Tmp::new("schema");
    let (roots, src, w) = (t.roots(), points(), test_points());
    let env = ExportEnv::new(&reg, &roots, Principal::Script);
    let real_params =
        params_json(",\"options\":{\"stride\":3},\"overwrite\":false,\"dry_run\":false", "root://out/a.ftp");
    conforms(&file.params_schema, &real_params, "params").unwrap();
    let out = generic(&w).run_value(&env, &real_params, &src).unwrap();
    conforms(&file.result_schema, &wire(&out.report), "result").unwrap();

    // The per-format command: the same params without `format`, its result too.
    let CValue::Obj(mut o) = real_params.clone() else { panic!() };
    o.remove("format");
    let per_params = CValue::Obj(o);
    conforms(&per.params_schema, &per_params, "per params").unwrap();
    let cmd = ExportCommand::new(&w, &[]).unwrap();
    fs::remove_file(t.out().join("a.ftp")).unwrap();
    let out = cmd.run_value(&env, &per_params, &src).unwrap();
    conforms(&per.result_schema, &wire(&out.report), "per result").unwrap();
    assert!(conforms(&per.params_schema, &real_params, "format on per").is_err());

    // Params the schema must refuse.
    let id = "0190f3a2-7c41-7d2e-9b10-3c5e8f6a4d21";
    for bad in [
        "{}".to_string(),
        format!("{{\"source\":\"{id}\",\"target\":\"root://out/a.ftp\"}}"),
        format!("{{\"source\":\"{}\",\"format\":\"fid-test-points\",\"target\":\"root://out/a.ftp\"}}", id.to_uppercase()),
        "{\"source\":[],\"format\":\"fid-test-points\",\"target\":\"root://out/a.ftp\"}".to_string(),
        format!("{{\"source\":\"{id}\",\"format\":\"Bad\",\"target\":\"root://out/a.ftp\"}}"),
        format!("{{\"source\":\"{id}\",\"format\":\"fid-test-points\",\"target\":\"\"}}"),
        format!("{{\"source\":\"{id}\",\"format\":\"fid-test-points\",\"target\":\"root://out/a.ftp\",\"dry_run\":\"yes\"}}"),
        format!("{{\"source\":\"{id}\",\"format\":\"fid-test-points\",\"target\":\"root://out/a.ftp\",\"x\":1}}"),
    ] {
        let v = fid_canon::parse(bad.as_bytes()).unwrap();
        assert!(conforms(&file.params_schema, &v, &bad).is_err(), "{bad}");
        // ... and the command refuses them as well.
        assert!(ExportParams::from_value(&v, true).is_err(), "{bad}");
    }
    // Results the schema must refuse.
    let CValue::Obj(good) = fid_canon::parse(exp.get("stride1", "report").as_bytes()).unwrap() else { panic!() };
    for (k, v) in [
        ("sha256", CValue::Str("sha256:xyz".into())),
        ("bytes", CValue::Num(26755.0)),
        ("lossy", CValue::Arr(vec![CValue::Str("Point".into())])),
    ] {
        let mut bad = good.clone();
        bad.insert(k.to_string(), v);
        assert!(conforms(&file.result_schema, &CValue::Obj(bad), k).is_err(), "{k}");
    }
    let mut bad = good.clone();
    bad.remove("target");
    assert!(conforms(&file.result_schema, &CValue::Obj(bad), "no target").is_err());
}

// ---------------------------------------------------------------------------------------
// Published patterns

#[test]
fn published_patterns_accept_and_refuse_the_right_strings() {
    let cases: [(&str, &[&str], &[&str]); 6] = [
        (
            ENTITY_ID_PATTERN,
            &["0190f3a2-7c41-7d2e-9b10-3c5e8f6a4d21", "00000000-0000-0000-0000-000000000000"],
            &[
                "0190F3A2-7C41-7D2E-9B10-3C5E8F6A4D21",
                "0190f3a27c417d2e9b103c5e8f6a4d21",
                "0190f3a2-7c41-7d2e-9b10-3c5e8f6a4d2",
                "",
            ],
        ),
        (
            FORMAT_ID_PATTERN,
            &["fid-test-points", "mesh.stl", "mesh.stl-ascii", "3mf", "a"],
            &["fid_test_points", "Mesh", "-a", ".a", "", "a b", &"a".repeat(65)],
        ),
        (
            U64_PATTERN,
            &[
                "0",
                "1",
                "26755",
                "9999999999999999999",
                "10000000000000000000",
                "18446744073709551614",
                "18446744073709551615",
            ],
            &[
                "",
                "01",
                "-1",
                "1.0",
                "18446744073709551616",
                "18446744073709551620",
                "18446744073709552000",
                "19000000000000000000",
                "99999999999999999999",
                "100000000000000000000",
            ],
        ),
        (
            DIGEST_PATTERN,
            &["sha256:ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"],
            &[
                "sha256:BA7816BF8F01CFEA414140DE5DAE2223B00361A396177A9CB410FF61F20015AD",
                "sha256:ab",
                "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
            ],
        ),
        (CODE_PATTERN, &["unit_assumed", "a", "nan_coordinate2"], &["Unit", "_a", "1a", "a-b", "", &"a".repeat(65)]),
        (
            TYPE_NAME_PATTERN,
            &["point_set", "point_set.color", "mesh.normals"],
            &["point_set.", ".a", "Point", "a..b", "a-b", ""],
        ),
    ];
    for (p, good, bad) in cases {
        let re = regex::Regex::new(p).unwrap();
        for g in good {
            assert!(re.is_match(g), "{p} should accept {g:?}");
        }
        for b in bad {
            assert!(!re.is_match(b), "{p} should refuse {b:?}");
        }
    }
    // The patterns agree with the Rust validators on the same strings.
    let format_re = regex::Regex::new(FORMAT_ID_PATTERN).unwrap();
    for x in ["fid-test-points", "fid_test_points", "mesh.stl", "-a", "Mesh"] {
        assert_eq!(format_re.is_match(x), FormatId::new(x).is_ok(), "{x}");
    }
    let u64_re = regex::Regex::new(U64_PATTERN).unwrap();
    for x in ["18446744073709551615", "18446744073709551616", "0", "01"] {
        let parsed = x.parse::<u64>().is_ok() && (x == "0" || !x.starts_with('0'));
        assert_eq!(u64_re.is_match(x), parsed, "{x}");
    }
}

// ---------------------------------------------------------------------------------------
// Command IDs

#[test]
fn per_format_command_ids_are_one_to_one_and_never_collide() {
    // Format IDs hold no `_`, so `-` -> `_` cannot merge two formats.
    assert!(FormatId::new("fid_test_points").is_err());
    let a = format_command_id(&FormatId::new("mesh-stl").unwrap(), &[]).unwrap();
    let b = format_command_id(&FormatId::new("mesh.stl").unwrap(), &[]).unwrap();
    assert_eq!(a.as_str(), "export.mesh_stl@1");
    assert_eq!(b.as_str(), "export.mesh.stl@1");
    assert_ne!(a, b);
    // Reserved names: export.file and export.list_formats.
    for f in ["file", "list-formats"] {
        let e = format_command_id(&FormatId::new(f).unwrap(), &[]).unwrap_err();
        assert_eq!(e, ExportError::CommandIdTaken, "{f}");
        assert_eq!(e.bus_code(), ("internal", "invariant_violated"));
    }
    // Any other export command already in the registry, with or without `@major`.
    let reg = registry();
    let fmt = FormatId::new("fid-test-points").unwrap();
    for taken in ["export.fid_test_points@1", "export.fid_test_points", "export.fid_test_points@2"] {
        assert_eq!(format_command_id(&fmt, &[taken]).unwrap_err(), ExportError::CommandIdTaken, "{taken}");
        assert_eq!(ExportCommand::new(test_points(), &[taken]).unwrap_err(), ExportError::CommandIdTaken);
        assert_eq!(format_def(&reg, &fmt, &[taken]).unwrap_err(), ExportError::CommandIdTaken);
    }
    assert!(format_def(&reg, &FormatId::new("file").unwrap(), &[]).is_err());
    assert!(format_command_id(&fmt, &["export.other@1", "import.fid_test_points@1"]).is_ok());
    // A format ID that would not end a command name.
    assert!(format_command_id(&FormatId::new("abc-").unwrap(), &[]).is_err());
}

// ---------------------------------------------------------------------------------------
// REQ-004 on the resolved name

#[test]
fn the_extension_check_sees_the_resolved_file_name() {
    let t = Tmp::new("resolved");
    let (reg, roots, src, w) = (registry(), t.roots(), points(), test_points());
    let human = ExportEnv::new(&reg, &roots, Principal::Human);
    let script = ExportEnv::new(&reg, &roots, Principal::Script);
    let dir = t.out().to_string_lossy().replace('\\', "/");
    for name in ["a.stl.", "a.stl ", "a.stl::$DATA", "a.ftp:x", "CON.ftp"] {
        // A human's raw path: refused before the extension check could be dodged.
        let raw = format!("{dir}/{name}");
        for extra in ["", ",\"dry_run\":true"] {
            let e = generic(&w).run_value(&human, &params_json(extra, &raw), &src).unwrap_err();
            assert_eq!(e, ExportError::Io(IoError::PathOutsideRoot), "{name:?}");
            assert_eq!(e.bus_code(), ("policy", "outside_root"));
        }
        // A handle: refused by the lexical check of IO.REG.ROOTS.
        let e = generic(&w).run_value(&script, &params_json("", &format!("root://out/{name}")), &src).unwrap_err();
        assert_eq!(e, ExportError::Io(IoError::PathOutsideRoot), "{name:?}");
    }
    assert!(listing(&t.out()).is_empty());
    // The raw path's resolved name is what is checked: `.STL` names fid-test-stl.
    let e = generic(&w).run_value(&human, &params_json("", &format!("{dir}/a.STL")), &src).unwrap_err();
    assert_eq!(e, ExportError::Io(IoError::FormatMismatch));
    assert!(listing(&t.out()).is_empty());
}

#[cfg(windows)]
#[test]
fn a_junction_out_of_the_root_is_refused() {
    let t = Tmp::new("junction");
    let outside = t.raw.join("outside");
    fs::create_dir_all(&outside).unwrap();
    let ok = std::process::Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(t.out().join("j"))
        .arg(&outside)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    assert!(ok, "mklink /J needs no privilege");
    let (reg, roots, src, w) = (registry(), t.roots(), points(), test_points());
    let env = ExportEnv::new(&reg, &roots, Principal::Agent);
    for extra in ["", ",\"dry_run\":true"] {
        let e = generic(&w).run_value(&env, &params_json(extra, "root://out/j/a.ftp"), &src).unwrap_err();
        assert_eq!(e, ExportError::Io(IoError::PathOutsideRoot));
        assert_eq!(e.bus_code(), ("policy", "outside_root"));
    }
    assert!(listing(&outside).is_empty());
}

// ---------------------------------------------------------------------------------------
// AC-6 with a forced extension; prepare and place

#[test]
fn a_human_forced_export_replays_identically_and_only_for_a_human_record() {
    let t = Tmp::new("force");
    let (reg, roots, src, w) = (registry(), t.roots(), points(), test_points());
    let human = ExportEnv::new(&reg, &roots, Principal::Human);
    let out =
        generic(&w).run_value(&human, &params_json(",\"force_extension\":true", "root://out/a.stl"), &src).unwrap();
    let event = out.event.unwrap();
    assert!(event.force_extension && !event.overwrite);
    assert_eq!(event.principal, Principal::Human);
    assert_eq!(ExportEvent::from_json(&event.to_json().unwrap()).unwrap(), event);

    // Replayed by a script into an empty root: the recorded human's force is honoured.
    let t2 = Tmp::new("force2");
    let roots2 = t2.roots();
    let script = ExportEnv::new(&reg, &roots2, Principal::Script);
    assert_eq!(generic(&w).replay(&script, &event, &src).unwrap(), ReplayOutcome::Identical);
    assert_eq!(listing(&t2.out()), ["a.stl"]);
    // A record claiming a forced extension for a script is not replayable.
    let t3 = Tmp::new("force3");
    let roots3 = t3.roots();
    let script3 = ExportEnv::new(&reg, &roots3, Principal::Script);
    let forged = ExportEvent { principal: Principal::Script, ..event.clone() };
    assert_eq!(generic(&w).replay(&script3, &forged, &src).unwrap(), ReplayOutcome::NotReplayable);
    assert!(listing(&t3.out()).is_empty());
    // A recorded overwrite is replayed too: over an existing file it replaces it.
    let t4 = Tmp::new("force4");
    fs::write(t4.out().join("a.ftp"), b"old").unwrap();
    let roots4 = t4.roots();
    let env4 = ExportEnv::new(&reg, &roots4, Principal::Script);
    let out = generic(&w).run_value(&env4, &params_json(",\"overwrite\":true", "root://out/a.ftp"), &src).unwrap();
    let ev = out.event.unwrap();
    assert!(ev.overwrite);
    assert_eq!(generic(&w).replay(&env4, &ev, &src).unwrap(), ReplayOutcome::Identical);
}

#[test]
fn prepare_writes_only_a_temp_file_and_place_or_discard_finishes_it() {
    let exp = Expected::load();
    let t = Tmp::new("prepare");
    let (reg, roots, src, w) = (registry(), t.roots(), points(), test_points());
    let env = ExportEnv::new(&reg, &roots, Principal::Script);
    let f = generic(&w);
    let p = ExportParams::from_value(&params_json("", "root://out/a.ftp"), true).unwrap();

    let prepared = f.prepare(&env, &p, &src).unwrap();
    // The record and its report digest exist before anything is placed.
    let ev = prepared.event().unwrap().clone();
    assert_eq!(ev.to_json().unwrap(), exp.get("stride1", "event").as_bytes());
    assert_eq!(prepared.report().sha256().as_str(), exp.get("stride1", "sha256"));
    let names = listing(&t.out());
    assert_eq!(names.len(), 1);
    assert!(names[0].starts_with(".fid-tmp-"), "{names:?}");
    prepared.discard();
    assert!(listing(&t.out()).is_empty());

    let prepared = f.prepare(&env, &p, &src).unwrap();
    // A target that appears between prepare and place is not clobbered.
    fs::write(t.out().join("a.ftp"), b"raced").unwrap();
    assert_eq!(prepared.place().unwrap_err(), ExportError::Io(IoError::TargetExists));
    assert_eq!(listing(&t.out()), ["a.ftp"]);
    assert_eq!(fs::read(t.out().join("a.ftp")).unwrap(), b"raced");
    fs::remove_file(t.out().join("a.ftp")).unwrap();

    let out = f.prepare(&env, &p, &src).unwrap().place().unwrap();
    assert_eq!(out.event.unwrap(), ev);
    assert_eq!(sha_of(&fs::read(t.out().join("a.ftp")).unwrap()), exp.get("stride1", "sha256"));

    // A bus-level dry run becomes the export's own dry run: no file, not even a temp one.
    let mut p2 = ExportParams::from_value(&params_json("", "root://out/b.ftp"), true).unwrap();
    p2.apply_bus_dry_run(true);
    assert!(p2.dry_run);
    let prepared = f.prepare(&env, &p2, &src).unwrap();
    assert!(prepared.event().is_none());
    assert_eq!(listing(&t.out()), ["a.ftp"]);
    assert!(prepared.place().unwrap().event.is_none());
    assert_eq!(listing(&t.out()), ["a.ftp"]);
    p2.apply_bus_dry_run(false);
    assert!(p2.dry_run, "a bus flag never clears the export's own dry run");
}

// ---------------------------------------------------------------------------------------
// Conformance kit (REQ-005)

struct Kit {
    reg: Registry,
    source: SourceRef,
    options: CValue,
    sha: Digest,
    variants: Vec<CValue>,
    format: FormatId,
}

fn kit() -> Kit {
    let exp = Expected::load();
    Kit {
        reg: registry(),
        source: exp.source(),
        options: CValue::Obj(BTreeMap::new()),
        sha: Digest::parse(&exp.get("stride1", "sha256")).unwrap(),
        variants: vec![
            fid_canon::parse(b"{\"stride\":3}").unwrap(),
            fid_canon::parse(b"{\"stride\":0}").unwrap(),
            fid_canon::parse(b"{\"other\":1}").unwrap(),
        ],
        format: FormatId::new("fid-test-points").unwrap(),
    }
}

fn fixture<'a>(k: &'a Kit, roots: &'a RootSet, dir: &'a Path) -> Fixture<'a> {
    Fixture {
        registry: &k.reg,
        roots,
        root_id: "out",
        root_dir: dir,
        format: &k.format,
        file_name: "kit.ftp",
        source: &k.source,
        options: &k.options,
        expected_sha256: &k.sha,
        option_variants: &k.variants,
        fail_after: 10 * 1024,
    }
}

#[test]
fn both_commands_pass_the_conformance_kit() {
    let k = kit();
    let t = Tmp::new("kit");
    let (roots, out, src, w) = (t.roots(), t.out(), points(), test_points());
    let fx = fixture(&k, &roots, &out);
    let file = generic(&w);
    conformance::check(&file, &export_file_def(&k.reg).unwrap(), &src, &fx).unwrap();
    assert!(listing(&out).is_empty(), "the kit removes what it wrote");
    let cmd = ExportCommand::new(&w, &[EXPORT_FILE]).unwrap();
    conformance::check(&cmd, &format_def(&k.reg, &k.format, &[EXPORT_FILE]).unwrap(), &src, &fx).unwrap();
    assert!(listing(&out).is_empty());
    // The wrong definition for a command is caught.
    let e = conformance::check(&cmd, &export_file_def(&k.reg).unwrap(), &src, &fx).unwrap_err();
    assert_eq!(e.step, "definition");
}

/// A writer whose output changes on every call (a timestamp stand-in): it must fail the kit.
struct Unstable {
    inner: TestPoints,
    calls: Cell<u64>,
}

impl Writer for Unstable {
    type Source = Points;

    fn format(&self) -> &FormatId {
        self.inner.format()
    }

    fn version(&self) -> &VersionToken {
        self.inner.version()
    }

    fn options(&self, given: &CValue) -> Result<CValue, &'static str> {
        self.inner.options(given)
    }

    fn write(
        &self,
        src: &Points,
        opts: &CValue,
        sink: &mut dyn ByteSink,
        r: &mut ReportBuilder,
    ) -> Result<(), IoError> {
        self.calls.set(self.calls.get() + 1);
        sink.write_bytes(format!("# {}\n", self.calls.get()).as_bytes())?;
        self.inner.write(src, opts, sink, r)
    }
}

/// A writer whose output is stable within a process but differs from the stored digest
/// (as hash-order output would between processes).
struct Shifted(TestPoints);

impl Writer for Shifted {
    type Source = Points;

    fn format(&self) -> &FormatId {
        self.0.format()
    }

    fn version(&self) -> &VersionToken {
        self.0.version()
    }

    fn options(&self, given: &CValue) -> Result<CValue, &'static str> {
        self.0.options(given)
    }

    fn write(
        &self,
        src: &Points,
        opts: &CValue,
        sink: &mut dyn ByteSink,
        r: &mut ReportBuilder,
    ) -> Result<(), IoError> {
        sink.write_bytes(b"#\n")?;
        self.0.write(src, opts, sink, r)
    }
}

/// A writer that accepts options but ignores them.
struct Deaf(TestPoints);

impl Writer for Deaf {
    type Source = Points;

    fn format(&self) -> &FormatId {
        self.0.format()
    }

    fn version(&self) -> &VersionToken {
        self.0.version()
    }

    fn options(&self, given: &CValue) -> Result<CValue, &'static str> {
        self.0.options(given)
    }

    fn write(&self, src: &Points, _: &CValue, sink: &mut dyn ByteSink, r: &mut ReportBuilder) -> Result<(), IoError> {
        self.0.write(src, &CValue::Obj(BTreeMap::new()), sink, r)
    }
}

/// A writer that swallows sink errors and writes a side file.
struct Leaky<'a> {
    inner: TestPoints,
    dir: &'a Path,
}

impl Writer for Leaky<'_> {
    type Source = Points;

    fn format(&self) -> &FormatId {
        self.inner.format()
    }

    fn version(&self) -> &VersionToken {
        self.inner.version()
    }

    fn options(&self, given: &CValue) -> Result<CValue, &'static str> {
        self.inner.options(given)
    }

    fn write(
        &self,
        src: &Points,
        opts: &CValue,
        sink: &mut dyn ByteSink,
        r: &mut ReportBuilder,
    ) -> Result<(), IoError> {
        let _ = fs::write(self.dir.join("side.txt"), b"x");
        self.inner.write(src, opts, sink, r)
    }
}

fn kit_step<W: Writer<Source = Points>>(name: &str, w: W) -> &'static str {
    let k = kit();
    let t = Tmp::new(name);
    let (roots, out, src) = (t.roots(), t.out(), points());
    let fx = fixture(&k, &roots, &out);
    let mut file = ExportFile::new();
    file.add(Box::new(w)).unwrap();
    conformance::check(&file, &export_file_def(&k.reg).unwrap(), &src, &fx).unwrap_err().step
}

#[test]
fn the_conformance_kit_catches_broken_writers() {
    assert_eq!(kit_step("kit-unstable", Unstable { inner: test_points(), calls: Cell::new(0) }), "dry_run");
    assert_eq!(kit_step("kit-shifted", Shifted(test_points())), "dry_run");
    assert_eq!(kit_step("kit-deaf", Deaf(test_points())), "options");

    // Side effects outside the sink.
    let k = kit();
    let t = Tmp::new("kit-leaky");
    let (roots, out, src) = (t.roots(), t.out(), points());
    let fx = fixture(&k, &roots, &out);
    let mut file = ExportFile::new();
    file.add(Box::new(Leaky { inner: test_points(), dir: &out })).unwrap();
    assert_eq!(conformance::check(&file, &export_file_def(&k.reg).unwrap(), &src, &fx).unwrap_err().step, "dry_run");

    // An option the descriptor declares but no variant exercises.
    let t = Tmp::new("kit-novar");
    let (roots, out) = (t.roots(), t.out());
    let empty: Vec<CValue> = vec![fid_canon::parse(b"{\"other\":1}").unwrap()];
    let fx = Fixture { option_variants: &empty, ..fixture(&k, &roots, &out) };
    let w = test_points();
    let e = conformance::check(&generic(&w), &export_file_def(&k.reg).unwrap(), &src, &fx).unwrap_err();
    assert_eq!(e.step, "options");

    // A non-empty root is refused up front.
    let t = Tmp::new("kit-busy");
    fs::write(t.out().join("x"), b"").unwrap();
    let (roots, out) = (t.roots(), t.out());
    let fx = fixture(&k, &roots, &out);
    assert_eq!(
        conformance::check(&generic(&w), &export_file_def(&k.reg).unwrap(), &src, &fx).unwrap_err().step,
        "setup"
    );
}
