//! Acceptance criteria AC-1, AC-4 and AC-5 of spec node ALN.QA.REPORT (AC-2 and AC-3 are in
//! `fid-align-solve`'s `tests/report.rs`), plus the fail-closed checks of
//! `align.get_residuals@1`. Every expected statistic, digest and output byte comes from
//! `fixtures/aln/qa/make_report_fixtures.py`, an independent Python reference of the rules
//! pinned in the leaf's Design; nothing expected is produced by the crates under test.
//!
//! AC-4 uses fixture `fixtures/aln/model/chain3` (built here the way `tests/elem.rs` builds
//! it) with a stub solver whose residual report is the hand case `mixed_kinds_weights`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_align::{
    get_residuals, invoke_cli_with, invoke_mcp_with, AlgorithmId, AlignError, AlignmentModel, AlignmentSolver,
    CreateRequest, Disposition, Entry, EntryKind, EntryRef, InputRef, ParamSet, PrincipalKind, RejectReason,
    ReportBuilder, ReportError, ReportRef, ResidualReport, ResidualStore, ResidualsParams, SolveStatus, SolverError,
    SolverOutput, SolverRegistry, Surface, GET_RESIDUALS, MAX_PAGE,
};
use fid_canon::{parse, to_canonical_bytes, CValue};
use fid_dep::{EdgeIndex, EdgesSet};
use fid_geo_frame::{Rigid, Transform};
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

fn bits(x: f64) -> String {
    format!("0x{:016x}", x.to_bits())
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

fn cases() -> Vec<CValue> {
    let doc = parse(read("qa/report-cases.json").as_bytes()).unwrap();
    match &o(&doc)["cases"] {
        CValue::Arr(c) => c.clone(),
        _ => panic!("cases"),
    }
}

fn case(name: &str) -> CValue {
    cases().into_iter().find(|c| s(&o(c)["name"]) == name).unwrap()
}

/// Pushes the entries of a fixture case; returns the builder or the first refusal and its
/// entry index.
fn push_case(c: &CValue) -> Result<ReportBuilder, (ReportError, u64)> {
    let CValue::Arr(entries) = &o(c)["entries"] else { panic!() };
    let mut b = ReportBuilder::new();
    for (i, e) in entries.iter().enumerate() {
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
        assert_eq!(e["used"], CValue::Bool(matches!(d, Disposition::Used)));
        b.push(entry, d).map_err(|err| (err, i as u64))?;
    }
    Ok(b)
}

fn report_of(name: &str) -> ResidualReport {
    push_case(&case(name)).unwrap().finish().unwrap()
}

// ------------------------------------------------------------------ AC-1

fn opt_bits(l: Option<fid_align_solve::Length>) -> CValue {
    l.map_or(CValue::Null, |l| CValue::Str(bits(l.value())))
}

#[test]
fn ac1_hand_cases_give_the_reference_statistics_exactly() {
    let all = cases();
    assert_eq!(all.len(), 16, "case count");
    let mut checked = 0;
    for c in &all {
        let name = s(&o(c)["name"]).to_string();
        let built = push_case(c).and_then(|b| b.finish().map_err(|e| (e, u64::MAX)));
        if let Some(exp) = o(c).get("expect") {
            let exp = o(exp);
            let rep = built.unwrap_or_else(|e| panic!("{name}: {e:?}"));
            let st = rep.stats();
            assert_eq!(opt_bits(st.rms()), exp["rms"], "{name} rms");
            assert_eq!(opt_bits(st.weighted_rms()), exp["weighted_rms"], "{name} weighted_rms");
            assert_eq!(opt_bits(st.max_abs()), exp["max_abs"], "{name} max_abs");
            assert_eq!(st.max_entry().map_or(CValue::Null, |k| CValue::Num(k as f64)), exp["max_entry"], "{name}");
            assert_eq!(st.n_used(), n(&exp["n_used"]), "{name} n_used");
            assert_eq!(st.n_rejected(), n(&exp["n_rejected"]), "{name} n_rejected");
            assert_eq!(std::str::from_utf8(rep.stats_json()).unwrap(), s(&exp["stats_json"]), "{name} stats json");
            assert_eq!(rep.digest().as_str(), s(&exp["digest"]), "{name} digest");
        } else {
            let err = o(&o(c)["error"]);
            let (e, at) = built.err().unwrap_or_else(|| panic!("{name} should fail"));
            assert_eq!(e.code(), s(&err["code"]), "{name}");
            match &err["index"] {
                CValue::Num(i) => assert_eq!(at, *i as u64, "{name} index"),
                _ => assert_eq!(at, u64::MAX, "{name} fails in finish"),
            }
            if e.code() == "nan_residual" {
                assert_eq!(e, ReportError::NanResidual { index: at });
            }
        }
        checked += 1;
    }
    assert_eq!(checked, 16);
}

// ------------------------------------------------------------------ chain3 with real reports

const ROT: [[f64; 3]; 3] = [[0.0, -1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]];

/// chain3's stub solver, returning a fixed residual-report reference.
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

fn report_id() -> Uuid {
    let l = lines("model/chain3/project.txt").into_iter().find(|l| l[0] == "report_id").unwrap();
    Uuid::parse(&l[1]).unwrap()
}

/// chain3 with every alignment created through the wrapper; each alignment's report is
/// `report` under the fixture's report ID. Both stub versions solve rigidly here: only the
/// IDs (drawn from the replay source, not from the solver) matter to these tests.
fn chain3(report: &ResidualReport) -> Chain3 {
    let report_id = report_id();
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

fn expected_id(name: &str) -> String {
    lines("model/chain3/expected.txt").into_iter().find(|l| l[0] == "id" && l[1] == name).unwrap()[2].clone()
}

fn snapshot(m: &AlignmentModel) -> Vec<Vec<u8>> {
    m.iter().map(|a| a.to_json().unwrap()).collect()
}

// ------------------------------------------------------------------ AC-4

#[test]
fn ac4_cli_and_mcp_reads_are_identical_and_leave_the_record_journal_unchanged() {
    let report = report_of("mixed_kinds_weights");
    let p = chain3(&report);
    let mut store = ResidualStore::new();
    store.insert(p.report_id, report).unwrap();
    let a2 = p.names["A2"].to_string();
    assert_eq!(a2, expected_id("A2"));
    let before = (p.journal.clone(), snapshot(&p.model));
    let mut log = Vec::new();

    let cli = invoke_cli_with(&p.model, &store, &mut log, &["get-residuals", &a2, "--summary-only", "--json"]).unwrap();
    let params = format!("{{\"summary_only\":true,\"alignment\":\"{a2}\"}}");
    let mcp = invoke_mcp_with(&p.model, &store, &mut log, "align_get_residuals", params.as_bytes()).unwrap();
    assert_eq!(cli, mcp);
    assert_eq!(String::from_utf8(cli).unwrap(), read("qa/get-residuals-a2-summary.json"));

    let cli =
        invoke_cli_with(&p.model, &store, &mut log, &["get-residuals", "--offset", "1", &a2, "--limit", "3"]).unwrap();
    let params = format!("{{\"alignment\":\"{a2}\",\"offset\":1,\"limit\":3}}");
    let mcp = invoke_mcp_with(&p.model, &store, &mut log, "align_get_residuals", params.as_bytes()).unwrap();
    assert_eq!(cli, mcp);
    assert_eq!(String::from_utf8(cli.clone()).unwrap(), read("qa/get-residuals-a2-page.json"));
    // Canonicalising the payloads changes nothing: they are already RFC 8785.
    assert_eq!(fid_canon::canonicalize(&cli).unwrap(), cli);

    // R0: every call is in the Activity Log; the Record Journal and the model are unchanged.
    assert_eq!(log.len(), 4);
    assert!(log.iter().all(|e| e.command == GET_RESIDUALS));
    let surfaces: Vec<Surface> = log.iter().map(|e| e.surface).collect();
    assert_eq!(surfaces, vec![Surface::Cli, Surface::Mcp, Surface::Cli, Surface::Mcp]);
    assert_eq!((p.journal.clone(), snapshot(&p.model)), before);
}

// ------------------------------------------------------------------ AC-5

fn splitmix64(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// The seeded generator pinned in the node (seed 13), as in `fid-align-solve`'s tests.
fn big_report() -> ResidualReport {
    let outlier = RejectReason::new("outlier").unwrap();
    let two_m53 = f64::from_bits(0x3CA0_0000_0000_0000);
    let mut b = ReportBuilder::new();
    let mut state = 13u64;
    for i in 0..1_000_000usize {
        let z = splitmix64(&mut state);
        let kind = EntryKind::ALL[(z & 3) as usize];
        let r = ((z >> 11) as f64 * two_m53 - 0.5) * 0.002;
        let entry = Entry {
            reference: EntryRef { input: (i % 3) as u32, item: i as u64 },
            kind,
            residual_m: r,
            vector_m: (kind == EntryKind::Point).then(|| [r * 0.6, r * 0.8, 0.0]),
            weight: Some(1.0 + ((z >> 6) & 7) as f64 * 0.25),
        };
        let d = if (z >> 2) & 15 != 0 { Disposition::Used } else { Disposition::Rejected(outlier.clone()) };
        b.push(entry, d).unwrap();
    }
    b.finish().unwrap()
}

fn golden() -> BTreeMap<String, String> {
    read("qa/report-1m.txt")
        .lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
        .map(|l| {
            let (k, v) = l.split_once(' ').unwrap();
            (k.to_string(), v.to_string())
        })
        .collect()
}

#[test]
fn ac5_the_last_page_of_a_million_entries_and_the_page_cap() {
    let g = golden();
    let report = big_report();
    assert_eq!(report.digest().as_str(), g["digest"]);
    let p = chain3(&report);
    let mut store = ResidualStore::new();
    store.insert(p.report_id, report).unwrap();
    let a2 = p.names["A2"];
    let params = ResidualsParams { alignment: a2, summary_only: false, offset: 900_000, limit: 100_000, digest: None };
    let out = get_residuals(&p.model, &store, &params).unwrap();
    let page = o(&o(&out)["page"]);
    let CValue::Arr(entries) = &page["entries"] else { panic!() };
    assert_eq!(entries.len(), 100_000);
    let idx: Vec<u64> = entries.iter().map(|e| n(&o(e)["index"])).collect();
    assert_eq!(idx, (900_000..1_000_000).collect::<Vec<u64>>());
    let residual = |e: &CValue| match &o(&o(e)["residual"])["value"] {
        CValue::Num(x) => bits(*x),
        _ => panic!(),
    };
    assert_eq!(residual(&entries[0]), g["entry_900000"]);
    assert_eq!(residual(&entries[99_999]), g["entry_999999"]);
    assert_eq!(page["next_offset"], CValue::Null);
    assert_eq!(s(&o(&o(&out)["summary"])["digest"]), g["digest"]);

    // limit 100001 fails with page_too_large on both surfaces.
    let mut log = Vec::new();
    let mcp = format!("{{\"alignment\":\"{a2}\",\"offset\":900000,\"limit\":100001}}");
    let err = invoke_mcp_with(&p.model, &store, &mut log, "align_get_residuals", mcp.as_bytes()).unwrap_err();
    assert_eq!(err, AlignError::PageTooLarge(100_001));
    assert_eq!(err.code(), "page_too_large");
    let a2s = a2.to_string();
    let err = invoke_cli_with(&p.model, &store, &mut log, &["get-residuals", &a2s, "--limit", "100001"]).unwrap_err();
    assert_eq!(err.code(), "page_too_large");
    assert_eq!(MAX_PAGE, 100_000);
    // A page in the middle points at the next one.
    let params = ResidualsParams { offset: 10, limit: 5, ..params };
    let out = get_residuals(&p.model, &store, &params).unwrap();
    assert_eq!(o(&o(&out)["page"])["next_offset"], CValue::Num(15.0));
}

// ------------------------------------------------------------------ refusals

#[test]
fn residual_reads_fail_closed() {
    let report = report_of("mixed_kinds_weights");
    let digest = report.digest();
    let p = chain3(&report);
    let mut store = ResidualStore::new();
    store.insert(p.report_id, report.clone()).unwrap();
    let a2 = p.names["A2"].to_string();
    let mut log = Vec::new();
    let mut mcp = |store: &ResidualStore, params: String| {
        invoke_mcp_with(&p.model, store, &mut log, "align_get_residuals", params.as_bytes())
            .map_err(|e| e.code().to_string())
    };
    let q = |rest: &str| format!("{{\"alignment\":\"{a2}\"{rest}}}");
    assert_eq!(mcp(&store, q(",\"colour\":1")).unwrap_err(), "bad_params");
    assert_eq!(mcp(&store, "{}".to_string()).unwrap_err(), "bad_params");
    assert_eq!(mcp(&store, "{\"alignment\":\"x\"}".to_string()).unwrap_err(), "bad_params");
    assert_eq!(mcp(&store, q(",\"summary_only\":1")).unwrap_err(), "bad_params");
    assert_eq!(mcp(&store, q(",\"summary_only\":true,\"offset\":0")).unwrap_err(), "bad_params");
    assert_eq!(mcp(&store, q(",\"summary_only\":true,\"limit\":5")).unwrap_err(), "bad_params");
    assert_eq!(mcp(&store, q(",\"limit\":0")).unwrap_err(), "bad_params");
    assert_eq!(mcp(&store, q(",\"limit\":1.5")).unwrap_err(), "bad_params");
    assert_eq!(mcp(&store, q(",\"offset\":-1")).unwrap_err(), "bad_params");
    assert_eq!(mcp(&store, q(",\"offset\":\"1\"")).unwrap_err(), "bad_params");
    assert_eq!(mcp(&store, q(",\"offset\":1e300")).unwrap_err(), "bad_params");
    assert_eq!(mcp(&store, q(",\"limit\":1e300")).unwrap_err(), "bad_params");
    assert_eq!(mcp(&store, q(",\"digest\":\"abc\"")).unwrap_err(), "bad_params");
    assert_eq!(mcp(&store, q(",\"offset\":1,\"offset\":1")).unwrap_err(), "bad_params");
    // Cursor: offset n is the empty page after the end; n + 1 is invalid.
    let end = mcp(&store, q(",\"offset\":7")).unwrap();
    let end = parse(&end).unwrap();
    assert_eq!(o(&end)["page"], parse(br#"{"entries":[],"limit":1000,"next_offset":null,"offset":7}"#).unwrap());
    assert_eq!(mcp(&store, q(",\"offset\":8")).unwrap_err(), "invalid_cursor");
    // The digest pins the report the pager started from.
    assert!(mcp(&store, q(&format!(",\"digest\":\"{}\"", digest.as_str()))).is_ok());
    let other = report_of("single").digest();
    assert_eq!(mcp(&store, q(&format!(",\"digest\":\"{}\"", other.as_str()))).unwrap_err(), "report_changed");
    // Unknown alignment, a report missing from the store, a stored report that is not the
    // one the alignment recorded.
    let unknown = "{\"alignment\":\"00000000-0000-7000-8000-000000000123\"}".to_string();
    assert_eq!(mcp(&store, unknown).unwrap_err(), "not_found");
    assert_eq!(mcp(&ResidualStore::new(), q("")).unwrap_err(), "not_found");
    let mut wrong = ResidualStore::new();
    wrong.insert(p.report_id, report_of("single")).unwrap();
    assert_eq!(mcp(&wrong, q("")).unwrap_err(), "digest_mismatch");
    // The store keeps one report per ID.
    assert!(store.insert(p.report_id, report.clone()).is_ok());
    assert_eq!(store.insert(p.report_id, report_of("single")).unwrap_err(), AlignError::IdCollision(p.report_id));
    assert_eq!(store.len(), 1);
}

#[test]
fn residual_cli_options_fail_closed() {
    let report = report_of("pythagorean");
    let p = chain3(&report);
    let mut store = ResidualStore::new();
    store.insert(p.report_id, report).unwrap();
    let a2 = p.names["A2"].to_string();
    let mut log = Vec::new();
    let mut cli =
        |args: &[&str]| invoke_cli_with(&p.model, &store, &mut log, args).map(|_| ()).map_err(|e| e.code().to_string());
    assert!(cli(&["get-residuals", &a2]).is_ok());
    assert!(cli(&["get-residuals", "--summary-only", &a2, "--json"]).is_ok());
    assert_eq!(cli(&["get-residuals"]).unwrap_err(), "bad_params");
    assert_eq!(cli(&["get-residuals", &a2, &a2]).unwrap_err(), "bad_params");
    assert_eq!(cli(&["get-residuals", &a2, "--colour", "red"]).unwrap_err(), "bad_params");
    assert_eq!(cli(&["get-residuals", &a2, "--json", "--json"]).unwrap_err(), "bad_params");
    assert_eq!(cli(&["get-residuals", &a2, "--summary-only", "--summary-only"]).unwrap_err(), "bad_params");
    assert_eq!(cli(&["get-residuals", &a2, "--summary-only", "--offset", "1"]).unwrap_err(), "bad_params");
    assert_eq!(cli(&["get-residuals", &a2, "--offset"]).unwrap_err(), "bad_params");
    for bad in ["", "-1", "+1", "1.0", "0x10", "1e3", "12345678901234567", "9007199254740992"] {
        assert_eq!(cli(&["get-residuals", &a2, "--offset", bad]).unwrap_err(), "bad_params", "{bad:?}");
    }
    assert_eq!(cli(&["get-residuals", &a2, "--limit", "0"]).unwrap_err(), "bad_params");
    assert_eq!(cli(&["get-residuals", &a2, "--offset", "3"]).unwrap_err(), "invalid_cursor");
    assert_eq!(cli(&["get-residuals", &a2, "--digest", "sha256:00"]).unwrap_err(), "bad_params");
    assert_eq!(cli(&["get-residuals", &a2.to_uppercase()]).unwrap_err(), "bad_params");
}
