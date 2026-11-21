//! Acceptance criteria of spec node IO.REG.ENV.IMPORT (AC-1 .. AC-7), the shared conformance
//! kit (REQ-006) on both commands, the untrusted-input rules (limits before allocation,
//! confinement, detection consistency, embedded text never acted on) and the record. Expected
//! values come from `fixtures/io/reg/env/import/make_fixtures.py` (independent Python
//! reference), never from a run of this code.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::{parse, to_canonical_bytes, CValue, Digest as CanonDigest};
use fid_io_reg::env::import::command::{format_def, import_file_def};
use fid_io_reg::env::import::conformance::{check, Fixture};
use fid_io_reg::env::import::{
    import_file_id, register_import_events, BlobSink, Fault, ImportCommand, ImportEnv, ImportError, ImportEvent,
    ImportFile, ImportParams, ImportRun, ImportSource, MemoryBlobs, Reader, StageSink,
};
use fid_io_reg::prov::{LengthUnit, PrincipalCommit, PrincipalKind, ReaderId, ReaderInfo, VersionToken};
use fid_io_reg::{
    Direction, FormatDescriptor, FormatId, IoError, LimitRequest, Matcher, Origin, Phase, Position, Principal,
    Registry, ReportBuilder, RootAccess, RootId, RootSet, Signature, UntrustedText, VersionRange,
};
use fid_project::{ElementTypeDef, ElementTypeRegistry, Tree, TreeEvent, Uuid, STAGE_ADDED};
use fid_replay::ReplayOutcome;
use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

const DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/io/reg/env/import/");

type Obj = BTreeMap<String, CValue>;

fn expected() -> CValue {
    parse(&fs::read(format!("{DIR}expected.json")).unwrap()).unwrap()
}

fn obj(v: &CValue) -> &Obj {
    match v {
        CValue::Obj(o) => o,
        other => panic!("expected object, got {other:?}"),
    }
}

fn get<'a>(o: &'a Obj, k: &str) -> &'a CValue {
    o.get(k).unwrap_or_else(|| panic!("missing {k}"))
}

fn st<'a>(o: &'a Obj, k: &str) -> &'a str {
    match o.get(k) {
        Some(CValue::Str(s)) => s,
        other => panic!("{k}: expected string, got {other:?}"),
    }
}

fn arr<'a>(o: &'a Obj, k: &str) -> &'a [CValue] {
    match o.get(k) {
        Some(CValue::Arr(a)) => a,
        other => panic!("{k}: expected array, got {other:?}"),
    }
}

fn canon(v: &CValue) -> Vec<u8> {
    to_canonical_bytes(v).unwrap()
}

fn case(name: &str) -> Obj {
    let e = expected();
    obj(get(obj(get(obj(&e), "cases")), name)).clone()
}

// ---------------------------------------------------------------------------------------
// The test format `fid-test-points` (grammar in the reference's docstring)

struct TestPoints {
    format: FormatId,
    info: ReaderInfo,
}

fn test_points() -> TestPoints {
    TestPoints {
        format: FormatId::new("fid-test-points").unwrap(),
        info: ReaderInfo {
            id: ReaderId::new("fid-test-points").unwrap(),
            version: VersionToken::new("1.0.0").unwrap(),
        },
    }
}

fn malformed(at: u64, reason: &'static str) -> IoError {
    IoError::Malformed { at: Position::byte(at), reason }
}

fn coord_ok(s: &str) -> bool {
    let t = s.strip_prefix('-').unwrap_or(s);
    let (a, b) = t.split_once('.').map_or((t, None), |(a, b)| (a, Some(b)));
    !a.is_empty()
        && a.bytes().all(|c| c.is_ascii_digit())
        && b.is_none_or(|b| !b.is_empty() && b.bytes().all(|c| c.is_ascii_digit()))
}

impl Reader for TestPoints {
    fn format(&self) -> &FormatId {
        &self.format
    }

    fn info(&self) -> &ReaderInfo {
        &self.info
    }

    fn read(
        &self,
        src: &mut ImportSource<'_>,
        _opts: &CValue,
        sink: &mut StageSink<'_, '_>,
        report: &mut ReportBuilder,
    ) -> Result<(), IoError> {
        // Read everything, charging the buffer growth before it happens.
        let mut data = Vec::new();
        let mut chunk = [0u8; 8192];
        loop {
            let n = src.read_bounded(&mut chunk)?;
            if n == 0 {
                break;
            }
            src.budget_mut().charge_alloc(n as u64, Position::byte(data.len() as u64))?;
            data.extend_from_slice(&chunk[..n]);
        }
        let mut lines: Vec<(u64, &[u8])> = Vec::new();
        let mut off = 0u64;
        for l in data.split(|b| *b == b'\n') {
            lines.push((off, l));
            off += l.len() as u64 + 1;
        }
        let head = std::str::from_utf8(lines[0].1).map_err(|_| malformed(0, "header"))?;
        if !head.starts_with("FTP1") {
            return Err(malformed(0, "header"));
        }
        if let Some(u) = head.strip_prefix("FTP1 unit=") {
            let unit = LengthUnit::ALL.into_iter().find(|x| x.symbol() == u).ok_or(malformed(0, "unit"))?;
            sink.declare_unit(unit);
        } else if head != "FTP1" {
            return Err(malformed(0, "header"));
        }
        let unit = head.strip_prefix("FTP1 unit=").map(str::to_string);
        let mut i = 1;
        let last = lines.len() - 1;
        while i < lines.len() {
            let (at, l) = lines[i];
            if l.is_empty() && i == last {
                break;
            }
            let name = l.strip_prefix(b"SET ").ok_or(malformed(at, "expected SET"))?;
            let name = std::str::from_utf8(name).map_err(|_| malformed(at, "name is not UTF-8"))?;
            src.budget_mut().charge_elements(1, Position::byte(at))?;
            let mut points = Vec::new();
            i += 1;
            loop {
                let Some(&(pat, p)) = lines.get(i) else {
                    return Err(IoError::TruncatedFile { at: Position::byte(off.saturating_sub(1)) });
                };
                if p.is_empty() && i == last {
                    return Err(IoError::TruncatedFile { at: Position::byte(pat) });
                }
                if p == b"END" {
                    break;
                }
                let t = std::str::from_utf8(p).map_err(|_| malformed(pat, "point"))?;
                let xyz: Vec<&str> = t.strip_prefix("P ").ok_or(malformed(pat, "expected P"))?.split(' ').collect();
                if xyz.len() != 3 || !xyz.iter().all(|c| coord_ok(c)) {
                    return Err(malformed(pat, "point"));
                }
                src.budget_mut().charge_elements(1, Position::byte(pat))?;
                points.push(CValue::Arr(xyz.iter().map(|c| CValue::Str((*c).to_string())).collect()));
                i += 1;
            }
            i += 1;
            let mut payload = Obj::new();
            payload.insert("name".into(), CValue::Str(name.to_string()));
            payload.insert("points".into(), CValue::Arr(points));
            payload.insert("unit".into(), unit.as_ref().map_or(CValue::Null, |u| CValue::Str(u.clone())));
            let bytes = to_canonical_bytes(&CValue::Obj(payload)).map_err(|_| malformed(at, "payload"))?;
            sink.add_entity("test.point_set", &UntrustedText::new(name), &bytes, Position::byte(at), report)?;
        }
        Ok(())
    }
}

/// A second test format, for the detection checks; it never stages anything.
struct TestMesh(FormatId, ReaderInfo);

impl Reader for TestMesh {
    fn format(&self) -> &FormatId {
        &self.0
    }
    fn info(&self) -> &ReaderInfo {
        &self.1
    }
    fn read(
        &self,
        _src: &mut ImportSource<'_>,
        _opts: &CValue,
        sink: &mut StageSink<'_, '_>,
        _report: &mut ReportBuilder,
    ) -> Result<(), IoError> {
        sink.declare_unit(LengthUnit::Mm);
        Ok(())
    }
}

fn test_mesh() -> TestMesh {
    TestMesh(
        FormatId::new("fid-test-mesh").unwrap(),
        ReaderInfo { id: ReaderId::new("fid-test-mesh").unwrap(), version: VersionToken::new("1.0.0").unwrap() },
    )
}

fn descriptor(id: &str, ext: &str, magic: &[u8]) -> FormatDescriptor {
    FormatDescriptor::new(
        FormatId::new(id).unwrap(),
        VersionRange::new("1", "1").unwrap(),
        vec![Signature::new(0, Matcher::Bytes(magic.to_vec()), 50).unwrap()],
        vec![ext.to_string()],
        CValue::Obj(BTreeMap::new()),
        Phase::new(0).unwrap(),
        Origin::FirstParty,
        Direction::Import,
    )
    .unwrap()
}

fn registry() -> Registry {
    let mut r = Registry::new();
    r.register(descriptor("fid-test-points", "ftp", b"FTP1")).unwrap();
    r.register(descriptor("fid-test-mesh", "ftm", b"FTM1")).unwrap();
    r
}

// ---------------------------------------------------------------------------------------
// The project and the roots

fn uuid(o: &Obj, k: &str) -> Uuid {
    Uuid::parse(st(o, k)).unwrap()
}

fn per_format_id() -> fid_types::CommandId {
    fid_types::CommandId::parse("import.fid_test_points@1").unwrap()
}

/// The project of the reference: S1 adopting the default stage, then S2; S1 active.
fn project() -> Tree {
    let e = expected();
    let o = obj(&e);
    let mut reg = ElementTypeRegistry::new();
    let mut schema = Obj::new();
    schema.insert("type".into(), CValue::Str("object".into()));
    reg.register(ElementTypeDef::new("test.point_set", CValue::Obj(schema), "point_cloud").unwrap()).unwrap();
    register_import_events(&mut reg, &[&import_file_id().unwrap(), &per_format_id()]).unwrap();
    let mut t = Tree::new(uuid(o, "namespace"), Arc::new(reg)).unwrap();
    let run = st(o, "run");
    let default = st(o, "default_stage");
    for (id, label, adopts) in [(st(o, "s1"), "S1", format!("\"{default}\"")), (st(o, "s2"), "S2", "null".to_string())]
    {
        let b = format!(
            r#"{{"adopts":{adopts},"id":"{id}","label":"{label}","metadata":{{}},"run":"{run}","serial":null}}"#
        );
        t.apply(STAGE_ADDED, &parse(b.as_bytes()).unwrap()).unwrap();
    }
    t
}

struct Tmp {
    raw: PathBuf,
}

impl Tmp {
    fn new(name: &str) -> Tmp {
        let raw = std::env::temp_dir().join(format!("fid-io-import-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&raw);
        fs::create_dir_all(raw.join("in")).unwrap();
        for f in fs::read_dir(format!("{DIR}files")).unwrap() {
            let f = f.unwrap();
            fs::copy(f.path(), raw.join("in").join(f.file_name())).unwrap();
        }
        Tmp { raw }
    }

    fn roots(&self) -> RootSet {
        let mut s = RootSet::new();
        s.insert(RootId::new("in").unwrap(), RootAccess::Import, self.raw.join("in"));
        s
    }

    fn raw_path(&self, f: &str) -> String {
        self.raw.join("in").join(f).to_string_lossy().into_owned()
    }
}

impl Drop for Tmp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.raw);
    }
}

fn commit() -> PrincipalCommit {
    PrincipalCommit { kind: PrincipalKind::Agent, commit: CanonDigest([0x5a; 32]), on_behalf_of_commit: None }
}

fn env<'a>(reg: &'a Registry, roots: &'a RootSet, tree: &'a Tree, principal: Principal) -> ImportEnv<'a> {
    let e = expected();
    ImportEnv {
        registry: reg,
        roots,
        principal,
        principal_commit: commit(),
        limits: LimitRequest::default(),
        tree,
        namespace: uuid(obj(&e), "namespace"),
        cancel: None,
        fault: None,
    }
}

fn generic<'r>(tp: TestPoints, tm: TestMesh) -> ImportFile<'r> {
    let mut f = ImportFile::new();
    f.add(Box::new(tp)).unwrap();
    f.add(Box::new(tm)).unwrap();
    f
}

fn digest(t: &Tree) -> String {
    t.state_digest().unwrap().as_str().to_string()
}

fn tree_value(e: &TreeEvent) -> CValue {
    let mut o = Obj::new();
    o.insert("body".into(), e.body.clone());
    o.insert("type".into(), CValue::Str(e.event_type.clone()));
    CValue::Obj(o)
}

// ---------------------------------------------------------------------------------------
// AC-1 .. AC-7

/// AC-1: ok.ftp imports into the active stage in one commit: the entity events and the
/// report equal the reference's, one record ends the commit, the payload is in the store and
/// the state digest after is the reference's.
#[test]
fn ac1_one_commit_one_record() {
    let c = case("ok_s1");
    let tmp = Tmp::new("ac1");
    let (reg, roots, tree) = (registry(), tmp.roots(), project());
    assert_eq!(digest(&tree), st(&c, "state_before"));
    let cmd = generic(test_points(), test_mesh());
    let mut blobs = MemoryBlobs::default();
    let p = cmd
        .prepare(
            &env(&reg, &roots, &tree, Principal::Agent),
            &ImportParams::new("root://in/ok.ftp"),
            Some(&mut blobs as &mut dyn BlobSink),
        )
        .unwrap();
    let events = p.events().to_vec();
    let want = arr(&c, "events");
    assert_eq!(events.len(), want.len() + 1);
    for (e, w) in events.iter().zip(want) {
        assert_eq!(canon(&tree_value(e)), canon(w));
    }
    assert_eq!(canon(&p.report().to_cvalue()), canon(get(&c, "report")));
    assert_eq!(p.event().report_sha256.as_str(), st(&c, "report_sha256"));
    let rec = events.last().unwrap();
    assert_eq!(rec.event_type, "import.file@1");
    let ev = p.event().clone();
    assert_eq!(canon(&rec.body), canon(&ev.to_value()));
    assert_eq!(ImportEvent::from_json(&ev.to_json().unwrap()).unwrap(), ev);
    let prov = obj(&ev.provenance);
    assert_eq!(st(prov, "sha256"), st(&c, "sha256"));
    assert_eq!(
        (st(prov, "format_selected_by"), st(prov, "unit"), st(prov, "evidence")),
        ("content_signature", "mm", "uncharacterized")
    );
    for pl in arr(&c, "payloads") {
        let pl = obj(pl);
        let d = fid_types::Digest::parse(st(pl, "digest")).unwrap();
        assert_eq!(blobs.blobs[&d], st(pl, "text").as_bytes());
    }
    let mut t = tree.clone();
    let out = p.commit(&mut t).unwrap();
    assert_eq!(digest(&t), st(&c, "state_after"));
    let counts: u64 = out.report.entity_counts().values().sum();
    assert_eq!(counts, 1);
    let sd = obj(get(&c, "stage_digests_after"));
    for s in sd.keys() {
        assert_eq!(t.stage_digest(&Uuid::parse(s).unwrap()).unwrap().as_str(), st(sd, s));
    }
}

/// AC-2: trunc.ftp fails after 1,000 staged entities: `truncated_file`
/// (validation/bad_artifact), nothing to commit, the tree unchanged, and the 1,000 stored
/// blobs referenced by nothing.
#[test]
fn ac2_failure_commits_nothing() {
    let tmp = Tmp::new("ac2");
    let (reg, roots, tree) = (registry(), tmp.roots(), project());
    let before = digest(&tree);
    let cmd = generic(test_points(), test_mesh());
    let mut blobs = MemoryBlobs::default();
    let err = cmd
        .prepare(
            &env(&reg, &roots, &tree, Principal::Agent),
            &ImportParams::new("root://in/trunc.ftp"),
            Some(&mut blobs as &mut dyn BlobSink),
        )
        .unwrap_err();
    assert_eq!(err.code(), "truncated_file", "{err}");
    assert_eq!(err.bus_code(), ("validation", "bad_artifact"));
    let e = expected();
    assert_eq!(blobs.blobs.len() as u64, num(obj(&e), "trunc_complete_sets"));
    assert_eq!(digest(&tree), before);
    assert!(tree.elements().next().is_none());
}

fn num(o: &Obj, k: &str) -> u64 {
    match o.get(k) {
        Some(CValue::Num(n)) => *n as u64,
        Some(CValue::Str(s)) => s.parse().unwrap(),
        other => panic!("{other:?}"),
    }
}

/// AC-3: `--stage S2` (by label) puts the point set in S2 only; S1's per-stage digest is
/// unchanged; the same by stage ID.
#[test]
fn ac3_named_stage_only() {
    let c = case("ok_s2");
    let tmp = Tmp::new("ac3");
    let (reg, roots, tree) = (registry(), tmp.roots(), project());
    let e = expected();
    let s1 = uuid(obj(&e), "s1");
    let s1_before = tree.stage_digest(&s1).unwrap();
    let cmd = generic(test_points(), test_mesh());
    for stage in ["S2".to_string(), st(obj(&e), "s2").to_string()] {
        let mut p = ImportParams::new("root://in/ok.ftp");
        p.stage = Some(stage);
        let prep = cmd.prepare(&env(&reg, &roots, &tree, Principal::Agent), &p, None).unwrap();
        let mut t = tree.clone();
        prep.commit(&mut t).unwrap();
        assert_eq!(digest(&t), st(&c, "state_after"));
        assert_eq!(t.stage_digest(&s1).unwrap(), s1_before);
    }
}

/// AC-4: `--stage NOPE` fails `stage_not_found` (validation/bad_stage) and the source is
/// never opened: no file-system call is made.
#[test]
fn ac4_unknown_stage_before_the_source() {
    let tmp = Tmp::new("ac4");
    let (reg, roots, tree) = (registry(), tmp.roots(), project());
    let calls = roots.fs_calls();
    let cmd = generic(test_points(), test_mesh());
    for src in ["root://in/ok.ftp", "root://in/missing.ftp"] {
        let mut p = ImportParams::new(src);
        p.stage = Some("NOPE".into());
        let err = cmd.prepare(&env(&reg, &roots, &tree, Principal::Agent), &p, None).unwrap_err();
        assert_eq!(err.code(), "stage_not_found");
        assert_eq!(err.bus_code(), ("validation", "bad_stage"));
        let per = ImportCommand::new(test_points(), &[]).unwrap();
        assert_eq!(
            per.prepare(&env(&reg, &roots, &tree, Principal::Agent), &p, None).unwrap_err().code(),
            "stage_not_found"
        );
    }
    assert_eq!(roots.fs_calls(), calls, "the source was touched");
}

/// AC-5: a dry run returns AC-1's report but for `dry_run: true`, the would-be record equals
/// the real one, nothing is committed and no blob is stored.
#[test]
fn ac5_dry_run() {
    let c = case("ok_s1");
    let tmp = Tmp::new("ac5");
    let (reg, roots, tree) = (registry(), tmp.roots(), project());
    let cmd = generic(test_points(), test_mesh());
    let mut p = ImportParams::new("root://in/ok.ftp");
    p.dry_run = true;
    let mut blobs = MemoryBlobs::default();
    let prep =
        cmd.prepare(&env(&reg, &roots, &tree, Principal::Agent), &p, Some(&mut blobs as &mut dyn BlobSink)).unwrap();
    assert_eq!(canon(&prep.report().to_cvalue()), canon(get(&c, "report_dry")));
    assert!(prep.events().is_empty());
    assert!(blobs.blobs.is_empty());
    let real =
        cmd.prepare(&env(&reg, &roots, &tree, Principal::Agent), &ImportParams::new("root://in/ok.ftp"), None).unwrap();
    assert_eq!(prep.event(), real.event());
    let mut t = tree.clone();
    prep.commit(&mut t).unwrap();
    assert_eq!(digest(&t), digest(&tree));
}

/// AC-6: a cancel after half the input (500 of 1,000 sets) equals AC-2: `cancelled`,
/// nothing committed; a cancel before the start too.
#[test]
fn ac6_cancel() {
    let tmp = Tmp::new("ac6");
    let (reg, roots, tree) = (registry(), tmp.roots(), project());
    let cmd = generic(test_points(), test_mesh());
    let mut e = env(&reg, &roots, &tree, Principal::Agent);
    e.fault = Some(Fault::CancelAfter(500));
    let err = cmd.prepare(&e, &ImportParams::new("root://in/many.ftp"), None).unwrap_err();
    assert_eq!((err.code(), err.bus_code()), ("cancelled", ("cancelled", "requested")));
    let flag = AtomicBool::new(true);
    let mut e = env(&reg, &roots, &tree, Principal::Agent);
    e.cancel = Some(&flag);
    assert_eq!(cmd.prepare(&e, &ImportParams::new("root://in/many.ftp"), None).unwrap_err().code(), "cancelled");
    let c = case("many_s1");
    assert_eq!(digest(&tree), st(&c, "state_before"));
    // The whole file imports when nothing cancels it.
    let ok = cmd
        .prepare(&env(&reg, &roots, &tree, Principal::Agent), &ImportParams::new("root://in/many.ftp"), None)
        .unwrap();
    assert_eq!(ok.event().report_sha256.as_str(), st(&c, "report_sha256"));
    let mut t = tree.clone();
    ok.commit(&mut t).unwrap();
    assert_eq!(digest(&t), st(&c, "state_after"));
}

/// AC-7: AC-1's record replayed exactly against the state before it is `IDENTICAL`; a
/// changed record is `DIVERGENT`; a human raw path is `NOT_REPLAYABLE`.
#[test]
fn ac7_replay() {
    let tmp = Tmp::new("ac7");
    let (reg, roots, tree) = (registry(), tmp.roots(), project());
    let cmd = generic(test_points(), test_mesh());
    let e = env(&reg, &roots, &tree, Principal::Agent);
    let prep = cmd.prepare(&e, &ImportParams::new("root://in/ok.ftp"), None).unwrap();
    let (ev, events) = (prep.event().clone(), prep.events().to_vec());
    assert_eq!(cmd.replay(&e, &ev, &events).unwrap(), ReplayOutcome::Identical);
    let mut other = ev.clone();
    other.options = parse(br#"{"x":1}"#).unwrap();
    assert_eq!(cmd.replay(&e, &other, &events).unwrap(), ReplayOutcome::Divergent);
    let h = env(&reg, &roots, &tree, Principal::Human);
    let raw = cmd.prepare(&h, &ImportParams::new(&tmp.raw_path("ok.ftp")), None).unwrap();
    assert_eq!(cmd.replay(&h, raw.event(), raw.events()).unwrap(), ReplayOutcome::NotReplayable);
}

// ---------------------------------------------------------------------------------------
// REQ-006: the conformance kit on both commands

#[test]
fn conformance_kit_passes_for_both_commands() {
    let tmp = Tmp::new("kit");
    let (reg, roots, tree) = (registry(), tmp.roots(), project());
    let e = expected();
    let fx = Fixture {
        registry: &reg,
        roots: &roots,
        tree: &tree,
        namespace: uuid(obj(&e), "namespace"),
        principal_commit: commit(),
        source: "root://in/many.ftp".into(),
        raw_path: Some(tmp.raw_path("many.ftp")),
        other_stage_label: "S2".into(),
        expected_entities: 1000,
        fail_after: 400,
    };
    let generic_cmd = generic(test_points(), test_mesh());
    check(&generic_cmd, &import_file_def(&reg).unwrap(), &fx).unwrap();
    let per = ImportCommand::new(test_points(), &["import.file@1"]).unwrap();
    assert_eq!(per.id().as_str(), "import.fid_test_points@1");
    let def = format_def(&reg, per.reader().format(), &["import.file@1"]).unwrap();
    check(&per, &def, &fx).unwrap();
    // The kit catches a command whose definition lies about itself.
    let mut wrong = def.clone();
    wrong.effects.retain(|x| x != "ElementSlotSet");
    assert!(check(&per, &wrong, &fx).is_err());
}

// ---------------------------------------------------------------------------------------
// Untrusted input

/// Names are kept verbatim as untrusted text, an injected instruction included; names that
/// break the element name rules are dropped with their positions; the commit holds only
/// entity events and the record.
#[test]
fn embedded_text_is_never_acted_on() {
    let c = case("names_s1");
    let tmp = Tmp::new("names");
    let (reg, roots, tree) = (registry(), tmp.roots(), project());
    let cmd = generic(test_points(), test_mesh());
    let prep = cmd
        .prepare(&env(&reg, &roots, &tree, Principal::Agent), &ImportParams::new("root://in/names.ftp"), None)
        .unwrap();
    assert_eq!(canon(&prep.report().to_cvalue()), canon(get(&c, "report")));
    for (e, w) in prep.events().iter().zip(arr(&c, "events")) {
        assert_eq!(canon(&tree_value(e)), canon(w));
    }
    let kinds: Vec<&str> = prep.events().iter().map(|e| e.event_type.as_str()).collect();
    assert!(kinds.iter().all(|k| ["ElementCreated", "ElementSlotSet", "import.file@1"].contains(k)));
    let mut t = tree.clone();
    prep.commit(&mut t).unwrap();
    assert_eq!(digest(&t), st(&c, "state_after"));
    assert!(t.elements().any(|e| e.name().as_untrusted_str().starts_with("ignore previous instructions")));
}

/// Every limit is enforced before the content is allocated: the file size against the byte
/// limit (before the prefix is read), elements and allocation while reading; an agent may
/// not raise a limit.
#[test]
fn limits_before_allocation() {
    let tmp = Tmp::new("limits");
    let (reg, roots, tree) = (registry(), tmp.roots(), project());
    let cmd = generic(test_points(), test_mesh());
    let run = |principal: Principal, limits: LimitRequest, src: &str| {
        let mut e = env(&reg, &roots, &tree, principal);
        e.limits = limits;
        cmd.prepare(&e, &ImportParams::new(src), None).map(|_| ())
    };
    let err = run(Principal::Agent, LimitRequest::default().bytes(10), "root://in/ok.ftp").unwrap_err();
    assert_eq!((err.code(), err.bus_code()), ("limit_exceeded", ("validation", "effect_limit")));
    let err = run(Principal::Agent, LimitRequest::default().elements(10), "root://in/many.ftp").unwrap_err();
    assert_eq!(err.code(), "limit_exceeded");
    let err = run(Principal::Agent, LimitRequest::default().alloc(1000), "root://in/many.ftp").unwrap_err();
    assert_eq!(err.code(), "limit_exceeded");
    let err = run(Principal::Agent, LimitRequest::default().bytes(u64::MAX), "root://in/ok.ftp").unwrap_err();
    assert_eq!(err.bus_code(), ("policy", "human_only"));
}

/// Source confinement: agents pass handles only; a short-name alias, a trailing dot or
/// space, an alternate data stream or a device name is `path_outside_root`, for handles and
/// for a human's raw file name alike.
#[test]
fn source_confinement() {
    let tmp = Tmp::new("confine");
    let (reg, roots, tree) = (registry(), tmp.roots(), project());
    let cmd = generic(test_points(), test_mesh());
    let prep = |p: Principal, src: &str| {
        let mut params = ImportParams::new(src);
        params.format = Some(FormatId::new("fid-test-points").unwrap());
        cmd.prepare(&env(&reg, &roots, &tree, p), &params, None).map(|_| ()).unwrap_err()
    };
    let raw = tmp.raw_path("ok.ftp");
    assert_eq!(prep(Principal::Agent, &raw).code(), "raw_path_forbidden");
    for bad in [
        "root://in/OKFILE~1.FTP",
        "root://in/ok.ftp.",
        "root://in/ok.ftp ",
        "root://in/ok.ftp::$DATA",
        "root://in/CON",
        "root://in/../ok.ftp",
    ] {
        let e = prep(Principal::Agent, bad);
        assert_eq!(e.bus_code(), ("policy", "outside_root"), "{bad}: {e}");
    }
    for bad in [format!("{raw}."), format!("{raw} "), format!("{raw}::$DATA"), tmp.raw_path("OKFILE~1.FTP")] {
        let e = prep(Principal::Human, &bad);
        assert_eq!(e.code(), "path_outside_root", "{bad}: {e}");
    }
}

/// Detected and declared formats must agree: content of another format, or an extension of
/// another format, is `format_mismatch` (policy/extension_refused) whether the format is
/// detected, named, or fixed by the command; a unitless file needs the caller's unit.
#[test]
fn detection_and_declared_format_agree() {
    let tmp = Tmp::new("detect");
    let (reg, roots, tree) = (registry(), tmp.roots(), project());
    let cmd = generic(test_points(), test_mesh());
    let e = env(&reg, &roots, &tree, Principal::Agent);
    let err = cmd.prepare(&e, &ImportParams::new("root://in/mesh-as.ftp"), None).unwrap_err();
    assert_eq!((err.code(), err.bus_code()), ("format_mismatch", ("policy", "extension_refused")));
    let mut named = ImportParams::new("root://in/mesh-as.ftp");
    named.format = Some(FormatId::new("fid-test-points").unwrap());
    assert_eq!(cmd.prepare(&e, &named, None).unwrap_err().code(), "format_mismatch");
    let per = ImportCommand::new(test_points(), &[]).unwrap();
    assert_eq!(per.prepare(&e, &ImportParams::new("root://in/mesh.ftm"), None).unwrap_err().code(), "format_mismatch");
    let mut wrong = ImportParams::new("root://in/ok.ftp");
    wrong.format = Some(FormatId::new("fid-test-mesh").unwrap());
    assert_eq!(cmd.prepare(&e, &wrong, None).unwrap_err().code(), "format_mismatch");
    // The mesh file under its own extension imports (no entities).
    cmd.prepare(&e, &ImportParams::new("root://in/mesh.ftm"), None).unwrap();
    // Unit: the file declares none.
    let err = cmd.prepare(&e, &ImportParams::new("root://in/unitless.ftp"), None).unwrap_err();
    assert_eq!((err.code(), err.bus_code()), ("unit_required", ("precondition", "false")));
    let mut with_unit = ImportParams::new("root://in/unitless.ftp");
    with_unit.unit = Some(LengthUnit::Mm);
    let p = cmd.prepare(&e, &with_unit, None).unwrap();
    assert_eq!(st(obj(&p.event().provenance), "unit_source"), "caller");
}

/// A second import of the same file into the same stage collides on the entity IDs
/// (conflict/id_collision) and the tree is unchanged.
#[test]
fn double_import_is_refused() {
    let tmp = Tmp::new("double");
    let (reg, roots, tree) = (registry(), tmp.roots(), project());
    let cmd = generic(test_points(), test_mesh());
    let p =
        cmd.prepare(&env(&reg, &roots, &tree, Principal::Agent), &ImportParams::new("root://in/ok.ftp"), None).unwrap();
    let mut t = tree.clone();
    p.commit(&mut t).unwrap();
    let before = digest(&t);
    let err = cmd
        .prepare(&env(&reg, &roots, &t, Principal::Agent), &ImportParams::new("root://in/ok.ftp"), None)
        .unwrap_err();
    assert_eq!(err.bus_code(), ("conflict", "id_collision"));
    assert_eq!(digest(&t), before);
}

/// Params and records are read strictly.
#[test]
fn params_and_records_are_strict() {
    for bad in [
        r#"{}"#,
        r#"{"source":""}"#,
        r#"{"source":"root://in/a.ftp","x":1}"#,
        r#"{"source":"root://in/a.ftp","unit":"furlong"}"#,
        r#"{"source":"root://in/a.ftp","stage":""}"#,
    ] {
        assert_eq!(ImportParams::from_json(bad.as_bytes(), true).unwrap_err().code(), "bad_params", "{bad}");
    }
    assert_eq!(
        ImportParams::from_json(br#"{"format":"fid-test-points","source":"root://in/a.ftp"}"#, false)
            .unwrap_err()
            .code(),
        "bad_params"
    );
    let p = ImportParams::from_json(br#"{"dry_run":true,"source":"root://in/a.ftp","stage":"S2","unit":"mm"}"#, true)
        .unwrap();
    assert_eq!((p.dry_run, p.stage.as_deref(), p.unit), (true, Some("S2"), Some(LengthUnit::Mm)));
    let tmp = Tmp::new("records");
    let (reg, roots, tree) = (registry(), tmp.roots(), project());
    let cmd = generic(test_points(), test_mesh());
    let prep =
        cmd.prepare(&env(&reg, &roots, &tree, Principal::Agent), &ImportParams::new("root://in/ok.ftp"), None).unwrap();
    let json = prep.event().to_json().unwrap();
    let mut spaced = b" ".to_vec();
    spaced.extend_from_slice(&json);
    assert_eq!(ImportEvent::from_json(&spaced).unwrap_err().code(), "bad_event");
    let mut v = prep.event().to_value();
    if let CValue::Obj(o) = &mut v {
        if let Some(CValue::Obj(p)) = o.get_mut("provenance") {
            p.insert("evidence".into(), CValue::Str("measured".into()));
        }
    }
    assert_eq!(ImportEvent::from_value(&v).unwrap_err().code(), "bad_event", "a changed provenance is caught");
}

/// Refusals map to catalogue v3 pairs.
#[test]
fn bus_codes() {
    let cases: Vec<(ImportError, (&str, &str))> = vec![
        (ImportError::StageNotFound, ("validation", "bad_stage")),
        (ImportError::StageAmbiguous, ("validation", "bad_stage")),
        (ImportError::Cancelled, ("cancelled", "requested")),
        (ImportError::BadParams("x"), ("validation", "schema")),
        (ImportError::Io(IoError::RawPathForbidden), ("policy", "raw_path_refused")),
        (ImportError::Io(IoError::RootNotFound), ("policy", "bad_root")),
        (
            ImportError::Reader(IoError::Malformed { at: Position::byte(0), reason: "x" }),
            ("validation", "bad_artifact"),
        ),
        (ImportError::Reader(IoError::ReaderCrashed), ("internal", "handler_failed")),
        (ImportError::Io(IoError::FormatUnknown), ("validation", "schema")),
        (ImportError::Provenance("x".into()), ("validation", "bad_artifact")),
        (ImportError::Internal("x"), ("internal", "invariant_violated")),
    ];
    for (e, want) in cases {
        assert_eq!(e.bus_code(), want, "{e}");
    }
}

/// The CAS adapter stores each payload as one blob addressed by its SHA-256.
#[cfg(feature = "cas")]
#[test]
fn cas_blobs_store_payloads() {
    use fid_io_reg::env::import::CasBlobs;
    let tmp = Tmp::new("cas");
    let store = fid_store::cas::BlobStore::open(&tmp.raw.join("blobs")).unwrap();
    let (reg, roots, tree) = (registry(), tmp.roots(), project());
    let cmd = generic(test_points(), test_mesh());
    let mut cas = CasBlobs(&store);
    let p = cmd
        .prepare(
            &env(&reg, &roots, &tree, Principal::Agent),
            &ImportParams::new("root://in/ok.ftp"),
            Some(&mut cas as &mut dyn BlobSink),
        )
        .unwrap();
    let c = case("ok_s1");
    let pl = obj(&arr(&c, "payloads")[0]);
    let d = fid_types::Digest::parse(st(pl, "digest")).unwrap();
    assert_eq!(p.event().blobs, vec![d]);
    assert!(store.has_blob(&d).unwrap());
}
