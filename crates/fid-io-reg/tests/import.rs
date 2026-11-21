//! Acceptance criteria of spec node IO.REG.ENV.IMPORT (AC-1 .. AC-7), the shared conformance
//! kit (REQ-006) on both commands, the untrusted-input rules (one budget for reader, sink and
//! commit, the entity cap, limits before allocation, confinement, detection consistency,
//! names never costing an entity, embedded text never acted on), cancellation inside the
//! source, the record (full form, readable at its bounds) and the bus codes. Expected values
//! come from `fixtures/io/reg/env/import/make_fixtures.py` (independent Python reference) or
//! are hand-labelled from the spec's rules, never from a run of this code.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::{parse, to_canonical_bytes, CValue, Digest as CanonDigest};
use fid_io_reg::env::import::command::{format_def, import_file_def};
use fid_io_reg::env::import::conformance::{check, Fixture};
use fid_io_reg::env::import::{
    entity_cost, import_file_id, register_import_events, BlobSink, Fault, ImportCommand, ImportEnv, ImportError,
    ImportEvent, ImportFile, ImportParams, ImportRun, ImportSource, MemoryBlobs, Reader, StageSink,
    MAX_IMPORT_ENTITIES, MAX_JSON_BYTES, PROVENANCE_META_KEY,
};
use fid_io_reg::prov::{
    LengthUnit, PrincipalCommit, PrincipalKind, ReaderId, ReaderInfo, VersionToken, MAX_DECLARED, MAX_OPTIONS_BYTES,
    MAX_OPTIONS_DEPTH, MAX_TEXT_BYTES,
};
use fid_io_reg::{
    Direction, FormatDescriptor, FormatId, IoError, LimitKind, LimitRequest, Location, Matcher, Origin, Phase,
    Position, Principal, Registry, ReportBuilder, RootAccess, RootHandle, RootId, RootSet, Signature, UntrustedText,
    VersionRange,
};
use fid_project::{ElementTypeDef, ElementTypeRegistry, Tree, TreeError, TreeEvent, Uuid, STAGE_ADDED};
use fid_replay::ReplayOutcome;
use std::cell::Cell;
use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
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

/// Reads the whole source, charging every growth of the buffer before it happens.
fn read_all(src: &mut ImportSource<'_>) -> Result<Vec<u8>, IoError> {
    let mut data = Vec::new();
    let mut chunk = [0u8; 8192];
    loop {
        let n = src.read_bounded(&mut chunk)?;
        if n == 0 {
            return Ok(data);
        }
        let need = data.len() + n;
        if need > data.capacity() {
            let cap = need.max(data.capacity() * 2);
            src.budget_mut().charge_alloc((cap - data.capacity()) as u64, Position::byte(data.len() as u64))?;
            data.reserve_exact(cap - data.len());
        }
        data.extend_from_slice(&chunk[..n]);
    }
}

impl Reader for TestPoints {
    fn format(&self) -> &FormatId {
        &self.format
    }

    fn info(&self) -> &ReaderInfo {
        &self.info
    }

    fn read(&self, _opts: &CValue, sink: &mut StageSink<'_, '_>, report: &mut ReportBuilder) -> Result<(), IoError> {
        let data = read_all(sink.source())?;
        let n_lines = data.iter().filter(|b| **b == b'\n').count() as u64 + 1;
        let line_size = std::mem::size_of::<(u64, &[u8])>() as u64;
        sink.source().budget_mut().charge_alloc(2 * n_lines * line_size, Position::byte(0))?;
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
        sink.declare_format_version(VersionToken::new("1").unwrap());
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
            sink.source().budget_mut().charge_elements(1, Position::byte(at))?;
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
                sink.source().budget_mut().charge_elements(1, Position::byte(pat))?;
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
    fn read(&self, _opts: &CValue, sink: &mut StageSink<'_, '_>, _report: &mut ReportBuilder) -> Result<(), IoError> {
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

fn stage_added(id: &str, label: &str, adopts: &str, run: &str) -> CValue {
    let b =
        format!(r#"{{"adopts":{adopts},"id":"{id}","label":"{label}","metadata":{{}},"run":"{run}","serial":null}}"#);
    parse(b.as_bytes()).unwrap()
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
    t.apply(STAGE_ADDED, &stage_added(st(o, "s1"), "S1", &format!("\"{default}\""), run)).unwrap();
    t.apply(STAGE_ADDED, &stage_added(st(o, "s2"), "S2", "null", run)).unwrap();
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

    /// Writes a `fid-test-points` file of `n` sets without points (hand-labelled: `n` entities).
    fn tiny(&self, f: &str, n: u64) {
        let mut s = String::from("FTP1 unit=mm\n");
        for i in 0..n {
            s.push_str(&format!("SET t{i}\nEND\n"));
        }
        fs::write(self.raw.join("in").join(f), s).unwrap();
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
    ImportEnv::new(reg, roots, principal, commit(), tree, uuid(obj(&e), "namespace"))
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

fn assert_events(events: &[TreeEvent], want: &[CValue]) {
    assert_eq!(events.len(), want.len());
    for (e, w) in events.iter().zip(want) {
        assert_eq!(canon(&tree_value(e)), canon(w), "{}", e.event_type);
    }
}

// ---------------------------------------------------------------------------------------
// AC-1 .. AC-7

/// AC-1: ok.ftp imports into the active stage in one commit: every event, the record (in
/// full: provenance with the reader's format version, entity IDs, manifest digest and count)
/// and the report equal the reference's; the payload and the manifest are in the store; the
/// state digest after is the reference's.
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
    assert_events(p.events(), arr(&c, "events"));
    assert_eq!(canon(&p.report().to_cvalue()), canon(get(&c, "report")));
    assert_eq!(canon(&p.event().to_value()), canon(get(&c, "record")));
    let rec = p.events().last().unwrap();
    assert_eq!(rec.event_type, "import.file@1");
    let ev = p.event().clone();
    assert_eq!(ImportEvent::from_json(&ev.to_json().unwrap()).unwrap(), ev);
    assert_eq!(st(obj(&ev.provenance), "format_version"), "1");
    assert_eq!(p.manifest(), st(&c, "manifest").as_bytes());
    assert_eq!(blobs.blobs[&ev.blob_manifest], st(&c, "manifest").as_bytes());
    for pl in arr(&c, "payloads") {
        let pl = obj(pl);
        let d = fid_types::Digest::parse(st(pl, "digest")).unwrap();
        assert_eq!(blobs.blobs[&d], st(pl, "text").as_bytes());
    }
    assert_eq!(blobs.blobs.len(), 2, "the payload and the manifest");
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

/// VIZ.OVERLAY.STATUS REQ-007: every imported element carries the provenance marker in its
/// `meta` (claimed, the provenance's evidence, the record's provenance digest), and the record
/// names the elements.
#[test]
fn imported_values_are_claimed() {
    let tmp = Tmp::new("claimed");
    let (reg, roots, tree) = (registry(), tmp.roots(), project());
    let cmd = generic(test_points(), test_mesh());
    let p = cmd
        .prepare(&env(&reg, &roots, &tree, Principal::Agent), &ImportParams::new("root://in/names.ftp"), None)
        .unwrap();
    let ev = p.event().clone();
    let mut t = tree.clone();
    p.commit(&mut t).unwrap();
    assert_eq!(ev.entities.len(), 8);
    for id in &ev.entities {
        let e = t.element(id).unwrap();
        let marker = obj(get(obj(e.meta()), PROVENANCE_META_KEY));
        assert_eq!(get(marker, "claimed"), &CValue::Bool(true));
        assert_eq!(st(marker, "evidence"), "uncharacterized");
        assert_eq!(st(marker, "provenance_sha256"), ev.provenance_sha256.as_str());
    }
    assert_eq!(t.elements().count(), 8);
}

/// AC-2: trunc.ftp fails after 1,000 staged entities: `truncated_file`
/// (validation/bad_artifact), nothing to commit, the tree unchanged, and the 1,000 stored
/// blobs referenced by nothing (no manifest is stored).
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
/// unchanged; the same by stage ID; the record is the reference's.
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
        assert_eq!(canon(&prep.event().to_value()), canon(get(&c, "record")));
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

/// A label two live stages carry is `stage_ambiguous` (validation/bad_stage), before the
/// source is opened; the stage IDs still work.
#[test]
fn ambiguous_stage_label() {
    let tmp = Tmp::new("ambiguous");
    let (reg, roots) = (registry(), tmp.roots());
    let e = expected();
    let mut tree = project();
    let s3 = "0190aaaa-0000-7000-8000-0000000000a3";
    tree.apply(STAGE_ADDED, &stage_added(s3, "S2", "null", st(obj(&e), "run"))).unwrap();
    let calls = roots.fs_calls();
    let generic_cmd = generic(test_points(), test_mesh());
    let per = ImportCommand::new(test_points(), &[]).unwrap();
    for cmd in [&generic_cmd as &dyn ImportRun, &per] {
        let mut p = ImportParams::new("root://in/ok.ftp");
        p.stage = Some("S2".into());
        let err = cmd.prepare(&env(&reg, &roots, &tree, Principal::Agent), &p, None).unwrap_err();
        assert_eq!((err.code(), err.bus_code()), ("stage_ambiguous", ("validation", "bad_stage")));
    }
    assert_eq!(roots.fs_calls(), calls, "the source was touched");
    let mut p = ImportParams::new("root://in/ok.ftp");
    p.stage = Some(s3.into());
    let prep = generic_cmd.prepare(&env(&reg, &roots, &tree, Principal::Agent), &p, None).unwrap();
    assert_eq!(prep.event().stage.to_string(), s3);
}

/// AC-5: a dry run returns AC-1's report but for `dry_run: true`, the would-be record equals
/// the real one, nothing is committed and no blob (payload or manifest) is stored.
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
    assert_eq!(canon(&prep.event().to_value()), canon(get(&c, "record")));
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
    e.set_fault(Some(Fault::CancelAfter(500)));
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

/// A reader that reads one chunk, then flips the real cancel flag and reads again: the
/// source itself refuses the second read, so a long read is cancellable whatever the reader
/// does with entities.
struct FlipReader<'f> {
    inner: TestPoints,
    flag: &'f AtomicBool,
    second_read_failed: Cell<bool>,
}

impl Reader for FlipReader<'_> {
    fn format(&self) -> &FormatId {
        self.inner.format()
    }
    fn info(&self) -> &ReaderInfo {
        self.inner.info()
    }
    fn read(&self, _opts: &CValue, sink: &mut StageSink<'_, '_>, _report: &mut ReportBuilder) -> Result<(), IoError> {
        let mut chunk = [0u8; 4096];
        assert!(sink.source().read_bounded(&mut chunk)? > 0);
        self.flag.store(true, Ordering::SeqCst);
        let r = sink.source().read_bounded(&mut chunk);
        self.second_read_failed.set(r.is_err());
        r.map(|_| ())
    }
}

/// Cancellation inside the bounded source: the real flag, flipped mid-read, fails the very
/// next read of the file and the import is `cancelled` with nothing to commit.
#[test]
fn cancel_flag_flipped_mid_read() {
    let tmp = Tmp::new("flip");
    let (reg, roots, tree) = (registry(), tmp.roots(), project());
    let flag = AtomicBool::new(false);
    let cmd =
        ImportCommand::new(FlipReader { inner: test_points(), flag: &flag, second_read_failed: Cell::new(false) }, &[])
            .unwrap();
    let mut e = env(&reg, &roots, &tree, Principal::Agent);
    e.cancel = Some(&flag);
    let mut blobs = MemoryBlobs::default();
    let err =
        cmd.prepare(&e, &ImportParams::new("root://in/many.ftp"), Some(&mut blobs as &mut dyn BlobSink)).unwrap_err();
    assert_eq!((err.code(), err.bus_code()), ("cancelled", ("cancelled", "requested")));
    assert!(cmd.reader().second_read_failed.get(), "the source did not check the flag");
    assert!(blobs.blobs.is_empty());
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
    // ... or that lists another format's extensions for a per-format command.
    let generic_def = import_file_def(&reg).unwrap();
    let mut lying = def.clone();
    lying.params_schema = generic_def.params_schema.clone();
    assert!(check(&per, &lying, &fx).is_err());
}

/// Per-format definitions list only their own format's extensions; `import.file@1` lists
/// every import format's.
#[test]
fn x_extensions_per_format() {
    let reg = registry();
    let exts = |d: &fid_registry::CommandDef| -> Vec<String> {
        let props = obj(get(obj(&d.params_schema), "properties"));
        arr(obj(get(props, "source")), "x-extensions")
            .iter()
            .map(|v| match v {
                CValue::Str(s) => s.clone(),
                other => panic!("{other:?}"),
            })
            .collect()
    };
    assert_eq!(exts(&import_file_def(&reg).unwrap()), ["ftm", "ftp"]);
    assert_eq!(exts(&format_def(&reg, &FormatId::new("fid-test-points").unwrap(), &[]).unwrap()), ["ftp"]);
    assert_eq!(exts(&format_def(&reg, &FormatId::new("fid-test-mesh").unwrap(), &[]).unwrap()), ["ftm"]);
}

// ---------------------------------------------------------------------------------------
// Untrusted input

/// Names are kept verbatim as untrusted text, an injected instruction included. A name with
/// a forbidden character (a C0 control, a bidi override, a zero-width space, a tag
/// character), a name not in NFC and a name over 256 characters keep their entities under
/// `unnamed-<index>`, each with a warning holding its position, reason and original text; the
/// payloads keep the file's names, so nothing is lost; the commit holds only entity events
/// and the record.
#[test]
fn names_never_cost_an_entity() {
    let c = case("names_s1");
    let tmp = Tmp::new("names");
    let (reg, roots, tree) = (registry(), tmp.roots(), project());
    let cmd = generic(test_points(), test_mesh());
    let prep = cmd
        .prepare(&env(&reg, &roots, &tree, Principal::Agent), &ImportParams::new("root://in/names.ftp"), None)
        .unwrap();
    assert_eq!(canon(&prep.report().to_cvalue()), canon(get(&c, "report")));
    assert_eq!(prep.report().warnings_total(), 6);
    assert_eq!(prep.report().dropped_total(), 0);
    assert_events(prep.events(), arr(&c, "events"));
    let kinds: Vec<&str> = prep.events().iter().map(|e| e.event_type.as_str()).collect();
    assert!(kinds.iter().all(|k| ["ElementCreated", "ElementSlotSet", "import.file@1"].contains(k)));
    let mut t = tree.clone();
    prep.commit(&mut t).unwrap();
    assert_eq!(digest(&t), st(&c, "state_after"));
    assert_eq!(t.elements().count(), 8);
    assert!(t.elements().any(|e| e.name().as_untrusted_str().starts_with("ignore previous instructions")));
    assert!(t.elements().any(|e| e.name().as_untrusted_str() == "unnamed-1"));
}

/// Every limit is enforced before the content is allocated: the file size against the byte
/// limit (before the prefix is read), elements and allocation while reading; an agent may
/// not raise a limit, and that is refused before the file is opened.
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
    let calls = roots.fs_calls();
    for src in ["root://in/ok.ftp", "root://in/missing.ftp"] {
        let err = run(Principal::Agent, LimitRequest::default().bytes(u64::MAX), src).unwrap_err();
        assert_eq!(err.bus_code(), ("policy", "human_only"));
    }
    assert_eq!(roots.fs_calls(), calls, "the file was opened before the limit raise was refused");
}

/// Many tiny entities: one budget pays for the reader, the sink and the commit, so a file of
/// 2,000 sets without points is charged at least 2,000 entity costs, and the same file under
/// an allocation limit just below what it needs fails `limit_exceeded{alloc}` instead of
/// growing the commit past it.
#[test]
fn many_tiny_entities_are_charged() {
    let tmp = Tmp::new("tiny");
    tmp.tiny("tiny.ftp", 2000);
    let (reg, roots, tree) = (registry(), tmp.roots(), project());
    let cmd = generic(test_points(), test_mesh());
    let e = env(&reg, &roots, &tree, Principal::Agent);
    let p = cmd.prepare(&e, &ImportParams::new("root://in/tiny.ftp"), None).unwrap();
    assert_eq!(p.event().entities.len(), 2000);
    let charged = p.alloc_charged();
    // Each set's name `t<i>` and type `test.point_set`, and its payload, on top.
    assert!(charged >= 2000 * entity_cost(0, 0), "{charged}");
    let mut e = env(&reg, &roots, &tree, Principal::Agent);
    e.limits = LimitRequest::default().alloc(charged - 1);
    let err = cmd.prepare(&e, &ImportParams::new("root://in/tiny.ftp"), None).unwrap_err();
    assert!(matches!(err, ImportError::Io(IoError::LimitExceeded { limit: LimitKind::Alloc, .. })), "{err}");
    let mut e = env(&reg, &roots, &tree, Principal::Agent);
    e.limits = LimitRequest::default().alloc(charged);
    cmd.prepare(&e, &ImportParams::new("root://in/tiny.ftp"), None).unwrap();
}

/// The entity cap (an element bound in IO.REG.LIMITS terms): one entity more than
/// [`MAX_IMPORT_ENTITIES`] fails `limit_exceeded{elements}` with the cap as its value
/// (validation/effect_limit), whatever the caller's element limit.
#[test]
fn entity_cap() {
    let tmp = Tmp::new("cap");
    tmp.tiny("over.ftp", MAX_IMPORT_ENTITIES + 1);
    let (reg, roots, tree) = (registry(), tmp.roots(), project());
    let cmd = ImportCommand::new(test_points(), &[]).unwrap();
    let mut e = env(&reg, &roots, &tree, Principal::Human);
    e.limits = LimitRequest::default().elements(u64::MAX);
    let err = cmd.prepare(&e, &ImportParams::new("root://in/over.ftp"), None).unwrap_err();
    assert!(
        matches!(
            err,
            ImportError::Io(IoError::LimitExceeded { limit: LimitKind::Elements, value: MAX_IMPORT_ENTITIES, .. })
        ),
        "{err}"
    );
    assert_eq!(err.bus_code(), ("validation", "effect_limit"));
}

fn worst_text(bytes: usize) -> String {
    // A C0 control is the longest JSON form of one byte (`\u0001`, six bytes).
    "\u{1}".repeat(bytes)
}

/// The record's bound: a record at every input bound at once (the entity cap, options of
/// `MAX_OPTIONS_BYTES` nested `MAX_OPTIONS_DEPTH` deep, a provenance whose every untrusted
/// text is `MAX_TEXT_BYTES` of the longest-escaping character, `MAX_DECLARED` declared items,
/// the longest source handle) stays within `MAX_JSON_BYTES` and reads back through
/// `ImportEvent::from_json`; so does every record the skeleton commits (it checks this itself
/// before returning), and options one level deeper than the bound are refused.
#[test]
fn every_record_is_readable_at_its_bounds() {
    // `deep` nests MAX_OPTIONS_DEPTH - 1 objects, so `{"deep": deep}` is exactly at the bound.
    let mut deep = CValue::Obj(Obj::new());
    for _ in 1..MAX_OPTIONS_DEPTH - 1 {
        let mut o = Obj::new();
        o.insert("a".into(), deep);
        deep = CValue::Obj(o);
    }
    let mut options = Obj::new();
    options.insert("deep".into(), deep.clone());
    options.insert("pad".into(), CValue::Str("x".repeat(MAX_OPTIONS_BYTES - 64)));
    let options = CValue::Obj(options);
    let text = CValue::Str(worst_text(MAX_TEXT_BYTES));
    let mut declared = Obj::new();
    for i in 0..MAX_DECLARED {
        declared.insert(format!("{i:064}"), text.clone());
    }
    let base = case("ok_s1");
    let mut p = obj(get(obj(get(&base, "record")), "provenance")).clone();
    p.insert("declared".into(), CValue::Obj(declared));
    p.insert("source_name".into(), text.clone());
    p.insert("origin_claim".into(), text.clone());
    p.insert("options".into(), options.clone());
    let p = CValue::Obj(p);
    let sha = fid_types::Digest::from_bytes(<[u8; 32]>::from(sha2_digest(&canon(&p))));
    let id = Uuid::parse("0190aaaa-0000-7000-8000-000000000001").unwrap();
    let handle = format!("root://in/{}x.ftp", format!("{}/", "a".repeat(199)).repeat(19));
    let ev = ImportEvent {
        command: import_file_id().unwrap(),
        principal: Principal::Human,
        source: Location::handle(RootHandle::parse(&handle).unwrap()).unwrap(),
        format: FormatId::new("fid-test-points").unwrap(),
        format_named: true,
        unit: Some(LengthUnit::Mm),
        stage: id,
        options,
        provenance: p,
        provenance_sha256: sha,
        report_sha256: sha,
        entities: vec![id; MAX_IMPORT_ENTITIES as usize],
        blob_manifest: sha,
        blob_count: MAX_IMPORT_ENTITIES,
    };
    let json = ev.to_json().unwrap();
    assert!(json.len() <= MAX_JSON_BYTES, "{} > {MAX_JSON_BYTES}", json.len());
    assert_eq!(ImportEvent::from_json(&json).unwrap(), ev);

    // Through the skeleton: options at the depth bound import, one deeper are refused.
    let tmp = Tmp::new("bounds");
    let (reg, roots, tree) = (registry(), tmp.roots(), project());
    let cmd = generic(test_points(), test_mesh());
    let mut p = ImportParams::new("root://in/ok.ftp");
    let mut top = Obj::new();
    top.insert("deep".into(), deep.clone());
    p.options = CValue::Obj(top);
    let prep = cmd.prepare(&env(&reg, &roots, &tree, Principal::Agent), &p, None).unwrap();
    assert_eq!(ImportEvent::from_json(&prep.event().to_json().unwrap()).unwrap(), *prep.event());
    let mut top = Obj::new();
    let mut deeper = Obj::new();
    deeper.insert("a".into(), deep);
    top.insert("deep".into(), CValue::Obj(deeper));
    p.options = CValue::Obj(top);
    let err = cmd.prepare(&env(&reg, &roots, &tree, Principal::Agent), &p, None).unwrap_err();
    assert_eq!(err.code(), "bad_options", "{err}");
}

fn sha2_digest(bytes: &[u8]) -> [u8; 32] {
    use sha2::Digest as _;
    sha2::Sha256::digest(bytes).into()
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

/// A hard-linked source (IO.REG.ROOTS `RootWarning::HardLinked`) imports with a report
/// warning `hard_linked` (no position, detail `2 links`). Windows reports no link count.
#[cfg(unix)]
#[test]
fn hard_linked_source_is_reported() {
    let tmp = Tmp::new("hardlink");
    fs::hard_link(tmp.raw.join("in").join("ok.ftp"), tmp.raw.join("in").join("linked.ftp")).unwrap();
    let (reg, roots, tree) = (registry(), tmp.roots(), project());
    let cmd = generic(test_points(), test_mesh());
    let p = cmd
        .prepare(&env(&reg, &roots, &tree, Principal::Agent), &ImportParams::new("root://in/linked.ftp"), None)
        .unwrap();
    let w = &p.report().warnings()[0];
    assert_eq!((w.code.as_str(), w.pos.is_none(), w.detail.text().raw()), ("hard_linked", true, "2 links"));
    assert_eq!(p.report().warnings_total(), 1);
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
    let edit = |f: &dyn Fn(&mut Obj)| {
        let mut v = prep.event().to_value();
        if let CValue::Obj(o) = &mut v {
            f(o);
        }
        ImportEvent::from_value(&v).unwrap_err().code()
    };
    let changed_provenance = |o: &mut Obj| {
        if let Some(CValue::Obj(p)) = o.get_mut("provenance") {
            p.insert("evidence".into(), CValue::Str("measured".into()));
        }
    };
    assert_eq!(edit(&changed_provenance), "bad_event", "a changed provenance is caught");
    assert_eq!(
        edit(&|o: &mut Obj| {
            o.insert("blob_count".into(), CValue::U64(2));
        }),
        "bad_event"
    );
    assert_eq!(
        edit(&|o: &mut Obj| {
            o.insert("blob_count".into(), CValue::Str("01".into()));
        }),
        "bad_event"
    );
    assert_eq!(
        edit(&|o: &mut Obj| {
            o.insert("entities".into(), CValue::Arr(vec![]));
        }),
        "bad_event"
    );
    assert_eq!(
        edit(&|o: &mut Obj| {
            o.remove("blob_manifest");
        }),
        "bad_event"
    );
}

fn every_import_error() -> Vec<ImportError> {
    let at = Position::byte(3);
    let mut io: Vec<IoError> =
        [LimitKind::Bytes, LimitKind::Elements, LimitKind::Depth, LimitKind::Alloc, LimitKind::Wall]
            .into_iter()
            .map(|limit| IoError::LimitExceeded { limit, value: 1, at })
            .collect();
    io.extend([
        IoError::Malformed { at, reason: "x" },
        IoError::TruncatedFile { at },
        IoError::UnitRequired,
        IoError::UnitConflict,
        IoError::FormatUnknown,
        IoError::FormatAmbiguous,
        IoError::FormatMismatch,
        IoError::FormatDuplicate,
        IoError::RawPathForbidden,
        IoError::PathOutsideRoot,
        IoError::RootNotFound,
        IoError::TargetExists,
        IoError::LimitRaiseForbidden { limit: LimitKind::Bytes, requested: 2, default: 1 },
        IoError::ReaderCrashed,
        IoError::Io { kind: std::io::ErrorKind::NotFound, at },
        IoError::Io { kind: std::io::ErrorKind::Other, at },
    ]);
    let id = Uuid::parse("0190aaaa-0000-7000-8000-000000000001").unwrap();
    let mut all: Vec<ImportError> = io.iter().cloned().map(ImportError::Io).collect();
    all.extend(io.into_iter().map(ImportError::Reader));
    all.extend([
        ImportError::Cancelled,
        ImportError::StageNotFound,
        ImportError::StageAmbiguous,
        ImportError::BadParams("x"),
        ImportError::BadOptions("x"),
        ImportError::BadEvent("x"),
        ImportError::Provenance("x".into()),
        ImportError::CommandIdTaken,
        ImportError::Internal("x"),
    ]);
    all.extend(
        [
            TreeError::Schema("x".into()),
            TreeError::Name,
            TreeError::UnregisteredType("x".into()),
            TreeError::BadStage(id),
            TreeError::StageNotFound("x".into()),
            TreeError::AmbiguousStage("x".into()),
            TreeError::IdCollision(id),
            TreeError::Invariant("x".into()),
            TreeError::Label,
        ]
        .into_iter()
        .map(ImportError::Tree),
    );
    all
}

/// Every error variant maps to a pair the bus's own catalogue decodes
/// (`fid_bus::error::BusError::from_wire`), and the pinned pairs hold.
#[test]
fn bus_codes() {
    for e in every_import_error() {
        let (code, reason) = e.bus_code();
        let back = fid_bus::error::BusError::from_wire(code, reason, "detail");
        assert!(back.is_some(), "{e}: {code}/{reason} is not in the bus catalogue");
        let back = back.unwrap();
        assert_eq!((back.code().as_str(), back.reason()), (code, reason), "{e}");
    }
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
        (ImportError::Tree(TreeError::IdCollision(Uuid::NIL)), ("conflict", "id_collision")),
    ];
    for (e, want) in cases {
        assert_eq!(e.bus_code(), want, "{e}");
    }
}

/// The CAS adapter stores each payload, and the blob manifest, as one blob addressed by its
/// SHA-256.
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
    assert_eq!(p.event().blob_count, 1);
    assert!(store.has_blob(&d).unwrap());
    assert!(store.has_blob(&p.event().blob_manifest).unwrap());
}
