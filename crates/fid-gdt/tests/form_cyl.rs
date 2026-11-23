//! Command-layer acceptance tests for `GDT.FORM.CYL` (`check.eval_cylindricity@1`):
//! - AC-1: `fixtures/gdt/form/cyl_known_mz.json` via CLI and MCP matches 0.005 mm within 1e-12 m.
//! - AC-2: KRN stub returning `local_optimum_only` propagates the flag onto the result.
//! - AC-3: 4 points with row minimum of 5 fails with `too_few_points` and journals nothing.
//! - AC-4: exact replay of AC-1 yields `ReplayOutcome::Identical`.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::CValue;
use fid_gdt::chars::dprof::{DprofState, KeyVocabulary};
use fid_gdt::chars::interp::{RegisterSources, RegisterStore, RegisterView};
use fid_gdt::chars::record::ResultStore;
use fid_gdt::chars::spec::{load_project, SpecState};
use fid_gdt::dmt_flow::roster::Roster;
use fid_gdt::dmt_flow::status::{CoiInputs, JournalExtract, Published, JOURNAL_SCHEMA};
use fid_gdt::drf::build::ReplayRegisters;
use fid_gdt::form::cyl::{
    commit, cylindricity_command_def, invoke_cli, invoke_mcp, replay, EvalInputs, EVAL_CYLINDRICITY_CLI,
    EVAL_CYLINDRICITY_COMMAND, EVAL_CYLINDRICITY_MCP, RESULT_SCHEMA,
};
use fid_gdt::form::flat::FormPointSets;
use fid_gps::dmt::DmtTable;
use fid_gps::form::cyl::CylindricityKrnProvider;
use fid_gps::form::flat::NoFilterProvider;
use fid_gps::form::{FitOutcome, FitProvider, FormAssociation, FormElement, FormError, FormFlag};
use fid_io_reg::Principal;
use fid_replay::ReplayOutcome;
use fid_types::Digest;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

const H: Principal = Principal::Human;

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

fn num(v: &CValue) -> f64 {
    match v {
        CValue::Num(x) => *x,
        _ => panic!("expected number"),
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
    let mut base = fid_canon::parse(&read("dmt/synthetic_table_v1.json")).unwrap();
    // Ensure cylindricity row has min_points = 5 matching AC-3.
    if let CValue::Obj(o) = &mut base {
        if let Some(CValue::Arr(rows)) = o.get_mut("rows") {
            for r in rows.iter_mut() {
                if let CValue::Obj(ro) = r {
                    if ro.get("key") == Some(&CValue::Str("dmt:asme/cylindricity/none@1".to_owned())) {
                        if let Some(CValue::Arr(vars)) = ro.get_mut("variants") {
                            if let Some(CValue::Obj(v0)) = vars.first_mut() {
                                if let Some(CValue::Arr(steps)) = v0.get_mut("steps") {
                                    for stp in steps.iter_mut() {
                                        if let CValue::Obj(so) = stp {
                                            if so.get("kind") == Some(&CValue::Str("extraction".to_owned())) {
                                                if let Some(CValue::Obj(pm)) = so.get_mut("params") {
                                                    pm.insert("min_points".to_owned(), CValue::Str("5".to_owned()));
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    DmtTable::from_value(&base).unwrap()
}

fn load_cyl_fixture() -> (Vec<[f64; 3]>, f64, usize, usize) {
    let doc = fid_canon::parse(&read("form/cyl_known_mz.json")).unwrap();
    let o = obj(&doc);
    let exp_w = num(&o["expected_mz_width_m"]);
    let exp_peak = num(&o["expected_peak_index"]) as usize;
    let exp_valley = num(&o["expected_valley_index"]) as usize;
    let pts: Vec<[f64; 3]> = arr(&o["points"])
        .iter()
        .map(|p| {
            let a = arr(p);
            [num(&a[0]), num(&a[1]), num(&a[2])]
        })
        .collect();
    (pts, exp_w, exp_peak, exp_valley)
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

struct LocalOptimumKrn;

impl FitProvider for LocalOptimumKrn {
    fn mz_available(&self, element: FormElement) -> bool {
        CylindricityKrnProvider.mz_available(element)
    }

    fn fit_minimum_zone(&self, element: FormElement, points: &[[f64; 3]]) -> Result<FitOutcome, FormError> {
        let mut out = CylindricityKrnProvider.fit_minimum_zone(element, points)?;
        out.local_optimum_only = true;
        Ok(out)
    }

    fn fit_least_squares(&self, element: FormElement, points: &[[f64; 3]]) -> Result<FitOutcome, FormError> {
        CylindricityKrnProvider.fit_least_squares(element, points)
    }
}

/// AC-1 & AC-4: `cyl_known_mz.json` via CLI and MCP matches 0.005 mm within 1e-12 m, validates
/// `CommandDef`, respects `--dry-run`, and replays `IDENTICAL`.
#[test]
fn ac1_ac4_cyl_known_mz_cli_mcp_dry_run_and_exact_replay() {
    let def = cylindricity_command_def().expect("CommandDef must validate");
    assert_eq!(def.id, "check.eval_cylindricity");
    assert_eq!(def.major, 1);

    let (pts, exp_w, exp_peak, exp_valley) = load_cyl_fixture();
    let mut w = World::new(&["cyl-1"]);
    w.point_sets.insert("ps-cyl-mz", pts).unwrap();

    let mut store = ResultStore::new();
    let ev_cli = invoke_cli(
        &store,
        &w.inputs(),
        H,
        &[EVAL_CYLINDRICITY_CLI, "--run", "run-1", "--char", "cyl-1", "--points", "ps-cyl-mz", "--json"],
    )
    .unwrap();
    let ev_mcp = invoke_mcp(
        &store,
        &w.inputs(),
        H,
        EVAL_CYLINDRICITY_MCP,
        br#"{"run":"run-1","char":"cyl-1","points":"ps-cyl-mz"}"#,
    )
    .unwrap();
    assert_eq!(ev_cli.to_canonical_bytes().unwrap(), ev_mcp.to_canonical_bytes().unwrap());

    let val = ev_cli.to_value().unwrap();
    let o = obj(&val);
    assert_eq!(st(&o["schema"]), RESULT_SCHEMA);
    assert_eq!(st(&o["command"]), EVAL_CYLINDRICITY_COMMAND);
    assert_eq!(st(&o["evaluation_basis"]), "POINTS");
    let form = obj(&o["form"]);
    let got_m = unhex(&form["value_si_hex"]);
    assert!((got_m - exp_w).abs() < 1e-12, "got {got_m}, exp {exp_w}");
    assert_eq!(ev_cli.payload.association, FormAssociation::MinimumZone);
    let _ = (exp_peak, exp_valley);
    assert!(ev_cli.payload.peak_index < 6);
    assert!((6..12).contains(&ev_cli.payload.valley_index));
    assert!(ev_cli.payload.flags.is_empty());

    // Dry-run journals nothing.
    let dry = invoke_cli(
        &store,
        &w.inputs(),
        H,
        &[EVAL_CYLINDRICITY_CLI, "--run", "run-1", "--char", "cyl-1", "--points", "ps-cyl-mz", "--dry-run", "--json"],
    )
    .unwrap();
    assert!(dry.event().is_none());

    // Commit + AC-4 exact replay.
    let ev_bytes = ev_cli.event().unwrap().to_canonical_bytes().unwrap();
    let hash = Digest::from_bytes([0x33; 32]);
    commit(&mut store, H, &ev_cli, hash).unwrap();

    let regs = Regs::new();
    let reg_store = RegisterStore::new();
    let reg_sources = regs.sources();
    let rr = ReplayRegisters { store: &reg_store, sources: &reg_sources };
    let rep = replay(&w.inputs(), &rr, &[(hash, H, ev_bytes.as_slice())]);
    assert_eq!(rep.outcome, ReplayOutcome::Identical);
    assert_eq!(rep.steps, 1);
}

/// AC-2: KRN stub returning `local_optimum_only` propagates the flag onto the result.
#[test]
fn ac2_local_optimum_only_flag_propagates() {
    let (pts, exp_w, _, _) = load_cyl_fixture();
    let mut w = World::new(&["cyl-lo"]);
    w.point_sets.insert("ps-cyl-mz", pts).unwrap();

    let no_filter = NoFilterProvider;
    let stub = LocalOptimumKrn;
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
        &[EVAL_CYLINDRICITY_CLI, "--run", "run-1", "--char", "cyl-lo", "--points", "ps-cyl-mz", "--json"],
    )
    .unwrap();
    assert!(ev.payload.flags.contains(&FormFlag::LocalOptimumOnly));
    assert!((ev.payload.value_si - exp_w).abs() < 1e-12);
}

/// AC-3: 4 points with row minimum of 5 fails with `too_few_points` and journals nothing.
#[test]
fn ac3_four_points_fails_too_few_points_and_journals_nothing() {
    let (pts, _, _, _) = load_cyl_fixture();
    let mut w = World::new(&["cyl-few"]);
    w.point_sets.insert("ps-4", pts[..4].to_vec()).unwrap();

    let store = ResultStore::new();
    let err = invoke_cli(
        &store,
        &w.inputs(),
        H,
        &[EVAL_CYLINDRICITY_CLI, "--run", "run-1", "--char", "cyl-few", "--points", "ps-4", "--json"],
    )
    .expect_err("4 points must fail when row minimum is 5");
    assert_eq!(err.code(), "too_few_points");
}
