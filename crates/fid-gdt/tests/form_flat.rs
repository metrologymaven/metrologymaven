//! Acceptance tests for `GDT.FORM.FLAT` through the command layer (`check.eval_flatness@1`):
//! - AC-1: `fixtures/gdt/form/flat_known_mz.json` via CLI (`fid check eval-flatness`) and MCP
//!   (`check_eval_flatness`) yields width 0.008 mm within 1e-12 m and planted peak/valley indices.
//! - AC-2: 3 points fails with `too_few_points` and journals nothing.
//! - AC-3: characteristic with no governing spec fails with `governing_spec_missing` and journals nothing.
//! - AC-4: KRN stub with MZ unavailable falls back to LS and carries `conservative_upper_bound`.
//! - AC-5: exact replay of AC-1 yields `ReplayOutcome::Identical`.
//! - REQ-002: optional filter/resample step runs only when the chain names it and records its algorithm ID.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::CValue;
use fid_gdt::chars::dprof::{DprofState, KeyVocabulary};
use fid_gdt::chars::interp::{RegisterSources, RegisterStore, RegisterView};
use fid_gdt::chars::record::ResultStore;
use fid_gdt::chars::spec::{load_project, SpecState};
use fid_gdt::dmt_flow::roster::Roster;
use fid_gdt::dmt_flow::status::{CoiInputs, JournalExtract, Published, JOURNAL_SCHEMA};
use fid_gdt::drf::build::ReplayRegisters;
use fid_gdt::form::flat::{
    commit, flatness_command_def, invoke_cli, invoke_mcp, replay, EvalInputs, FormPointSets, EVAL_FLATNESS_CLI,
    EVAL_FLATNESS_COMMAND, EVAL_FLATNESS_MCP, RESULT_SCHEMA,
};
use fid_gps::dmt::{DmtTable, Operator};
use fid_gps::form::flat::{FilterProvider, FlatnessKrnProvider, NoFilterProvider};
use fid_gps::form::{FitOutcome, FitProvider, FormAssociation, FormElement, FormError, FormFlag};
use fid_io_reg::Principal;
use fid_replay::ReplayOutcome;
use fid_types::Digest;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

const H: Principal = Principal::Human;
const TOL: f64 = 1e-12;

fn fix(sub: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/gdt").join(sub)
}

fn read(sub: &str) -> Vec<u8> {
    std::fs::read(fix(sub)).unwrap()
}

fn obj(v: &CValue) -> &BTreeMap<String, CValue> {
    match v {
        CValue::Obj(o) => o,
        _ => panic!("expected object"),
    }
}

fn arr(v: &CValue) -> &[CValue] {
    match v {
        CValue::Arr(a) => a,
        _ => panic!("expected array"),
    }
}

fn st(v: &CValue) -> &str {
    match v {
        CValue::Str(s) => s,
        _ => panic!("expected string"),
    }
}

fn unhex(v: &CValue) -> f64 {
    let s = st(v);
    assert!(s.starts_with("0x") && s.len() == 18, "{s}");
    f64::from_bits(u64::from_str_radix(&s[2..], 16).unwrap())
}

fn test_vocab() -> KeyVocabulary {
    let v = fid_canon::parse(&read("dprof/vocabulary.json")).unwrap();
    let keys: Vec<&str> = arr(&obj(&v)["keys"]).iter().map(st).collect();
    KeyVocabulary::new(keys).unwrap()
}

fn table() -> DmtTable {
    let base = fid_canon::parse(&read("dmt/synthetic_table_v1.json")).unwrap();
    DmtTable::from_value(&base).unwrap()
}

fn load_flat_fixture() -> (Vec<[f64; 3]>, f64, usize, usize) {
    let doc = fid_canon::parse(&read("form/flat_known_mz.json")).unwrap();
    let o = obj(&doc);
    let expected_width_m: f64 = match &o["expected_mz_width_m"] {
        CValue::Num(n) => *n,
        CValue::Str(s) => s.parse().unwrap(),
        _ => panic!("bad expected_mz_width_m"),
    };
    let peak_idx: usize = match &o["expected_peak_index"] {
        CValue::Num(n) => *n as usize,
        CValue::Str(s) => s.parse().unwrap(),
        _ => panic!("bad expected_peak_index"),
    };
    let valley_idx: usize = match &o["expected_valley_index"] {
        CValue::Num(n) => *n as usize,
        CValue::Str(s) => s.parse().unwrap(),
        _ => panic!("bad expected_valley_index"),
    };
    let pts: Vec<[f64; 3]> = arr(&o["points"])
        .iter()
        .map(|row| {
            let a = arr(row);
            let n = |x: &CValue| match x {
                CValue::Num(d) => *d,
                CValue::Str(s) => s.parse().unwrap(),
                _ => panic!("expected point coord"),
            };
            [n(&a[0]), n(&a[1]), n(&a[2])]
        })
        .collect();
    (pts, expected_width_m, peak_idx, valley_idx)
}

struct World {
    spec: SpecState,
    dprof: DprofState,
    table: DmtTable,
    point_sets: FormPointSets,
    register: RegisterView,
}

impl World {
    fn new(chars: &[&str]) -> Self {
        let (mut spec, mut dprof) = load_project(&read("spec/plan_asme.fidproj"), &test_vocab()).unwrap();
        for c in chars {
            spec.create_characteristic(&mut dprof, H, "plan-1", c, false).unwrap();
        }
        let journal = format!(r#"{{"entries":[],"schema":"{JOURNAL_SCHEMA}"}}"#);
        let register = RegisterView::none(&JournalExtract::load(journal.as_bytes()).unwrap());
        Self { spec, dprof, table: table(), point_sets: FormPointSets::new(), register }
    }

    fn inputs(&self) -> EvalInputs<'_> {
        EvalInputs::with_default_krn(&self.spec, &self.dprof, Some(&self.table), &self.register, &self.point_sets)
    }
}

struct Regs {
    journal: JournalExtract,
    rosters: Vec<Roster>,
    coi: CoiInputs,
    published: Vec<u8>,
    prior: Published,
}

impl Regs {
    fn new() -> Self {
        Self {
            journal: JournalExtract::load(&read("interp/currency/journal.json")).unwrap(),
            rosters: vec![Roster::load(&read("status/roster.json")).unwrap()],
            coi: CoiInputs::load(&read("status/reviewers.toml"), &read("status/profiles.toml")).unwrap(),
            published: read("interp/currency/published_v1.json"),
            prior: Published::load(&read("interp/currency/published_v1.json")).unwrap(),
        }
    }

    fn sources(&self) -> RegisterSources<'_> {
        RegisterSources {
            journal: &self.journal,
            rosters: &self.rosters,
            coi: &self.coi,
            published: &self.published,
            pinned_prior: &self.prior,
        }
    }
}

struct MzUnavailableKrn;

impl FitProvider for MzUnavailableKrn {
    fn mz_available(&self, _element: FormElement) -> bool {
        false
    }

    fn fit_minimum_zone(&self, _element: FormElement, _points: &[[f64; 3]]) -> Result<FitOutcome, FormError> {
        Err(FormError::FitRefused { code: "mz_unavailable" })
    }

    fn fit_least_squares(&self, element: FormElement, points: &[[f64; 3]]) -> Result<FitOutcome, FormError> {
        FlatnessKrnProvider.fit_least_squares(element, points)
    }
}

struct PassthroughFilter;

impl FilterProvider for PassthroughFilter {
    fn apply(&self, _step: &Operator, points: &[[f64; 3]]) -> Result<Vec<[f64; 3]>, FormError> {
        Ok(points.to_vec())
    }
}

/// AC-1 & AC-5: `flat_known_mz.json` via CLI and MCP matches 0.008 mm within 1e-12 m, identifies
/// planted peak and valley indices, validates `CommandDef`, respects `--dry-run`, and replays
/// `IDENTICAL`.
#[test]
fn ac1_ac5_flat_known_mz_cli_mcp_dry_run_and_exact_replay() {
    let def = flatness_command_def().expect("CommandDef must validate");
    assert_eq!(def.id, "check.eval_flatness");
    assert_eq!(def.major, 1);

    let (pts, exp_width, exp_peak, exp_valley) = load_flat_fixture();
    let mut w = World::new(&["flat-1"]);
    w.point_sets.insert("ps-flat-mz", pts).unwrap();

    let mut store = ResultStore::new();
    let ev_cli = invoke_cli(
        &store,
        &w.inputs(),
        H,
        &[EVAL_FLATNESS_CLI, "--run", "run-1", "--char", "flat-1", "--points", "ps-flat-mz", "--json"],
    )
    .unwrap();
    let ev_mcp = invoke_mcp(
        &store,
        &w.inputs(),
        H,
        EVAL_FLATNESS_MCP,
        br#"{"run":"run-1","char":"flat-1","points":"ps-flat-mz"}"#,
    )
    .unwrap();

    // CLI and MCP outputs are byte-identical.
    assert_eq!(ev_cli.to_canonical_bytes().unwrap(), ev_mcp.to_canonical_bytes().unwrap());
    let val = ev_cli.to_value().unwrap();
    let o = obj(&val);
    assert_eq!(st(&o["schema"]), RESULT_SCHEMA);
    assert_eq!(st(&o["command"]), EVAL_FLATNESS_COMMAND);
    assert_eq!(st(&o["evaluation_basis"]), "POINTS");
    let form = obj(&o["form"]);
    let got_m = unhex(&form["value_si_hex"]);
    assert!((got_m - exp_width).abs() < TOL, "got {got_m}, exp {exp_width}");
    assert_eq!(ev_cli.payload.association, FormAssociation::MinimumZone);
    assert!(ev_cli.payload.flags.is_empty());
    assert_eq!(ev_cli.payload.peak_index, exp_peak);
    assert_eq!(ev_cli.payload.valley_index, exp_valley);
    assert!(ev_cli.filter_algorithms.is_empty());

    // Dry-run journals nothing.
    let dry = invoke_cli(
        &store,
        &w.inputs(),
        H,
        &[EVAL_FLATNESS_CLI, "--run", "run-1", "--char", "flat-1", "--points", "ps-flat-mz", "--dry-run", "--json"],
    )
    .unwrap();
    assert!(dry.event().is_none());
    assert_eq!(dry.value_si_bits(), ev_cli.value_si_bits());

    // Commit and run AC-5 exact replay.
    let ev_bytes = ev_cli.event().unwrap().to_canonical_bytes().unwrap();
    let hash = Digest::from_bytes([0x11; 32]);
    commit(&mut store, H, &ev_cli, hash).unwrap();

    let regs = Regs::new();
    let reg_store = RegisterStore::new();
    let reg_sources = regs.sources();
    let rr = ReplayRegisters { store: &reg_store, sources: &reg_sources };
    let rep = replay(&w.inputs(), &rr, &[(hash, H, ev_bytes.as_slice())]);
    assert_eq!(rep.outcome, ReplayOutcome::Identical);
    assert_eq!(rep.steps, 1);
}

/// AC-2: Given 3 points, command fails with `too_few_points` and journals nothing.
#[test]
fn ac2_three_points_fails_too_few_points_and_journals_nothing() {
    let mut w = World::new(&["flat-few"]);
    w.point_sets.insert("ps-3", vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]]).unwrap();
    let store = ResultStore::new();
    let err = invoke_cli(
        &store,
        &w.inputs(),
        H,
        &[EVAL_FLATNESS_CLI, "--run", "run-1", "--char", "flat-few", "--points", "ps-3", "--json"],
    )
    .expect_err("3 points must fail");
    assert_eq!(err.code(), "too_few_points");
}

/// AC-3: Characteristic with no governing spec fails with `governing_spec_missing` and journals nothing.
#[test]
fn ac3_missing_governing_spec_fails_and_journals_nothing() {
    let (mut spec, mut dprof) = load_project(&read("spec/no_spec.fidproj"), &test_vocab()).unwrap();
    spec.create_characteristic(&mut dprof, H, "plan-1", "flat-nogs", false).unwrap();
    let journal = format!(r#"{{"entries":[],"schema":"{JOURNAL_SCHEMA}"}}"#);
    let register = RegisterView::none(&JournalExtract::load(journal.as_bytes()).unwrap());
    let tbl = table();
    let mut point_sets = FormPointSets::new();
    let (pts, _, _, _) = load_flat_fixture();
    point_sets.insert("ps-flat-mz", pts).unwrap();

    let inp = EvalInputs::with_default_krn(&spec, &dprof, Some(&tbl), &register, &point_sets);
    let store = ResultStore::new();
    let err = invoke_cli(
        &store,
        &inp,
        H,
        &[EVAL_FLATNESS_CLI, "--run", "run-1", "--char", "flat-nogs", "--points", "ps-flat-mz", "--json"],
    )
    .expect_err("missing governing spec must fail");
    assert_eq!(err.code(), "governing_spec_missing");
}

/// AC-4: Given a KRN stub with MZ unavailable, the result falls back to LS and carries
/// `conservative_upper_bound`.
#[test]
fn ac4_mz_unavailable_falls_back_with_conservative_upper_bound() {
    let (pts, exp_width, _, _) = load_flat_fixture();
    let mut w = World::new(&["flat-cub"]);
    w.point_sets.insert("ps-flat-mz", pts).unwrap();
    let no_filter = NoFilterProvider;
    let stub = MzUnavailableKrn;
    let inp = EvalInputs {
        spec: &w.spec,
        dprof: &w.dprof,
        table: Some(&w.table),
        register: &w.register,
        point_sets: &w.point_sets,
        filters: &no_filter,
        krn: &stub,
    };
    let store = ResultStore::new();
    let ev = invoke_cli(
        &store,
        &inp,
        H,
        &[EVAL_FLATNESS_CLI, "--run", "run-1", "--char", "flat-cub", "--points", "ps-flat-mz", "--json"],
    )
    .unwrap();
    assert_eq!(ev.payload.association, FormAssociation::LeastSquares);
    assert!(ev.payload.flags.contains(&FormFlag::ConservativeUpperBound));
    assert!(ev.payload.value_si + 1e-15 >= exp_width, "LS residual range must bound MZ width from above");
}

/// REQ-GDT.FORM.FLAT-002: When the resolved chain contains a filtration step, the command runs
/// the filter provider and records its algorithm ID; when absent, none is applied.
#[test]
fn req002_optional_filter_runs_only_when_named_by_chain() {
    let mut raw = fid_canon::parse(&read("dmt/synthetic_table_v1.json")).unwrap();
    if let CValue::Obj(o) = &mut raw {
        if let Some(CValue::Arr(rows)) = o.get_mut("rows") {
            for r in rows.iter_mut() {
                if let CValue::Obj(ro) = r {
                    if ro.get("key") == Some(&CValue::Str("dmt:asme/flatness/none@1".to_owned())) {
                        if let Some(CValue::Arr(vars)) = ro.get_mut("variants") {
                            if let Some(CValue::Obj(v0)) = vars.first_mut() {
                                if let Some(CValue::Arr(steps)) = v0.get_mut("steps") {
                                    let mut fs = BTreeMap::new();
                                    fs.insert("alg".to_owned(), CValue::Str("krn.filter.gauss_plane@1.0.0".to_owned()));
                                    fs.insert("kind".to_owned(), CValue::Str("filtration".to_owned()));
                                    fs.insert("params".to_owned(), CValue::Obj(BTreeMap::new()));
                                    steps.insert(2, CValue::Obj(fs));
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    let filtered_table = DmtTable::from_value(&raw).unwrap();
    let (pts, exp_width, _, _) = load_flat_fixture();
    let mut w = World::new(&["flat-filt"]);
    w.table = filtered_table;
    w.point_sets.insert("ps-flat-mz", pts).unwrap();

    let filt = PassthroughFilter;
    let krn = FlatnessKrnProvider;
    let inp = EvalInputs {
        spec: &w.spec,
        dprof: &w.dprof,
        table: Some(&w.table),
        register: &w.register,
        point_sets: &w.point_sets,
        filters: &filt,
        krn: &krn,
    };
    let store = ResultStore::new();
    let ev = invoke_cli(
        &store,
        &inp,
        H,
        &[EVAL_FLATNESS_CLI, "--run", "run-1", "--char", "flat-filt", "--points", "ps-flat-mz", "--json"],
    )
    .unwrap();
    assert_eq!(ev.filter_algorithms, vec!["krn.filter.gauss_plane@1.0.0".to_owned()]);
    assert!((ev.payload.value_si - exp_width).abs() < TOL);
}
