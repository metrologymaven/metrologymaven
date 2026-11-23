//! Command-layer acceptance tests for `GDT.FORM.ROUND` (`check.eval_roundness@1`):
//! - AC-1: `fixtures/gdt/form/round_known_mz.json` via CLI and MCP reports 0.006 mm within 1e-12 m
//!   and lists all three sections.
//! - AC-2: `fixtures/gdt/form/round_from_cylinder_cloud.json` with chain naming z = 5, 10, 15 mm
//!   and band half-width 0.1 mm matches planted widths within 1e-9 m and records axis, positions, band.
//! - AC-3: section 1 with 2 points fails with `too_few_points{section: 1}` and journals nothing;
//!   missing section spec on 3D input fails with `section_spec_missing`.
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
use fid_gdt::form::flat::FormPointSets;
use fid_gdt::form::round::{
    commit, invoke_cli, invoke_mcp, replay, roundness_command_def, EvalInputs, FormSectionSets, EVAL_ROUNDNESS_CLI,
    EVAL_ROUNDNESS_COMMAND, EVAL_ROUNDNESS_MCP, RESULT_SCHEMA,
};
use fid_gps::dmt::DmtTable;
use fid_gps::form::round::SectionPoints;
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

fn table(with_3d_section_spec: bool) -> DmtTable {
    let mut base = fid_canon::parse(&read("dmt/synthetic_table_v1.json")).unwrap();
    if with_3d_section_spec {
        if let CValue::Obj(o) = &mut base {
            if let Some(CValue::Arr(rows)) = o.get_mut("rows") {
                for r in rows.iter_mut() {
                    if let CValue::Obj(ro) = r {
                        if ro.get("key") == Some(&CValue::Str("dmt:asme/circularity/none@1".to_owned())) {
                            if let Some(CValue::Arr(vars)) = ro.get_mut("variants") {
                                if let Some(CValue::Obj(v0)) = vars.first_mut() {
                                    if let Some(CValue::Arr(steps)) = v0.get_mut("steps") {
                                        for stp in steps.iter_mut() {
                                            if let CValue::Obj(so) = stp {
                                                if so.get("kind") == Some(&CValue::Str("extraction".to_owned())) {
                                                    if let Some(CValue::Obj(pm)) = so.get_mut("params") {
                                                        pm.insert(
                                                            "axis_fit".to_owned(),
                                                            CValue::Str("cylinder".to_owned()),
                                                        );
                                                        pm.insert(
                                                            "band_half_width_m".to_owned(),
                                                            CValue::Str(format!("0x{:016x}", 0.0001_f64.to_bits())),
                                                        );
                                                        pm.insert("min_points".to_owned(), CValue::Str("4".to_owned()));
                                                        pm.insert(
                                                            "section_0_m".to_owned(),
                                                            CValue::Str(format!("0x{:016x}", 0.005_f64.to_bits())),
                                                        );
                                                        pm.insert(
                                                            "section_1_m".to_owned(),
                                                            CValue::Str(format!("0x{:016x}", 0.010_f64.to_bits())),
                                                        );
                                                        pm.insert(
                                                            "section_2_m".to_owned(),
                                                            CValue::Str(format!("0x{:016x}", 0.015_f64.to_bits())),
                                                        );
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
    }
    DmtTable::from_value(&base).unwrap()
}

struct World {
    spec: SpecState,
    dprof: DprofState,
    table: DmtTable,
    section_sets: FormSectionSets,
    point_sets: FormPointSets,
    register: RegisterView,
}

impl World {
    fn new(chars: &[&str], with_3d_spec: bool) -> Self {
        let (mut spec, mut dprof) = load_project(&read("spec/plan_asme.fidproj"), &test_vocab()).unwrap();
        for c in chars {
            spec.create_characteristic(&mut dprof, H, "plan-1", c, false).unwrap();
        }
        let journal = format!(r#"{{"entries":[],"schema":"{JOURNAL_SCHEMA}"}}"#);
        let register = RegisterView::none(&JournalExtract::load(journal.as_bytes()).unwrap());
        Self {
            spec,
            dprof,
            table: table(with_3d_spec),
            section_sets: FormSectionSets::new(),
            point_sets: FormPointSets::new(),
            register,
        }
    }

    fn inputs(&self) -> EvalInputs<'_> {
        EvalInputs::with_default_krn(
            &self.spec,
            &self.dprof,
            Some(&self.table),
            &self.register,
            &self.section_sets,
            &self.point_sets,
        )
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

fn load_explicit_sections() -> (Vec<SectionPoints>, f64, Vec<f64>) {
    let doc = fid_canon::parse(&read("form/round_known_mz.json")).unwrap();
    let o = obj(&doc);
    let exp_worst = num(&o["expected_worst_width_m"]);
    let mut sections = Vec::new();
    let mut exp_widths = Vec::new();
    for s in arr(&o["sections"]) {
        let so = obj(s);
        exp_widths.push(num(&so["expected_mz_width_m"]));
        let pts: Vec<[f64; 2]> = arr(&so["points2d"])
            .iter()
            .map(|p| {
                let a = arr(p);
                [num(&a[0]), num(&a[1])]
            })
            .collect();
        sections.push(SectionPoints { position_m: num(&so["position_m"]), points2d: pts });
    }
    (sections, exp_worst, exp_widths)
}

/// AC-1 & AC-4: Explicit sections via CLI and MCP match 0.006 mm within 1e-12 m, list all 3
/// sections, validate `CommandDef`, respect `--dry-run`, and replay `IDENTICAL`.
#[test]
fn ac1_ac4_explicit_sections_cli_mcp_and_exact_replay() {
    let def = roundness_command_def().expect("CommandDef must validate");
    assert_eq!(def.id, "check.eval_roundness");
    assert_eq!(def.major, 1);

    let (sections, exp_worst, exp_widths) = load_explicit_sections();
    let mut w = World::new(&["rnd-1"], false);
    w.section_sets.insert("sec-3", sections).unwrap();

    let mut store = ResultStore::new();
    let ev_cli = invoke_cli(
        &store,
        &w.inputs(),
        H,
        &[EVAL_ROUNDNESS_CLI, "--run", "run-1", "--char", "rnd-1", "--sections", "sec-3", "--json"],
    )
    .unwrap();
    let ev_mcp =
        invoke_mcp(&store, &w.inputs(), H, EVAL_ROUNDNESS_MCP, br#"{"run":"run-1","char":"rnd-1","sections":"sec-3"}"#)
            .unwrap();
    assert_eq!(ev_cli.to_canonical_bytes().unwrap(), ev_mcp.to_canonical_bytes().unwrap());

    let val = ev_cli.to_value().unwrap();
    let o = obj(&val);
    assert_eq!(st(&o["schema"]), RESULT_SCHEMA);
    assert_eq!(st(&o["command"]), EVAL_ROUNDNESS_COMMAND);
    assert!((unhex(&o["value_si_hex"]) - exp_worst).abs() < 1e-12);
    assert_eq!(st(&o["worst_section_index"]), "1");
    let out_secs = arr(&o["sections"]);
    assert_eq!(out_secs.len(), 3);
    for (i, s) in out_secs.iter().enumerate() {
        assert!((unhex(&obj(s)["value_si_hex"]) - exp_widths[i]).abs() < 1e-12);
    }

    // Dry-run journals nothing.
    let dry = invoke_cli(
        &store,
        &w.inputs(),
        H,
        &[EVAL_ROUNDNESS_CLI, "--run", "run-1", "--char", "rnd-1", "--sections", "sec-3", "--dry-run", "--json"],
    )
    .unwrap();
    assert!(dry.event().is_none());

    // Commit + AC-4 exact replay.
    let ev_bytes = ev_cli.event().unwrap().to_canonical_bytes().unwrap();
    let hash = Digest::from_bytes([0x22; 32]);
    commit(&mut store, H, &ev_cli, hash).unwrap();

    let regs = Regs::new();
    let reg_store = RegisterStore::new();
    let reg_sources = regs.sources();
    let rr = ReplayRegisters { store: &reg_store, sources: &reg_sources };
    let rep = replay(&w.inputs(), &rr, &[(hash, H, ev_bytes.as_slice())]);
    assert_eq!(rep.outcome, ReplayOutcome::Identical);
    assert_eq!(rep.steps, 1);
}

/// AC-2: 3D cylinder cloud with chain naming z = 5, 10, 15 mm and band 0.1 mm matches planted
/// lobing within 1e-9 m and records axis, positions and band in output & replay.
#[test]
fn ac2_cylinder_cloud_extraction_via_cli_and_exact_replay() {
    let doc = fid_canon::parse(&read("form/round_from_cylinder_cloud.json")).unwrap();
    let o = obj(&doc);
    let exp_worst = num(&o["expected_worst_width_m"]);
    let exp_widths: Vec<f64> = arr(&o["expected_section_widths_m"]).iter().map(num).collect();
    let pts: Vec<[f64; 3]> = arr(&o["points"])
        .iter()
        .map(|p| {
            let a = arr(p);
            [num(&a[0]), num(&a[1]), num(&a[2])]
        })
        .collect();

    let mut w = World::new(&["rnd-3d"], true);
    w.point_sets.insert("cyl-cloud", pts).unwrap();

    let mut store = ResultStore::new();
    let ev = invoke_cli(
        &store,
        &w.inputs(),
        H,
        &[EVAL_ROUNDNESS_CLI, "--run", "run-1", "--char", "rnd-3d", "--points", "cyl-cloud", "--json"],
    )
    .unwrap();
    assert!((ev.result.value_si - exp_worst).abs() < 1e-9);
    for (i, s) in ev.result.sections.iter().enumerate() {
        assert!((s.payload.value_si - exp_widths[i]).abs() < 1e-9);
    }
    let val = ev.to_value().unwrap();
    let ext = obj(&obj(&val)["extracted"]);
    assert!((unhex(&ext["band_half_width_m_hex"]) - 0.0001).abs() < 1e-15);
    assert_eq!(arr(&ext["positions_m_hex"]).len(), 3);

    let ev_bytes = ev.event().unwrap().to_canonical_bytes().unwrap();
    let hash = Digest::from_bytes([0x23; 32]);
    commit(&mut store, H, &ev, hash).unwrap();

    let regs = Regs::new();
    let reg_store = RegisterStore::new();
    let reg_sources = regs.sources();
    let rr = ReplayRegisters { store: &reg_store, sources: &reg_sources };
    let rep = replay(&w.inputs(), &rr, &[(hash, H, ev_bytes.as_slice())]);
    assert_eq!(rep.outcome, ReplayOutcome::Identical);
    assert_eq!(rep.steps, 1);
}

/// AC-3: Section 1 with 2 points fails with `too_few_points{section: 1}` and journals nothing;
/// 3D cloud without section spec in chain fails with `section_spec_missing`.
#[test]
fn ac3_too_few_points_names_section_and_missing_spec_refused() {
    let (sections, _, _) = load_explicit_sections();
    let bad = vec![sections[0].clone(), SectionPoints { position_m: 0.010, points2d: vec![[0.01, 0.0], [-0.01, 0.0]] }];
    let mut w = World::new(&["rnd-few", "rnd-nospec"], false);
    w.section_sets.insert("sec-bad", bad).unwrap();
    w.point_sets.insert("pts-any", vec![[0.01, 0.0, 0.005]; 10]).unwrap();

    let store = ResultStore::new();
    let err = invoke_cli(
        &store,
        &w.inputs(),
        H,
        &[EVAL_ROUNDNESS_CLI, "--run", "run-1", "--char", "rnd-few", "--sections", "sec-bad", "--json"],
    )
    .expect_err("2-point section must fail");
    assert_eq!(err.code(), "too_few_points");
    assert!(err.to_string().contains("section: 1"), "unexpected error: {err}");

    let err_ns = invoke_cli(
        &store,
        &w.inputs(),
        H,
        &[EVAL_ROUNDNESS_CLI, "--run", "run-1", "--char", "rnd-nospec", "--points", "pts-any", "--json"],
    )
    .expect_err("3D input without section spec must fail");
    assert_eq!(err_ns.code(), "section_spec_missing");
}
