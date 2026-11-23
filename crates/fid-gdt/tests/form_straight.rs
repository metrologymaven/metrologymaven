//! Command-layer acceptance tests for `GDT.FORM.STRAIGHT.CMD` (`check.eval_straightness@1`):
//! - AC-1: `fixtures/gdt/form/straight_lines_planted.json` + synthetic row without median-line
//!   reduction -> dispatches to `line_elements`, value = 0.006 mm within 1e-12 m, `evaluation_basis: POINTS`.
//! - AC-2: synthetic row with median-line reduction + `median_line_bowed.json` -> dispatches to
//!   `derived_median_line`, value = 0.010 mm within 1e-12 m.
//! - AC-3: no governing spec -> `governing_spec_missing` and nothing journaled.
//! - AC-4: exact replay of AC-1 yields `ReplayOutcome::Identical`.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::{digest_bytes, CValue};
use fid_gdt::chars::dprof::{DprofState, KeyVocabulary};
use fid_gdt::chars::interp::{RegisterSources, RegisterStore, RegisterView};
use fid_gdt::chars::record::{EvaluationBasis, ResultStore};
use fid_gdt::chars::spec::{load_project, SpecState};
use fid_gdt::dmt_flow::roster::Roster;
use fid_gdt::dmt_flow::status::{CoiInputs, JournalExtract, Published, JOURNAL_SCHEMA};
use fid_gdt::drf::build::ReplayRegisters;
use fid_gdt::form::flat::FormPointSets;
use fid_gdt::form::straightness::{
    command_def, commit, eval_straightness, invoke_cli, invoke_mcp, replay, EvalInputs, EvalRequest,
    EVAL_STRAIGHTNESS_CLI, EVAL_STRAIGHTNESS_COMMAND, RESULT_SCHEMA,
};
use fid_gps::dmt::DmtTable;
use fid_gps::form::straightness::derived::{DerivedToleranceSpec, MaterialModifier, MZ_LINE3D_ALGORITHM_ID};
use fid_gps::form::straightness::StraightnessPath;
use fid_io_reg::Principal;
use fid_registry::schema::check_schema;
use fid_registry::validate::{validate, CommandDecl};
use fid_registry::Catalogue;
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

fn vec3(v: &CValue) -> [f64; 3] {
    let a = arr(v);
    [num(&a[0]), num(&a[1]), num(&a[2])]
}

fn f64_hex(x: f64) -> CValue {
    CValue::Str(format!("0x{:016x}", x.to_bits()))
}

fn test_vocab() -> KeyVocabulary {
    let v = fid_canon::parse(&read("dprof/vocabulary.json")).unwrap();
    let keys: Vec<&str> = arr(&obj(&v)["keys"]).iter().map(st).collect();
    KeyVocabulary::new(keys).unwrap()
}

fn load_lineel_fixture() -> (Vec<[f64; 3]>, BTreeMap<String, CValue>) {
    let doc = fid_canon::parse(&read("form/straight_lines_planted.json")).unwrap();
    let o = obj(&doc);
    let pts: Vec<[f64; 3]> = arr(&o["points"]).iter().map(vec3).collect();
    let pn = vec3(&o["plane_normal"]);
    let org = vec3(&o["origin"]);
    let ld = vec3(&o["line_direction"]);
    let sp = num(&o["spacing_m"]);
    let rs = num(&o["range_start"]) as i64;
    let re = num(&o["range_end"]) as i64;
    let bhw = num(&o["band_half_width_m"]);

    let mut ex = BTreeMap::new();
    ex.insert("min_points".to_owned(), CValue::Str("3".to_owned()));
    ex.insert("plane_normal_x".to_owned(), f64_hex(pn[0]));
    ex.insert("plane_normal_y".to_owned(), f64_hex(pn[1]));
    ex.insert("plane_normal_z".to_owned(), f64_hex(pn[2]));
    ex.insert("origin_x".to_owned(), f64_hex(org[0]));
    ex.insert("origin_y".to_owned(), f64_hex(org[1]));
    ex.insert("origin_z".to_owned(), f64_hex(org[2]));
    ex.insert("spacing".to_owned(), f64_hex(sp));
    ex.insert("range_start".to_owned(), CValue::Str(rs.to_string()));
    ex.insert("range_end".to_owned(), CValue::Str(re.to_string()));
    ex.insert("band_half_width".to_owned(), f64_hex(bhw));
    ex.insert("line_direction_x".to_owned(), f64_hex(ld[0]));
    ex.insert("line_direction_y".to_owned(), f64_hex(ld[1]));
    ex.insert("line_direction_z".to_owned(), f64_hex(ld[2]));
    (pts, ex)
}

fn load_median_line_points() -> Vec<[f64; 3]> {
    let doc = fid_canon::parse(&read("form/median_line_bowed.json")).unwrap();
    let o = obj(&doc);
    arr(&o["points"]).iter().map(vec3).collect()
}

fn make_table(with_median_line_reduction: bool, ext_params: BTreeMap<String, CValue>) -> DmtTable {
    let mut base = fid_canon::parse(&read("dmt/synthetic_table_v1.json")).unwrap();
    if let CValue::Obj(o) = &mut base {
        if let Some(CValue::Arr(rows)) = o.get_mut("rows") {
            for r in rows.iter_mut() {
                if let CValue::Obj(ro) = r {
                    if ro.get("key") == Some(&CValue::Str("dmt:asme/straightness/none@1".to_owned())) {
                        let steps = if with_median_line_reduction {
                            vec![
                                CValue::Obj(
                                    [
                                        ("alg".to_owned(), CValue::Str("loc.median_line.ls_sections@1.0.0".to_owned())),
                                        ("kind".to_owned(), CValue::Str("reduction".to_owned())),
                                        ("params".to_owned(), CValue::Obj(BTreeMap::new())),
                                    ]
                                    .into_iter()
                                    .collect(),
                                ),
                                CValue::Obj(
                                    [
                                        ("alg".to_owned(), CValue::Str(MZ_LINE3D_ALGORITHM_ID.to_owned())),
                                        ("kind".to_owned(), CValue::Str("association".to_owned())),
                                        ("params".to_owned(), CValue::Obj(BTreeMap::new())),
                                    ]
                                    .into_iter()
                                    .collect(),
                                ),
                            ]
                        } else {
                            vec![
                                CValue::Obj(
                                    [
                                        ("alg".to_owned(), CValue::Str("syn.extract.line_elements@0.1.0".to_owned())),
                                        ("kind".to_owned(), CValue::Str("extraction".to_owned())),
                                        ("params".to_owned(), CValue::Obj(ext_params.clone())),
                                    ]
                                    .into_iter()
                                    .collect(),
                                ),
                                CValue::Obj(
                                    [
                                        ("alg".to_owned(), CValue::Str("syn.assoc.min_zone@0.1.0".to_owned())),
                                        ("kind".to_owned(), CValue::Str("association".to_owned())),
                                        ("params".to_owned(), CValue::Obj(BTreeMap::new())),
                                    ]
                                    .into_iter()
                                    .collect(),
                                ),
                            ]
                        };
                        let variant = CValue::Obj(
                            [
                                ("steps".to_owned(), CValue::Arr(steps)),
                                ("variant".to_owned(), CValue::Str("default".to_owned())),
                            ]
                            .into_iter()
                            .collect(),
                        );
                        ro.insert("variants".to_owned(), CValue::Arr(vec![variant]));
                    }
                }
            }
        }
    }
    DmtTable::from_value(&base).unwrap()
}

struct World {
    spec: SpecState,
    dprof: DprofState,
    table: DmtTable,
    point_sets: FormPointSets,
    register: RegisterView,
}

impl World {
    fn new(proj_sub: &str, chars: &[&str], table: DmtTable) -> Self {
        let (mut spec, mut dprof) = load_project(&read(proj_sub), &test_vocab()).unwrap();
        for c in chars {
            spec.create_characteristic(&mut dprof, H, "plan-1", c, false).unwrap();
        }
        let journal = format!(r#"{{"entries":[],"schema":"{JOURNAL_SCHEMA}"}}"#);
        let register = RegisterView::none(&JournalExtract::load(journal.as_bytes()).unwrap());
        Self { spec, dprof, table, point_sets: FormPointSets::new(), register }
    }

    fn inputs(&self) -> EvalInputs<'_> {
        EvalInputs::with_defaults(&self.spec, &self.dprof, Some(&self.table), &self.register, &self.point_sets)
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

/// Validates `check.eval_straightness@1` registry definition against schema & validator.
#[test]
fn straightness_command_def_validates_cleanly() {
    let def = command_def().unwrap();
    assert_eq!(def.id, "check.eval_straightness");
    assert_eq!(def.major, 1);
    let catalogue = Catalogue::geo_frame().unwrap();
    assert_eq!(check_schema(&def.params_schema, &catalogue), vec![]);
    assert_eq!(check_schema(&def.result_schema, &catalogue), vec![]);
    assert_eq!(validate(&[CommandDecl::from_def(&def)]), vec![]);
}

/// AC-1 & AC-4: `straight_lines_planted.json` + synthetic row without median-line reduction ->
/// `line_elements`, value = 0.006 mm within 1e-12 m, `evaluation_basis: POINTS`, exact replay IDENTICAL.
#[test]
fn ac1_and_ac4_line_elements_dispatch_and_exact_replay() {
    let (pts, ext_params) = load_lineel_fixture();
    let table = make_table(false, ext_params);
    let mut w = World::new("spec/plan_asme.fidproj", &["char-str-1"], table);
    w.point_sets.insert("P_LINES", pts).unwrap();

    let inp = w.inputs();
    let mut store = ResultStore::new();
    let ev = invoke_cli(
        &store,
        &inp,
        H,
        &[EVAL_STRAIGHTNESS_CLI, "--run", "run-1", "--char", "char-str-1", "--points", "P_LINES", "--json"],
    )
    .unwrap();

    assert_eq!(ev.command, EVAL_STRAIGHTNESS_COMMAND);
    assert_eq!(ev.payload.path, StraightnessPath::LineElements);
    assert_eq!(ev.payload.determinism, "D0");
    assert!((ev.payload.value_si - 6.0e-6).abs() <= 1.0e-12, "expected 0.006 mm (6e-6 m), got {}", ev.payload.value_si);

    let val = ev.to_value().unwrap();
    let m = obj(&val);
    assert_eq!(st(&m["schema"]), RESULT_SCHEMA);
    assert_eq!(st(&m["path"]), "line_elements");
    assert_eq!(st(&m["evaluation_basis"]), "POINTS");

    let entry_hash = Digest::from_bytes(digest_bytes(b"ac1-straight-entry-1").0);
    let rec = ev.record(entry_hash).unwrap();
    assert_eq!(rec.evaluation_basis(), EvaluationBasis::Points);
    commit(&mut store, H, &ev, entry_hash).unwrap();

    // AC-4: exact replay gives IDENTICAL.
    let raw_event = ev.event().unwrap().to_canonical_bytes().unwrap();
    let regs_data = Regs::new();
    let reg_store = RegisterStore::new();
    let sources = regs_data.sources();
    let regs = ReplayRegisters { store: &reg_store, sources: &sources };
    let replayed = replay(&inp, &regs, &[(entry_hash, H, raw_event.as_slice())]);
    assert_eq!(replayed.outcome, ReplayOutcome::Identical);
    assert_eq!(replayed.steps, 1);

    // MCP dry-run invocation returns R1 with no journal event.
    let dry = invoke_mcp(
        &ResultStore::new(),
        &inp,
        H,
        br#"{"char":"char-str-1","dry_run":true,"points":"P_LINES","run":"run-2"}"#,
    )
    .unwrap();
    assert!(dry.event().is_none());
    assert_eq!(dry.payload.path, StraightnessPath::LineElements);
}

/// AC-2: synthetic row with median-line reduction + `median_line_bowed.json` -> `derived_median_line`,
/// value = 0.010 mm within 1e-12 m.
#[test]
fn ac2_derived_median_line_dispatch() {
    let table = make_table(true, BTreeMap::new());
    let mut w = World::new("spec/plan_asme.fidproj", &["char-str-2"], table);
    w.point_sets.insert("P_MEDIAN", load_median_line_points()).unwrap();

    let mut inp = w.inputs();
    inp.tolerance = DerivedToleranceSpec { stated_si: 1.0e-5, modifier: MaterialModifier::Rfs };
    let store = ResultStore::new();
    let req = EvalRequest { run: "run-2", char: "char-str-2", points: "P_MEDIAN", variant: None };

    let ev = eval_straightness(&store, &inp, H, &req, false).unwrap();
    assert_eq!(ev.payload.path, StraightnessPath::DerivedMedianLine);
    assert_eq!(ev.payload.determinism, "D3");
    assert!((ev.payload.value_si - 1.0e-5).abs() <= 1.0e-12, "expected 0.010 mm (1e-5 m), got {}", ev.payload.value_si);
    let val = ev.to_value().unwrap();
    assert_eq!(st(&obj(&val)["path"]), "derived_median_line");
}

/// AC-3: no governing spec -> `governing_spec_missing` and nothing journaled.
#[test]
fn ac3_no_governing_spec_fails_and_journals_nothing() {
    let (pts, ext_params) = load_lineel_fixture();
    let table = make_table(false, ext_params);
    let mut w = World::new("spec/no_spec.fidproj", &["char-str-3"], table);
    w.point_sets.insert("P_LINES", pts).unwrap();

    let inp = w.inputs();
    let store = ResultStore::new();
    let req = EvalRequest { run: "run-3", char: "char-str-3", points: "P_LINES", variant: None };

    let err = eval_straightness(&store, &inp, H, &req, false).unwrap_err();
    assert_eq!(err.code(), "governing_spec_missing");
}
