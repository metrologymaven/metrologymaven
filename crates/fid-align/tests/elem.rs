//! Acceptance criteria of spec node ALN.MODEL.ELEM (AC-1 .. AC-6), plus fail-closed checks,
//! against fixture `fixtures/aln/model/chain3`. Every expected ID, digest and output byte
//! comes from `fixtures/aln/model/make_fixtures.py`, a separate reference implementation of
//! the rules pinned in the leaf's Design; nothing expected is produced by this crate.
//!
//! The test kit is a stub solver `aln.stub.fixed` (version 1.0.0 rigid, 1.1.0 similarity
//! with scale 1.5) that returns a fixed rotation and the translation given by parameters
//! `tx`, `ty`, `tz`, and a test "Record Journal" that holds the canonical bytes of every
//! committed record.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_align::{
    command_defs, invoke_cli, invoke_mcp, ActivityEntry, AlgorithmId, AlignError, Alignment, AlignmentModel,
    AlignmentSolver, AlignmentStatus, CreateRequest, InputRef, ParamSet, PrincipalKind, ProvenanceState, ReportRef,
    SolveStatus, SolverError, SolverOutput, SolverRegistry, Surface, GET_ALIGNMENT, LIST_ALIGNMENTS, MAX_PARAMS_BYTES,
};
use fid_canon::{parse, to_canonical_bytes, CValue};
use fid_dep::{EdgeIndex, EdgesSet};
use fid_geo_frame::{Rigid, Similarity, Transform};
use fid_project::{Bits, IdService, Observation, ReplaySource};
use fid_types::Digest;
use fid_uuid::Uuid;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

// ------------------------------------------------------------------ test kit

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/aln/model/chain3")
}

fn read(name: &str) -> String {
    std::fs::read_to_string(fixtures().join(name)).unwrap()
}

fn lines(name: &str) -> Vec<Vec<String>> {
    read(name)
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(|l| l.split(' ').map(str::to_string).collect())
        .collect()
}

fn uuid(s: &str) -> Uuid {
    Uuid::parse(s).unwrap()
}

fn alg(s: &str) -> AlgorithmId {
    AlgorithmId::parse(s).unwrap()
}

const ROT: [[f64; 3]; 3] = [[0.0, -1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]];

/// The fixture's stub solver. `fail` makes it refuse with that code.
struct Stub {
    id: AlgorithmId,
    report_id: Uuid,
    fail: Option<&'static str>,
}

impl AlignmentSolver for Stub {
    fn algorithm_id(&self) -> AlgorithmId {
        self.id.clone()
    }

    fn solve(
        &self,
        inputs: &[InputRef],
        params: &ParamSet,
        _parent_pose: Option<&Transform>,
    ) -> Result<SolverOutput, SolverError> {
        if let Some(code) = self.fail {
            return Err(SolverError::new(code).unwrap());
        }
        let p = |n: &str| params.get(n).map(|q| q.si()).ok_or_else(|| SolverError::new("missing_param").unwrap());
        let rigid = Rigid::new(ROT, [p("tx")?, p("ty")?, p("tz")?]).unwrap();
        let transform = if self.id.version.1 == 1 {
            Transform::Similarity(Similarity::new(rigid, 1.5).unwrap())
        } else {
            Transform::Rigid(rigid)
        };
        let joined: Vec<&str> = inputs.iter().map(|i| i.digest.as_str()).collect();
        let d = fid_canon::digest_bytes(joined.join("\n").as_bytes());
        let report = ReportRef { id: self.report_id, digest: Digest::from_bytes(d.0) };
        Ok(SolverOutput { transform, report, status: SolveStatus::Ok })
    }
}

struct Spec {
    name: String,
    algorithm: AlgorithmId,
    parent: Option<String>,
    inputs: Vec<String>,
    principal: String,
    kind: PrincipalKind,
    ms: u64,
    random: u128,
    params: Vec<(String, String, String)>,
}

/// chain3 as built: model, edge index, element digests, names and the test Record Journal.
struct Project {
    model: AlignmentModel,
    edges: EdgeIndex,
    world: BTreeMap<Uuid, Digest>,
    names: BTreeMap<String, Uuid>,
    shifted_p1: Digest,
    report_id: Uuid,
    specs: Vec<Spec>,
    journal: Vec<Vec<u8>>,
}

impl Project {
    fn id(&self, name: &str) -> Uuid {
        self.names[name]
    }

    fn get(&self, name: &str) -> &Alignment {
        self.model.get(&self.id(name)).unwrap()
    }

    fn request(&self, spec: &Spec) -> CreateRequest {
        let mut params = ParamSet::new();
        for (n, text, unit) in &spec.params {
            params.insert(n, text, unit).unwrap();
        }
        CreateRequest {
            algorithm: spec.algorithm.clone(),
            inputs: spec.inputs.iter().map(|n| self.id(n)).collect(),
            params,
            parent: spec.parent.as_ref().map(|n| self.id(n)),
            principal: spec.principal.clone(),
            kind: spec.kind,
        }
    }
}

fn replay(ms: u64, random: u128) -> ReplaySource {
    ReplaySource::new(vec![Observation::NowMs(ms), Observation::RandomBits(Bits::new(74, random).unwrap())])
}

fn registry(report_id: Uuid, versions: &[&str]) -> SolverRegistry {
    let mut r = SolverRegistry::new();
    for v in versions {
        r.register(Box::new(Stub { id: alg(v), report_id, fail: None })).unwrap();
    }
    r
}

const V100: &str = "aln.stub.fixed@1.0.0";
const V110: &str = "aln.stub.fixed@1.1.0";

fn load() -> Project {
    let mut p = Project {
        model: AlignmentModel::new(),
        edges: EdgeIndex::new(),
        world: BTreeMap::new(),
        names: BTreeMap::new(),
        shifted_p1: Digest::TOMBSTONE,
        report_id: Uuid::NIL,
        specs: Vec::new(),
        journal: Vec::new(),
    };
    let mut edge_lines = Vec::new();
    for l in lines("project.txt") {
        match l[0].as_str() {
            "element" => {
                let id = uuid(&l[2]);
                p.names.insert(l[1].clone(), id);
                p.world.insert(id, Digest::parse(&l[3]).unwrap());
            }
            "shifted" => p.shifted_p1 = Digest::parse(&l[2]).unwrap(),
            "edge" => edge_lines.push((l[1].clone(), l[2].clone())),
            "report_id" => p.report_id = uuid(&l[1]),
            "alignment" => p.specs.push(Spec {
                name: l[1].clone(),
                algorithm: alg(&l[2]),
                parent: (l[3] != "-").then(|| l[3].clone()),
                inputs: l[4].split(',').map(str::to_string).collect(),
                principal: l[5].clone(),
                kind: PrincipalKind::parse(&l[6]).unwrap(),
                ms: l[7].parse().unwrap(),
                random: u128::from_str_radix(&l[8], 16).unwrap(),
                params: l[9..]
                    .iter()
                    .map(|t| {
                        let (n, rest) = t.split_once('=').unwrap();
                        let (text, unit) = rest.rsplit_once(':').unwrap();
                        (n.to_string(), text.to_string(), unit.to_string())
                    })
                    .collect(),
            }),
            other => panic!("unknown fixture line {other}"),
        }
    }
    // The mesh, then each plane fitted to it.
    let mut sets = vec![EdgesSet::new(p.id("M"), vec![]).unwrap()];
    for (from, to) in &edge_lines {
        sets.push(EdgesSet::new(p.id(to), vec![p.id(from)]).unwrap());
    }
    for s in &sets {
        p.edges.apply(std::slice::from_ref(s)).unwrap();
    }
    p
}

/// chain3 with every alignment created through the wrapper.
fn chain3(solvers: &SolverRegistry) -> Project {
    let mut p = load();
    let specs = std::mem::take(&mut p.specs);
    for spec in &specs {
        let req = p.request(spec);
        let mut src = replay(spec.ms, spec.random);
        let existing: BTreeSet<Uuid> = BTreeSet::new();
        let mut ids = IdService::new(Uuid::from_u128(1), &mut src, &existing);
        let created = p.model.create(&req, solvers, &p.world, &mut p.edges, &mut ids).unwrap();
        p.journal.push(to_canonical_bytes(&created.alignment.to_value()).unwrap());
        p.journal.push(to_canonical_bytes(&created.edges.to_value()).unwrap());
        p.names.insert(spec.name.clone(), created.alignment.id());
    }
    p.specs = specs;
    p
}

fn full() -> SolverRegistry {
    registry(load().report_id, &[V100, V110])
}

fn expected() -> BTreeMap<(String, String), String> {
    lines("expected.txt").into_iter().map(|l| ((l[0].clone(), l[1].clone()), l[2].clone())).collect()
}

fn exp(kind: &str, name: &str) -> String {
    expected()[&(kind.to_string(), name.to_string())].clone()
}

fn obj_mut(v: &mut CValue) -> &mut BTreeMap<String, CValue> {
    match v {
        CValue::Obj(o) => o,
        _ => panic!("not an object"),
    }
}

fn a2_value() -> CValue {
    parse(read("get-a2.json").as_bytes()).unwrap()
}

// ------------------------------------------------------------------ AC-1

#[test]
fn ac1_get_alignment_returns_every_field_and_the_recorded_digest() {
    let p = chain3(&full());
    for n in ["A0", "A1", "A2"] {
        assert_eq!(p.id(n).to_string(), exp("id", n), "UUIDv7 of {n}");
    }
    let mut log: Vec<ActivityEntry> = Vec::new();
    let a2 = p.id("A2").to_string();
    let out = invoke_cli(&p.model, &mut log, &["get-alignment", &a2, "--json"]).unwrap();
    assert_eq!(String::from_utf8(out.clone()).unwrap(), read("get-a2.json"));
    // Every REQ-001 field is present.
    let v = parse(&out).unwrap();
    let CValue::Obj(o) = &v else { panic!() };
    let keys: Vec<&str> = o.keys().map(String::as_str).collect();
    assert_eq!(
        keys,
        [
            "algorithm",
            "digest",
            "failure",
            "id",
            "inputs",
            "params",
            "parent",
            "provenance",
            "report",
            "status",
            "transform"
        ]
    );
    assert_eq!(o["digest"], CValue::Str(exp("digest", "A2")));
    assert_eq!(p.get("A2").digest().as_str(), exp("digest", "A2"));
}

#[test]
fn ac1_every_alignment_round_trips_through_its_canonical_form() {
    let p = chain3(&full());
    for n in ["A0", "A1", "A2"] {
        let a = p.get(n);
        let bytes = a.to_json().unwrap();
        let back = Alignment::from_json(&bytes).unwrap();
        assert_eq!(&back, a, "{n}");
        assert_eq!(back.to_json().unwrap(), bytes, "{n}");
    }
    // Provenance: a human gets state human, an agent gets proposed.
    assert_eq!(p.get("A0").provenance().state(), ProvenanceState::Human);
    assert_eq!(p.get("A1").provenance().state(), ProvenanceState::Proposed);
    assert_eq!(p.get("A1").provenance().principal(), "agent-1");
    assert_eq!(p.get("A1").transform().scale().to_bits(), 1.5f64.to_bits());
    assert!(!p.get("A1").transform().is_rigid());
}

// ------------------------------------------------------------------ AC-2

#[test]
fn ac2_replacing_a1s_input_recomputes_a1_and_a2_in_one_entry_keeping_ids() {
    let solvers = full();
    let mut p = chain3(&solvers);
    let a0_before = p.get("A0").clone();
    let p1 = p.id("P1");
    p.world.insert(p1, p.shifted_p1);
    let entry = p.model.recompute(&[p1], &solvers, &p.world, &p.edges).unwrap();
    assert_eq!(entry.changed, vec![p1]);
    let ids: Vec<Uuid> = entry.items.iter().map(|i| i.id).collect();
    assert_eq!(ids, vec![p.id("A1"), p.id("A2")]);
    for (item, n) in entry.items.iter().zip(["A1", "A2"]) {
        assert_eq!(item.status, AlignmentStatus::Ok);
        assert_eq!(item.digest_before.as_str(), exp("digest", n));
        assert_eq!(item.digest_after.as_str(), exp("digest_after_shift", n));
        assert_ne!(item.digest_before, item.digest_after);
        let a = p.get(n);
        assert_eq!(a.id(), item.id, "{n} keeps its ID");
        assert_eq!(a.digest().as_str(), exp("digest_after_shift", n));
        assert_eq!(Alignment::from_json(&a.to_json().unwrap()).as_ref(), Ok(a));
    }
    assert_eq!(p.get("A1").inputs()[0].digest, p.shifted_p1);
    assert_eq!(p.get("A2").parent().map(|r| r.digest), Some(p.get("A1").digest()));
    assert_eq!(p.get("A0"), &a0_before, "A0 is upstream and untouched");
    // One entry, one canonical value.
    let v = entry.to_value();
    let CValue::Obj(o) = &v else { panic!() };
    assert!(matches!(&o["items"], CValue::Arr(a) if a.len() == 2));
}

// ------------------------------------------------------------------ AC-3

#[test]
fn ac3_a_solver_error_on_creation_creates_nothing_and_journals_nothing() {
    let mut p = chain3(&full());
    let mut solvers = full();
    solvers
        .register(Box::new(Stub {
            id: alg("aln.stub.broken@1.0.0"),
            report_id: p.report_id,
            fail: Some("degenerate_input"),
        }))
        .unwrap();
    let mut req = p.request(&p.specs[0]);
    req.algorithm = alg("aln.stub.broken@1.0.0");
    let (models, journal, edges) = (p.model.clone(), p.journal.len(), p.edges.digest().unwrap());
    let mut src = replay(1_763_510_400_100, 7);
    let existing: BTreeSet<Uuid> = BTreeSet::new();
    let mut ids = IdService::new(Uuid::from_u128(1), &mut src, &existing);
    let err = p.model.create(&req, &solvers, &p.world, &mut p.edges, &mut ids).unwrap_err();
    assert_eq!(err.code(), "degenerate_input", "solver code passed through unchanged");
    assert!(matches!(err, AlignError::Solver(_)));
    assert_eq!(p.model, models, "no element exists afterwards");
    assert_eq!(p.journal.len(), journal, "Record Journal length unchanged");
    assert_eq!(p.edges.digest().unwrap(), edges, "no edge registered");
    assert_eq!(src.remaining(), 2, "no ID drawn");
}

// ------------------------------------------------------------------ AC-4

#[test]
fn ac4_a_removed_algorithm_version_fails_with_method_unavailable_and_outdates_dependents() {
    let mut solvers = full();
    let mut p = chain3(&solvers);
    assert!(solvers.remove(&alg(V110)));
    assert!(solvers.get(&alg(V100)).is_some(), "another version stays registered and is not substituted");
    let p1 = p.id("P1");
    p.world.insert(p1, p.shifted_p1);
    let entry = p.model.recompute(&[p1], &solvers, &p.world, &p.edges).unwrap();
    let a1 = p.get("A1");
    assert_eq!(a1.status(), AlignmentStatus::Failed);
    assert_eq!(a1.failure(), Some("method_unavailable"));
    assert_eq!(a1.digest().as_str(), exp("digest", "A1"), "content kept");
    let a2 = p.get("A2");
    assert_eq!(a2.status(), AlignmentStatus::Outdated);
    assert_eq!(a2.failure(), None);
    assert_eq!(a2.digest().as_str(), exp("digest", "A2"));
    let got: Vec<(Uuid, AlignmentStatus, Option<&str>)> =
        entry.items.iter().map(|i| (i.id, i.status, i.failure.as_deref())).collect();
    assert_eq!(
        got,
        vec![
            (p.id("A1"), AlignmentStatus::Failed, Some("method_unavailable")),
            (p.id("A2"), AlignmentStatus::Outdated, None)
        ]
    );
    // A failed alignment still round-trips; its digest still matches its kept content.
    assert_eq!(Alignment::from_json(&a1.to_json().unwrap()).as_ref(), Ok(a1));
}

#[test]
fn a_solver_error_during_recompute_sets_failed_with_the_solver_code() {
    let mut p = chain3(&full());
    let mut solvers = registry(p.report_id, &[V100]);
    solvers
        .register(Box::new(Stub { id: alg(V110), report_id: p.report_id, fail: Some("not_enough_points") }))
        .unwrap();
    let a1 = p.id("A1");
    let entry = p.model.recompute(&[a1], &solvers, &p.world, &p.edges).unwrap();
    assert_eq!(entry.items[0].failure.as_deref(), Some("not_enough_points"));
    assert_eq!(p.get("A2").status(), AlignmentStatus::Outdated);
    // A child of a failed parent cannot be created.
    let mut req = p.request(&p.specs[2]);
    req.parent = Some(a1);
    let mut src = replay(1_763_510_400_100, 7);
    let existing: BTreeSet<Uuid> = BTreeSet::new();
    let mut ids = IdService::new(Uuid::from_u128(1), &mut src, &existing);
    let err = p.model.create(&req, &solvers, &p.world, &mut p.edges, &mut ids).unwrap_err();
    assert_eq!(err, AlignError::ParentUnusable(a1));
}

// ------------------------------------------------------------------ AC-5

#[test]
fn ac5_cli_and_mcp_list_payloads_are_byte_identical() {
    let p = chain3(&full());
    let mut log = Vec::new();
    let cli = invoke_cli(&p.model, &mut log, &["list-alignments", "--json"]).unwrap();
    let mcp = invoke_mcp(&p.model, &mut log, "align_list_alignments", b"{}").unwrap();
    assert_eq!(cli, mcp);
    assert_eq!(String::from_utf8(cli).unwrap(), read("list.json"));
    // With filters too, and for get.
    let a0 = p.id("A0").to_string();
    let cli = invoke_cli(&p.model, &mut log, &["list-alignments", "--parent", &a0, "--status", "ok"]).unwrap();
    let params = format!("{{\"status\":\"ok\",\"parent\":\"{a0}\"}}");
    let mcp = invoke_mcp(&p.model, &mut log, "align_list_alignments", params.as_bytes()).unwrap();
    assert_eq!(cli, mcp);
    let CValue::Arr(rows) = parse(&cli).unwrap() else { panic!() };
    assert_eq!(rows.len(), 1, "only A1 has parent A0");
    let empty = invoke_mcp(&p.model, &mut log, "align_list_alignments", b"{\"status\":\"failed\"}").unwrap();
    assert_eq!(empty, b"[]");
    let a2 = p.id("A2").to_string();
    let cli = invoke_cli(&p.model, &mut log, &["get-alignment", &a2]).unwrap();
    let mcp = invoke_mcp(&p.model, &mut log, "align_get_alignment", format!("{{\"id\":\"{a2}\"}}").as_bytes()).unwrap();
    assert_eq!(cli, mcp);
    // R0: every call is in the Activity Log, none in the Record Journal.
    let surfaces: Vec<(&str, Surface)> = log.iter().map(|e| (e.command, e.surface)).collect();
    assert_eq!(
        surfaces,
        vec![
            (LIST_ALIGNMENTS, Surface::Cli),
            (LIST_ALIGNMENTS, Surface::Mcp),
            (LIST_ALIGNMENTS, Surface::Cli),
            (LIST_ALIGNMENTS, Surface::Mcp),
            (LIST_ALIGNMENTS, Surface::Mcp),
            (GET_ALIGNMENT, Surface::Cli),
            (GET_ALIGNMENT, Surface::Mcp),
        ]
    );
    assert_eq!(p.journal.len(), 6);
}

// ------------------------------------------------------------------ AC-6

#[test]
fn ac6_digests_equal_the_platform_independent_reference() {
    // The reference values come from Python; CI runs this on Linux, Windows and macOS, so
    // equality here on each platform is equality across platforms.
    let p = chain3(&full());
    for n in ["A0", "A1", "A2"] {
        assert_eq!(p.get(n).digest().as_str(), exp("digest", n), "{n}");
    }
    // Building twice gives the same bytes.
    let q = chain3(&full());
    for n in ["A0", "A1", "A2"] {
        assert_eq!(p.get(n).to_json().unwrap(), q.get(n).to_json().unwrap());
    }
}

// ------------------------------------------------------------------ creation refusals

fn try_create(p: &mut Project, solvers: &SolverRegistry, req: &CreateRequest) -> (AlignError, usize) {
    let mut src = replay(1_763_510_400_100, 7);
    let existing: BTreeSet<Uuid> = BTreeSet::new();
    let mut ids = IdService::new(Uuid::from_u128(1), &mut src, &existing);
    let before = p.model.clone();
    let err = p.model.create(req, solvers, &p.world, &mut p.edges, &mut ids).unwrap_err();
    assert_eq!(p.model, before);
    (err, src.remaining())
}

#[test]
fn creation_refuses_missing_references_unknown_methods_and_repeats_before_drawing_an_id() {
    let solvers = full();
    let mut p = chain3(&solvers);
    let base = p.request(&p.specs[2]);
    let ghost = Uuid::from_u128(0x0123);

    let mut r = base.clone();
    r.inputs = vec![ghost];
    assert_eq!(try_create(&mut p, &solvers, &r), (AlignError::InputMissing(ghost), 2));

    let mut r = base.clone();
    r.inputs = vec![p.id("P2"), p.id("P2")];
    assert_eq!(try_create(&mut p, &solvers, &r), (AlignError::DuplicateInput(p.id("P2")), 2));

    let mut r = base.clone();
    r.algorithm = alg("aln.stub.fixed@2.0.0");
    let (e, left) = try_create(&mut p, &solvers, &r);
    assert_eq!((e.code(), left), ("method_unavailable", 2));

    let mut r = base.clone();
    r.parent = Some(ghost);
    assert_eq!(try_create(&mut p, &solvers, &r), (AlignError::NotFound(ghost), 2));

    // A plane is not an alignment, so it cannot be a parent.
    let mut r = base.clone();
    r.parent = Some(p.id("P0"));
    assert_eq!(try_create(&mut p, &solvers, &r), (AlignError::NotFound(p.id("P0")), 2));

    let mut r = base.clone();
    r.principal = "QA-1".into();
    assert_eq!(try_create(&mut p, &solvers, &r).0.code(), "bad_params");

    // Missing parameter: the solver's own typed error passes through.
    let mut r = base.clone();
    r.params = ParamSet::new();
    assert_eq!(try_create(&mut p, &solvers, &r).0.code(), "missing_param");

    // An ID already taken is refused.
    let a0 = p.get("A0").clone();
    let (ms, random) = (p.specs[0].ms, p.specs[0].random);
    let mut src = replay(ms, random);
    let existing: BTreeSet<Uuid> = BTreeSet::new();
    let mut ids = IdService::new(Uuid::from_u128(1), &mut src, &existing);
    let err = p.model.create(&base, &solvers, &p.world, &mut p.edges, &mut ids).unwrap_err();
    assert_eq!(err, AlignError::IdCollision(a0.id()));
}

#[test]
fn parameters_refuse_nan_unknown_units_bad_names_and_repeats() {
    let mut s = ParamSet::new();
    assert_eq!(s.insert("tx", "NaN", "mm").unwrap_err().code(), "non_finite");
    assert_eq!(s.insert("tx", "inf", "mm").unwrap_err().code(), "non_finite");
    assert_eq!(s.insert("tx", "1", "furlong").unwrap_err().code(), "unknown_kind");
    assert_eq!(s.insert("tx", "1,5", "mm").unwrap_err().code(), "bad_quantity");
    assert_eq!(s.insert("Tx", "1", "mm").unwrap_err().code(), "bad_params");
    s.insert("tx", "1", "mm").unwrap();
    assert_eq!(s.insert("tx", "2", "mm").unwrap_err().code(), "bad_params");
    assert_eq!(s.get("tx").map(|q| q.si().to_bits()), Some(0.001f64.to_bits()));
}

#[test]
fn the_registry_refuses_a_second_solver_for_one_version() {
    let mut r = full();
    let err = r.register(Box::new(Stub { id: alg(V100), report_id: Uuid::from_u128(9), fail: None })).unwrap_err();
    assert_eq!(err.code(), "duplicate_algorithm");
    let ids: Vec<String> = r.ids().map(|i| i.to_string()).collect();
    assert_eq!(ids, vec![V100.to_string(), V110.to_string()]);
}

#[test]
fn recompute_refuses_an_unknown_changed_element() {
    let solvers = full();
    let mut p = chain3(&solvers);
    let before = p.model.clone();
    let ghost = Uuid::from_u128(0x0123);
    assert_eq!(p.model.recompute(&[ghost], &solvers, &p.world, &p.edges), Err(AlignError::NotFound(ghost)));
    assert_eq!(p.model, before);
}

// ------------------------------------------------------------------ decoding refusals

fn decode_err(edit: impl FnOnce(&mut BTreeMap<String, CValue>)) -> String {
    let mut v = a2_value();
    edit(obj_mut(&mut v));
    Alignment::from_value(&v).unwrap_err().code().to_string()
}

fn sub<'a>(o: &'a mut BTreeMap<String, CValue>, key: &str) -> &'a mut BTreeMap<String, CValue> {
    obj_mut(o.get_mut(key).unwrap())
}

fn s(x: &str) -> CValue {
    CValue::Str(x.to_string())
}

#[test]
fn decoding_fails_closed() {
    assert!(Alignment::from_value(&a2_value()).is_ok());
    assert_eq!(decode_err(|o| drop(o.insert("extra".into(), CValue::Null))), "bad_element");
    assert_eq!(decode_err(|o| drop(o.remove("report"))), "bad_element");
    assert_eq!(decode_err(|o| drop(o.insert("status".into(), s("done")))), "unknown_kind");
    assert_eq!(decode_err(|o| drop(o.insert("id".into(), s("019A9969-3406-7747-9B3E-292EA977021E")))), "bad_element");
    assert_eq!(decode_err(|o| drop(o.insert("algorithm".into(), s("aln.stub@1.0.0")))), "bad_element");
    assert_eq!(decode_err(|o| drop(o.insert("failure".into(), s("x")))), "bad_element");
    assert_eq!(
        decode_err(|o| {
            o.insert("status".into(), s("failed"));
        }),
        "bad_element",
        "failed needs a failure code"
    );
    assert_eq!(
        decode_err(|o| drop(o.insert("digest".into(), s(&format!("sha256:{}", "0".repeat(64)))))),
        "digest_mismatch"
    );
    // Transform: unknown kind, NaN, degenerate rotation, bad scale.
    assert_eq!(decode_err(|o| drop(sub(o, "transform").insert("kind".into(), s("affine")))), "unknown_kind");
    assert_eq!(
        decode_err(|o| drop(sub(o, "transform").insert(
            "t".into(),
            CValue::Arr(vec![s("0x7ff8000000000000"), s("0x0000000000000000"), s("0x0000000000000000")])
        ))),
        "non_finite"
    );
    let row = |a: &str| CValue::Arr(vec![s(a), s("0x0000000000000000"), s("0x0000000000000000")]);
    assert_eq!(
        decode_err(|o| {
            let t = sub(o, "transform");
            t.insert(
                "r".into(),
                CValue::Arr(vec![
                    row("0x4000000000000000"),
                    CValue::Arr(vec![s("0x0000000000000000"), s("0x3ff0000000000000"), s("0x0000000000000000")]),
                    CValue::Arr(vec![s("0x0000000000000000"), s("0x0000000000000000"), s("0x3ff0000000000000")]),
                ]),
            );
        }),
        "degenerate_transform"
    );
    assert_eq!(
        decode_err(|o| {
            let t = sub(o, "transform");
            t.insert("kind".into(), s("similarity"));
            t.insert("s".into(), s("0xbff0000000000000"));
        }),
        "degenerate_transform"
    );
    assert_eq!(decode_err(|o| drop(sub(o, "transform").insert("s".into(), s("0x3ff0000000000000")))), "bad_element");
    // Parameters: unknown unit, text and SI bits that disagree.
    assert_eq!(decode_err(|o| drop(sub(sub(o, "params"), "tx").insert("unit".into(), s("furlong")))), "unknown_kind");
    assert_eq!(decode_err(|o| drop(sub(sub(o, "params"), "tx").insert("text".into(), s("251")))), "bad_element");
    // Provenance: unknown kind, agent marked human.
    assert_eq!(decode_err(|o| drop(sub(o, "provenance").insert("kind".into(), s("script")))), "unknown_kind");
    assert_eq!(decode_err(|o| drop(sub(o, "provenance").insert("kind".into(), s("agent")))), "bad_element");
    // Inputs: a repeated input.
    assert_eq!(
        decode_err(|o| {
            if let Some(CValue::Arr(a)) = o.get_mut("inputs") {
                let first = a[0].clone();
                a.push(first);
            }
        }),
        "duplicate_input"
    );
    // Raw bytes: duplicate keys and oversize input.
    assert_eq!(Alignment::from_json(b"{\"id\":\"a\",\"id\":\"b\"}").unwrap_err().code(), "bad_element");
    assert_eq!(Alignment::from_json(&vec![b' '; 2 * 1024 * 1024]).unwrap_err().code(), "input_limit");
    assert_eq!(Alignment::from_json(b"[").unwrap_err().code(), "bad_element");
}

// ------------------------------------------------------------------ command refusals

#[test]
fn commands_fail_closed() {
    let p = chain3(&full());
    let mut log = Vec::new();
    let mcp = |log: &mut Vec<ActivityEntry>, tool: &str, params: &[u8]| {
        invoke_mcp(&p.model, log, tool, params).unwrap_err().code().to_string()
    };
    assert_eq!(mcp(&mut log, "align_list_alignments", b"{\"status\":\"done\"}"), "unknown_kind");
    assert_eq!(mcp(&mut log, "align_list_alignments", b"{\"colour\":\"red\"}"), "bad_params");
    assert_eq!(mcp(&mut log, "align_list_alignments", b"{\"status\":1}"), "bad_params");
    assert_eq!(mcp(&mut log, "align_list_alignments", b"{\"parent\":\"x\"}"), "bad_params");
    assert_eq!(mcp(&mut log, "align_list_alignments", b"[]"), "bad_params");
    assert_eq!(mcp(&mut log, "align_list_alignments", b"{\"status\":\"ok\",\"status\":\"ok\"}"), "bad_params");
    assert_eq!(mcp(&mut log, "align_list_alignments", &vec![b' '; MAX_PARAMS_BYTES + 1]), "input_limit");
    assert_eq!(mcp(&mut log, "align_get_alignment", b"{}"), "bad_params");
    assert_eq!(mcp(&mut log, "align_get_alignment", b"{\"id\":\"00000000-0000-7000-8000-000000000123\"}"), "not_found");
    assert_eq!(mcp(&mut log, "align_delete_alignment", b"{}"), "unknown_kind");
    let cli =
        |log: &mut Vec<ActivityEntry>, args: &[&str]| invoke_cli(&p.model, log, args).unwrap_err().code().to_string();
    assert_eq!(cli(&mut log, &[]), "bad_params");
    assert_eq!(cli(&mut log, &["delete-alignment"]), "unknown_kind");
}

#[test]
fn cli_options_fail_closed() {
    let p = chain3(&full());
    let mut log = Vec::new();
    let a0 = p.id("A0").to_string();
    let mut cli = |args: &[&str]| invoke_cli(&p.model, &mut log, args).unwrap_err().code().to_string();
    assert_eq!(cli(&["list-alignments", "--colour", "red"]), "bad_params");
    assert_eq!(cli(&["list-alignments", "--json", "--json"]), "bad_params");
    assert_eq!(cli(&["list-alignments", "--status", "ok", "--status", "ok"]), "bad_params");
    assert_eq!(cli(&["list-alignments", "--status"]), "bad_params");
    assert_eq!(cli(&["list-alignments", &a0]), "bad_params");
    assert_eq!(cli(&["get-alignment"]), "bad_params");
    assert_eq!(cli(&["get-alignment", &a0, &a0]), "bad_params");
    assert_eq!(cli(&["get-alignment", "--parent", &a0]), "bad_params");
    assert_eq!(cli(&["get-alignment", &a0.to_uppercase()]), "bad_params");
    let many = ["--json"; 20];
    assert_eq!(cli(&many), "input_limit");
}

#[test]
fn command_definitions_are_r0_and_d0() {
    let defs = command_defs().unwrap();
    let ids: Vec<String> = defs.iter().map(|d| format!("{}@{}", d.id, d.major)).collect();
    assert_eq!(ids, vec![GET_ALIGNMENT.to_string(), LIST_ALIGNMENTS.to_string()]);
    for d in &defs {
        assert_eq!(d.side_effect.as_str(), "R0");
        assert_eq!(d.determinism.as_str(), "D0");
    }
}

// ------------------------------------------------------------------ review fixes (#230)

fn create(p: &mut Project, solvers: &SolverRegistry, req: &CreateRequest, ms: u64) -> Result<Alignment, AlignError> {
    let mut src = replay(ms, 0x2a);
    let existing: BTreeSet<Uuid> = BTreeSet::new();
    let mut ids = IdService::new(Uuid::from_u128(1), &mut src, &existing);
    p.model.create(req, solvers, &p.world, &mut p.edges, &mut ids).map(|c| c.alignment)
}

fn statuses(entry: &fid_align::RecomputeEntry) -> Vec<(Uuid, AlignmentStatus)> {
    entry.items.iter().map(|i| (i.id, i.status)).collect()
}

#[test]
fn a_pending_non_alignment_outdates_everything_downstream() {
    // changed = [M]: the planes are downstream of M but not recomputed yet, so every
    // alignment is outdated, never solved against a stale plane digest.
    let solvers = full();
    let mut p = chain3(&solvers);
    let m = p.id("M");
    let entry = p.model.recompute(&[m], &solvers, &p.world, &p.edges).unwrap();
    let want: Vec<(Uuid, AlignmentStatus)> =
        ["A0", "A1", "A2"].iter().map(|n| (p.id(n), AlignmentStatus::Outdated)).collect();
    assert_eq!(statuses(&entry), want);
    for n in ["A0", "A1", "A2"] {
        assert_eq!(p.get(n).digest().as_str(), exp("digest", n), "{n} keeps its content");
    }
}

#[test]
fn an_alignment_behind_a_pending_feature_is_outdated_unless_the_feature_is_changed_too() {
    // A0 -> F -> A3, where F is a non-alignment element computed from A0.
    let solvers = full();
    let mut p = chain3(&solvers);
    let (a0, f) = (p.id("A0"), Uuid::from_u128(0x0f0f));
    p.world.insert(f, Digest::from_bytes([7; 32]));
    p.edges.apply(&[EdgesSet::new(f, vec![a0]).unwrap()]).unwrap();
    let mut req = p.request(&p.specs[0]);
    req.inputs = vec![f];
    let a3 = create(&mut p, &solvers, &req, 1_763_510_400_200).unwrap().id();
    let p0 = p.id("P0");
    let entry = p.model.recompute(&[p0], &solvers, &p.world, &p.edges).unwrap();
    let got = statuses(&entry);
    assert!(got.contains(&(a0, AlignmentStatus::Ok)), "A0 is re-solved: {got:?}");
    assert!(got.contains(&(a3, AlignmentStatus::Outdated)), "F is pending, so A3 is outdated: {got:?}");
    // Once CORE.DEP has recomputed F and names it as changed, A3 solves again.
    let entry = p.model.recompute(&[f], &solvers, &p.world, &p.edges).unwrap();
    assert_eq!(statuses(&entry), vec![(a3, AlignmentStatus::Ok)]);
}

#[test]
fn an_alignment_input_must_be_ok_like_a_parent() {
    let mut solvers = full();
    let mut p = chain3(&solvers);
    let (a1, p2) = (p.id("A1"), p.id("P2"));
    let mut req = p.request(&p.specs[0]);
    req.inputs = vec![a1, p2];
    let a3 = create(&mut p, &solvers, &req, 1_763_510_400_200).unwrap().id();
    assert!(solvers.remove(&alg(V110)));
    p.model.recompute(&[a1], &solvers, &p.world, &p.edges).unwrap();
    assert_eq!(p.get("A1").status(), AlignmentStatus::Failed);
    assert_eq!(p.model.get(&a3).unwrap().status(), AlignmentStatus::Outdated);
    // A1 is not in this pass; A3 is outdated by the input rule alone.
    let entry = p.model.recompute(&[p2], &solvers, &p.world, &p.edges).unwrap();
    assert!(statuses(&entry).contains(&(a3, AlignmentStatus::Outdated)));
    // Creation refuses a failed alignment as an input.
    let (err, left) = try_create(&mut p, &solvers, &req);
    assert_eq!((err, left), (AlignError::ParentUnusable(a1), 2));
}

#[test]
fn negative_zero_is_stored_as_positive_zero() {
    let solvers = full();
    let mut p = chain3(&solvers);
    let mut req = p.request(&p.specs[0]);
    let mut params = ParamSet::new();
    for (n, t) in [("tx", "1"), ("ty", "-0"), ("tz", "-0")] {
        params.insert(n, t, "mm").unwrap();
    }
    req.params = params;
    let a = create(&mut p, &solvers, &req, 1_763_510_400_200).unwrap();
    assert_eq!(a.params().get("tz").map(|q| q.si().to_bits()), Some(1 << 63), "the authored value keeps its sign");
    let t = a.transform().translation();
    assert_eq!([t[1].to_bits(), t[2].to_bits()], [0, 0]);
    let back = Alignment::from_json(&a.to_json().unwrap()).unwrap();
    assert_eq!(back.digest(), a.digest());
    // A stored -0.0 is not canonical and is refused.
    assert_eq!(
        decode_err(|o| drop(sub(o, "transform").insert(
            "t".into(),
            CValue::Arr(vec![s("0x8000000000000000"), s("0x0000000000000000"), s("0x0000000000000000")])
        ))),
        "bad_element"
    );
}

#[test]
fn a_subnormal_scale_and_a_bad_parameter_name_are_refused_on_decoding() {
    assert_eq!(
        decode_err(|o| {
            let t = sub(o, "transform");
            t.insert("kind".into(), s("similarity"));
            t.insert("s".into(), s("0x0000000000000001"));
        }),
        "degenerate_transform"
    );
    assert_eq!(
        decode_err(|o| {
            let params = sub(o, "params");
            let tx = params.remove("tx").unwrap();
            params.insert("Tx".into(), tx);
        }),
        "bad_element"
    );
}

#[test]
fn a_changed_feature_that_depends_on_a_re_solved_alignment_is_still_pending() {
    // Re-review repro: P0 shifted, recompute(&[P0, F]). A0 is re-solved in this pass, so F
    // (computed from A0) is stale even though it is listed as changed; A3 must be outdated.
    let solvers = full();
    let mut p = chain3(&solvers);
    let (a0, p0, f) = (p.id("A0"), p.id("P0"), Uuid::from_u128(0x0f0f));
    p.world.insert(f, Digest::from_bytes([7; 32]));
    p.edges.apply(&[EdgesSet::new(f, vec![a0]).unwrap()]).unwrap();
    let mut req = p.request(&p.specs[0]);
    req.inputs = vec![f];
    let a3 = create(&mut p, &solvers, &req, 1_763_510_400_200).unwrap().id();
    p.world.insert(p0, Digest::from_bytes([9; 32]));
    let entry = p.model.recompute(&[p0, f], &solvers, &p.world, &p.edges).unwrap();
    let got = statuses(&entry);
    assert!(got.contains(&(a0, AlignmentStatus::Ok)), "A0 is re-solved: {got:?}");
    assert!(got.contains(&(a3, AlignmentStatus::Outdated)), "F is stale behind A0, so A3 is outdated: {got:?}");
    assert_eq!(p.model.get(&a3).unwrap().status(), AlignmentStatus::Outdated);
}
