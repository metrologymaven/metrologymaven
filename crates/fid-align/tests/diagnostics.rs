//! Acceptance criterion AC-4 of spec node ALN.QA.COND (`align.get_diagnostics@1`; AC-1 to AC-3
//! and AC-5 are in `fid-align-solve`'s `tests/cond.rs`), plus the fail-closed checks of the
//! read and its result schema (checked with `fid_bus::validate::Compiled`).
//!
//! AC-4 uses fixture `fixtures/aln/model/chain3` (built here the way `tests/elem.rs` and
//! `tests/residuals.rs` build it) with a stub solver whose residual report is the hand case
//! `mixed_kinds_weights` and whose diagnostics are those of the `wide` case of
//! `fixtures/aln/qa/cond-cases`. The expected payload bytes come from
//! `fixtures/aln/qa/cond-cases/make_cond_cases.py`, an independent Python reference; nothing
//! expected is produced by the crates under test.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_align::{
    command_defs, diagnose, get_diagnostics, invoke_cli_with, invoke_mcp_with, AlgorithmId, AlignError, AlignmentModel,
    AlignmentSolver, CreateRequest, DiagnosticsParams, Disposition, Entry, EntryKind, EntryRef, InputRef, ParamSet,
    PrincipalKind, RejectReason, ReportBuilder, ReportRef, ResidualReport, ResidualStore, SolveStatus, SolverError,
    SolverOutput, SolverRegistry, Surface, Threshold, GET_DIAGNOSTICS,
};
use fid_align_solve::{
    validate, verify_locks, ConvergenceHistory, Dof, DofLocks, FrameContext, HistoryRecorder, IterationRow,
    LockFrameRef, Outcome, StoppedBy, WeightSpec,
};
use fid_bus::error::{BusError, Detail, ValidationReason};
use fid_bus::unit::Normalised;
use fid_bus::validate::{Compiled, Hooks, RefLookup};
use fid_canon::{parse, to_canonical_bytes, CValue};
use fid_dep::{EdgeIndex, EdgesSet};
use fid_geo_frame::{FrameId, FrameTree, Rigid, Transform};
use fid_project::{Bits, IdService, Observation, ReplaySource};
use fid_types::Digest;
use fid_uuid::Uuid;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

// ------------------------------------------------------------------ fixtures

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/aln")
}

fn read(rel: &str) -> String {
    std::fs::read_to_string(root().join(rel)).unwrap()
}

fn hex(s: &str) -> f64 {
    f64::from_bits(u64::from_str_radix(s.strip_prefix("0x").unwrap(), 16).unwrap())
}

fn o(v: &CValue) -> &BTreeMap<String, CValue> {
    match v {
        CValue::Obj(o) => o,
        other => panic!("not an object: {other:?}"),
    }
}

fn s(v: &CValue) -> &str {
    match v {
        CValue::Str(s) => s,
        other => panic!("not a string: {other:?}"),
    }
}

fn n(v: &CValue) -> u64 {
    match v {
        CValue::Num(f) => *f as u64,
        other => panic!("not a number: {other:?}"),
    }
}

/// The hand case `mixed_kinds_weights` of `fixtures/aln/qa/report-cases.json`.
fn report() -> ResidualReport {
    let doc = parse(read("qa/report-cases.json").as_bytes()).unwrap();
    let CValue::Arr(cases) = &o(&doc)["cases"] else { panic!("cases") };
    let c = cases.iter().find(|c| s(&o(c)["name"]) == "mixed_kinds_weights").unwrap();
    let CValue::Arr(entries) = &o(c)["entries"] else { panic!() };
    let mut b = ReportBuilder::new();
    for e in entries {
        let e = o(e);
        let entry = Entry {
            reference: EntryRef { input: n(&e["input"]) as u32, item: n(&e["item"]) },
            kind: EntryKind::parse(s(&e["kind"])).unwrap(),
            residual_m: hex(s(&e["residual"])),
            vector_m: match &e["vector"] {
                CValue::Arr(v) => Some([hex(s(&v[0])), hex(s(&v[1])), hex(s(&v[2]))]),
                _ => None,
            },
            weight: match &e["weight"] {
                CValue::Str(w) => Some(hex(w)),
                _ => None,
            },
        };
        let d = match &e["reason"] {
            CValue::Str(r) => Disposition::Rejected(RejectReason::new(r).unwrap()),
            _ => Disposition::Used,
        };
        b.push(entry, d).unwrap();
    }
    b.finish().unwrap()
}

/// The `wide` case of `fixtures/aln/qa/cond-cases`: two rows, six parameters.
const WIDE_ROWS: [f64; 12] = [1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.5];
const WIDE_POINTS: [[f64; 3]; 2] = [[1.0, 0.0, 0.0], [-1.0, 0.0, 0.0]];

fn wide() -> fid_align::AlignmentDiagnostics {
    diagnose(&WIDE_ROWS, &Dof::ALL, &WIDE_POINTS, Threshold::DEFAULT).unwrap()
}

// ------------------------------------------------------------------ chain3

const ROT: [[f64; 3]; 3] = [[0.0, -1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]];

struct Stub {
    id: AlgorithmId,
    report: ReportRef,
}

impl AlignmentSolver for Stub {
    fn algorithm_id(&self) -> AlgorithmId {
        self.id.clone()
    }

    fn solve(&self, _: &[InputRef], params: &ParamSet, _: Option<&Transform>) -> Result<SolverOutput, SolverError> {
        let p = |n: &str| params.get(n).map(|q| q.si()).ok_or_else(|| SolverError::new("missing_param").unwrap());
        let rigid = Rigid::new(ROT, [p("tx")?, p("ty")?, p("tz")?]).unwrap();
        Ok(SolverOutput { transform: Transform::Rigid(rigid), report: self.report, status: SolveStatus::Ok })
    }
}

struct Chain3 {
    model: AlignmentModel,
    names: BTreeMap<String, Uuid>,
    report_id: Uuid,
    journal: Vec<Vec<u8>>,
}

fn lines(rel: &str) -> Vec<Vec<String>> {
    read(rel)
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(|l| l.split(' ').map(str::to_string).collect())
        .collect()
}

fn chain3(report: &ResidualReport) -> Chain3 {
    let l = lines("model/chain3/project.txt").into_iter().find(|l| l[0] == "report_id").unwrap();
    let report_id = Uuid::parse(&l[1]).unwrap();
    let rref = ReportRef { id: report_id, digest: report.digest() };
    let mut solvers = SolverRegistry::new();
    for v in ["aln.stub.fixed@1.0.0", "aln.stub.fixed@1.1.0"] {
        solvers.register(Box::new(Stub { id: AlgorithmId::parse(v).unwrap(), report: rref })).unwrap();
    }
    let mut p = Chain3 { model: AlignmentModel::new(), names: BTreeMap::new(), report_id, journal: Vec::new() };
    let mut world = BTreeMap::new();
    let mut edges = EdgeIndex::new();
    let mut specs = Vec::new();
    let mut edge_lines = Vec::new();
    for l in lines("model/chain3/project.txt") {
        match l[0].as_str() {
            "element" => {
                let id = Uuid::parse(&l[2]).unwrap();
                p.names.insert(l[1].clone(), id);
                world.insert(id, Digest::parse(&l[3]).unwrap());
            }
            "edge" => edge_lines.push((l[1].clone(), l[2].clone())),
            "alignment" => specs.push(l),
            _ => {}
        }
    }
    let mut sets = vec![EdgesSet::new(p.names["M"], vec![]).unwrap()];
    for (from, to) in &edge_lines {
        sets.push(EdgesSet::new(p.names[to], vec![p.names[from]]).unwrap());
    }
    for s in &sets {
        edges.apply(std::slice::from_ref(s)).unwrap();
    }
    for l in specs {
        let mut params = ParamSet::new();
        for t in &l[9..] {
            let (name, rest) = t.split_once('=').unwrap();
            let (text, unit) = rest.rsplit_once(':').unwrap();
            params.insert(name, text, unit).unwrap();
        }
        let req = CreateRequest {
            algorithm: AlgorithmId::parse(&l[2]).unwrap(),
            inputs: l[4].split(',').map(|n| p.names[n]).collect(),
            params,
            parent: (l[3] != "-").then(|| p.names[&l[3]]),
            principal: l[5].clone(),
            kind: PrincipalKind::parse(&l[6]).unwrap(),
        };
        let mut src = ReplaySource::new(vec![
            Observation::NowMs(l[7].parse().unwrap()),
            Observation::RandomBits(Bits::new(74, u128::from_str_radix(&l[8], 16).unwrap()).unwrap()),
        ]);
        let existing: BTreeSet<Uuid> = BTreeSet::new();
        let mut ids = IdService::new(Uuid::from_u128(1), &mut src, &existing);
        let created = p.model.create(&req, &solvers, &world, &mut edges, &mut ids).unwrap();
        p.journal.push(to_canonical_bytes(&created.alignment.to_value()).unwrap());
        p.journal.push(to_canonical_bytes(&created.edges.to_value()).unwrap());
        p.names.insert(l[1].clone(), created.alignment.id());
    }
    p
}

fn snapshot(m: &AlignmentModel) -> Vec<Vec<u8>> {
    m.iter().map(|a| a.to_json().unwrap()).collect()
}

fn store_with(report: ResidualReport, report_id: Uuid, d: fid_align::AlignmentDiagnostics) -> ResidualStore {
    let mut store = ResidualStore::new();
    store.insert(report_id, report).unwrap();
    store.insert_diagnostics(report_id, d).unwrap();
    store
}

// ------------------------------------------------------------------ schema

struct NoHooks;

impl Hooks for NoHooks {
    fn literal(&self, _: &str, _: &CValue) -> Result<Normalised, BusError> {
        Err(BusError::Validation(ValidationReason::UnitMismatch, Detail::new("no quantity literals here")))
    }
    fn entity(&self, _: &str) -> RefLookup {
        RefLookup::Missing
    }
}

fn schema() -> Compiled {
    let def = command_defs().unwrap().into_iter().find(|d| d.id == "align.get_diagnostics").unwrap();
    Compiled::compile(&def.result_schema).unwrap()
}

fn assert_valid(v: &CValue, what: &str) {
    if let Err(f) = schema().evaluate(v, &NoHooks) {
        panic!("{what}: {} at {} ({})", f.keyword, f.pointer, f.message);
    }
}

// ------------------------------------------------------------------ AC-4

#[test]
fn ac4_cli_and_mcp_reads_are_identical_include_the_history_and_leave_the_record_journal_unchanged() {
    let report = report();
    let p = chain3(&report);
    let store = store_with(report, p.report_id, wide());
    let a2 = p.names["A2"].to_string();
    let before = (p.journal.clone(), snapshot(&p.model));
    let mut log = Vec::new();

    let cli = invoke_cli_with(&p.model, &store, &mut log, &["get-diagnostics", &a2, "--json"]).unwrap();
    let params = format!("{{\"alignment\":\"{a2}\"}}");
    let mcp = invoke_mcp_with(&p.model, &store, &mut log, "align_get_diagnostics", params.as_bytes()).unwrap();
    assert_eq!(cli, mcp);
    assert_eq!(String::from_utf8(cli.clone()).unwrap(), read("qa/cond-cases/get-diagnostics-a2.json"));
    assert_eq!(fid_canon::canonicalize(&cli).unwrap(), cli);
    // The history field is present and null for a closed-form method.
    let v = parse(&cli).unwrap();
    assert_eq!(o(&v).get("history"), Some(&CValue::Null));
    assert_valid(&v, "closed-form payload");
    // Without --json the bytes are the same.
    let plain = invoke_cli_with(&p.model, &store, &mut log, &["get-diagnostics", &a2]).unwrap();
    assert_eq!(plain, mcp);

    // R0: every call is in the Activity Log; the Record Journal and the model are unchanged.
    assert_eq!(log.len(), 3);
    assert!(log.iter().all(|e| e.command == GET_DIAGNOSTICS));
    let surfaces: Vec<Surface> = log.iter().map(|e| e.surface).collect();
    assert_eq!(surfaces, vec![Surface::Cli, Surface::Mcp, Surface::Cli]);
    assert_eq!((p.journal.clone(), snapshot(&p.model)), before);
}

/// A two-iteration history of a run over the two `wide` points, finished on a certified pose.
fn history() -> ConvergenceHistory {
    let tree = FrameTree::new();
    let ctx = FrameContext { tree: &tree, parent: FrameId::WORLD };
    let cs = validate(DofLocks::NONE, LockFrameRef::Parent, &ctx, &Rigid::IDENTITY, &WeightSpec::default(), 2).unwrap();
    let pose = verify_locks(&cs, &Rigid::IDENTITY).unwrap();
    let mut rec = HistoryRecorder::new();
    rec.record(IterationRow { iteration: 1, objective: 0.5, d_translation: 1e-3, d_rotation: 2e-4, n_used: 2 })
        .unwrap();
    rec.record(IterationRow { iteration: 2, objective: 0.25, d_translation: 1e-9, d_rotation: 0.0, n_used: 2 })
        .unwrap();
    rec.finish(StoppedBy::new(Outcome::Converged, "increment").unwrap(), &cs, &pose).unwrap()
}

#[test]
fn an_iterative_method_returns_its_history_on_both_surfaces() {
    let report = report();
    let p = chain3(&report);
    let h = history();
    let store = store_with(report, p.report_id, wide().with_history(h.clone()));
    let a2 = p.names["A2"].to_string();
    let mut log = Vec::new();
    let cli = invoke_cli_with(&p.model, &store, &mut log, &["get-diagnostics", "--json", &a2]).unwrap();
    let params = format!("{{\"alignment\":\"{a2}\"}}");
    let mcp = invoke_mcp_with(&p.model, &store, &mut log, "align_get_diagnostics", params.as_bytes()).unwrap();
    assert_eq!(cli, mcp);
    let v = parse(&cli).unwrap();
    assert_valid(&v, "iterative payload");
    let hist = o(&o(&v)["history"]);
    assert_eq!(s(&hist["digest"]), h.digest().as_str());
    assert_eq!(n(&hist["n_rows"]), 2);
    let rows = o(&hist["rows"]);
    assert_eq!(rows["iteration"], CValue::Arr(vec![CValue::Num(1.0), CValue::Num(2.0)]));
    assert_eq!(o(&rows["d_rotation"])["unit"], CValue::Str("rad".into()));
    let bits = |x: f64| CValue::Str(format!("0x{:016x}", x.to_bits()));
    assert_eq!(rows["objective"], CValue::Arr(vec![bits(0.5), bits(0.25)]));
    let stop = o(&hist["stopped_by"]);
    assert_eq!(s(&stop["outcome"]), "converged");
    assert_eq!(s(&stop["criterion"]), "increment");
    // Everything but the history equals the closed-form payload.
    let closed = parse(read("qa/cond-cases/get-diagnostics-a2.json").as_bytes()).unwrap();
    let strip = |v: &CValue| {
        let mut m = o(v).clone();
        m.remove("history");
        m
    };
    assert_eq!(strip(&v), strip(&closed));
}

// ------------------------------------------------------------------ refusals and schema

#[test]
fn diagnostics_reads_fail_closed() {
    let report = report();
    let p = chain3(&report);
    let a2 = p.names["A2"];
    let params = DiagnosticsParams { alignment: a2 };

    // A report without diagnostics, and no report at all: not_found.
    let mut store = ResidualStore::new();
    assert_eq!(get_diagnostics(&p.model, &store, &params).unwrap_err(), AlignError::NotFound(p.report_id));
    assert_eq!(store.insert_diagnostics(p.report_id, wide()).unwrap_err().code(), "not_found");
    store.insert(p.report_id, report.clone()).unwrap();
    assert_eq!(get_diagnostics(&p.model, &store, &params).unwrap_err(), AlignError::NotFound(p.report_id));
    // Stored once; the same again is a no-op; different diagnostics are a collision.
    store.insert_diagnostics(p.report_id, wide()).unwrap();
    store.insert_diagnostics(p.report_id, wide()).unwrap();
    let other = diagnose(&[1.0, 0.0, 0.0, 1.0], &[Dof::Tx, Dof::Ty], &WIDE_POINTS, Threshold::DEFAULT).unwrap();
    assert_eq!(store.insert_diagnostics(p.report_id, other).unwrap_err().code(), "id_collision");
    assert!(get_diagnostics(&p.model, &store, &params).is_ok());
    // An unknown alignment.
    let unknown = DiagnosticsParams { alignment: Uuid::from_u128(77) };
    assert_eq!(get_diagnostics(&p.model, &store, &unknown).unwrap_err().code(), "not_found");

    // A report under the alignment's report ID that is not the one it recorded.
    let other_report = {
        let mut b = ReportBuilder::new();
        let e = Entry {
            reference: EntryRef { input: 0, item: 0 },
            kind: EntryKind::Point,
            residual_m: 0.001,
            vector_m: None,
            weight: None,
        };
        b.push(e, Disposition::Used).unwrap();
        b.finish().unwrap()
    };
    let swapped = store_with(other_report, p.report_id, wide());
    assert_eq!(get_diagnostics(&p.model, &swapped, &params).unwrap_err(), AlignError::DigestMismatch);

    // Parameters, on both surfaces.
    let mut log = Vec::new();
    let a2s = a2.to_string();
    let mut mcp = |params: &str| {
        invoke_mcp_with(&p.model, &store, &mut log, "align_get_diagnostics", params.as_bytes())
            .map_err(|e| e.code().to_string())
    };
    assert_eq!(mcp("{}").unwrap_err(), "bad_params");
    assert_eq!(mcp(&format!("{{\"alignment\":\"{a2s}\",\"x\":1}}")).unwrap_err(), "bad_params");
    assert_eq!(mcp("{\"alignment\":\"not-an-id\"}").unwrap_err(), "bad_params");
    assert_eq!(mcp("{\"alignment\":7}").unwrap_err(), "bad_params");
    assert_eq!(mcp(&format!("{{\"alignment\":\"{}\"}}", a2s.to_uppercase())).unwrap_err(), "bad_params");
    let mut log = Vec::new();
    let mut cli =
        |args: &[&str]| invoke_cli_with(&p.model, &store, &mut log, args).map(|_| ()).map_err(|e| e.code().to_string());
    assert_eq!(cli(&["get-diagnostics"]).unwrap_err(), "bad_params");
    assert_eq!(cli(&["get-diagnostics", &a2s, &a2s]).unwrap_err(), "bad_params");
    assert_eq!(cli(&["get-diagnostics", &a2s, "--json", "--json"]).unwrap_err(), "bad_params");
    assert_eq!(cli(&["get-diagnostics", &a2s, "--summary-only"]).unwrap_err(), "bad_params");
    assert!(cli(&["get-diagnostics", &a2s]).is_ok());
}

#[test]
fn the_definition_is_r0_d0_and_the_schema_rejects_a_measured_looking_threshold() {
    let def = command_defs().unwrap().into_iter().find(|d| d.id == "align.get_diagnostics").unwrap();
    assert_eq!(def.side_effect.as_str(), "R0");
    assert_eq!(def.determinism.as_str(), "D0");
    let v = parse(read("qa/cond-cases/get-diagnostics-a2.json").as_bytes()).unwrap();
    // D-045: a threshold without its basis (or with an unknown one) does not validate.
    let mut bad = v.clone();
    if let CValue::Obj(top) = &mut bad {
        if let Some(CValue::Obj(c)) = top.get_mut("conditioning") {
            if let Some(CValue::Obj(t)) = c.get_mut("threshold") {
                t.insert("basis".into(), CValue::Str("calibrated".into()));
            }
        }
    }
    assert!(schema().evaluate(&bad, &NoHooks).is_err());
    let mut bad = v.clone();
    if let CValue::Obj(top) = &mut bad {
        if let Some(CValue::Arr(w)) = top.get_mut("warnings") {
            w.push(w[0].clone());
        }
    }
    assert!(schema().evaluate(&bad, &NoHooks).is_err(), "at most one warning");
}
