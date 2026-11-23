//! Acceptance tests for `GDT.DIM.ANGLE` through the command layer (`check.eval_angle@1`):
//! AC-1 (analytic direction pairs at 0, 30, 45, 90, 135, 180 deg and 1e-9 rad via CLI and MCP
//! within 1e-12 rad in both acute and full modes), AC-2 (projected XY -30 deg about +Z),
//! AC-3 (`degenerate_direction` & `not_directed_element` journal nothing), AC-4 (bit-identical
//! outputs), AC-5 (exact replay `IDENTICAL`).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::CValue;
use fid_gdt::chars::dprof::{DprofState, KeyVocabulary};
use fid_gdt::chars::interp::{RegisterSources, RegisterStore, RegisterView};
use fid_gdt::chars::record::ResultStore;
use fid_gdt::chars::spec::{load_project, SpecState};
use fid_gdt::dim::angle::{
    angle_command_def, commit, invoke_cli, invoke_mcp, replay, AngleElements, EvalInputs, EVAL_ANGLE_CLI,
    EVAL_ANGLE_COMMAND, EVAL_ANGLE_MCP, RESULT_SCHEMA,
};
use fid_gdt::dmt_flow::roster::Roster;
use fid_gdt::dmt_flow::status::{CoiInputs, JournalExtract, Published, JOURNAL_SCHEMA};
use fid_gdt::drf::build::ReplayRegisters;
use fid_gps::dim::angle::ElementDirectionKind;
use fid_gps::dmt::DmtTable;
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

fn vec3(v: &CValue) -> [f64; 3] {
    let a = arr(v);
    let n = |x: &CValue| match x {
        CValue::Num(d) => *d,
        _ => panic!("expected number"),
    };
    [n(&a[0]), n(&a[1]), n(&a[2])]
}

fn test_vocab() -> KeyVocabulary {
    let v = fid_canon::parse(&read("dprof/vocabulary.json")).unwrap();
    let keys: Vec<&str> = arr(&obj(&v)["keys"]).iter().map(st).collect();
    KeyVocabulary::new(keys).unwrap()
}

fn table() -> DmtTable {
    let mut base = fid_canon::parse(&read("dmt/synthetic_table_v1.json")).unwrap();
    let angle = fid_canon::parse(&read("dim/angle_table.json")).unwrap();
    if let CValue::Obj(o) = &mut base {
        if let Some(CValue::Arr(rows)) = o.get_mut("rows") {
            rows.extend(arr(&obj(&angle)["rows"]).iter().cloned());
        }
    }
    DmtTable::from_value(&base).unwrap()
}

struct World {
    spec: SpecState,
    dprof: DprofState,
    table: DmtTable,
    elements: AngleElements,
    register: RegisterView,
}

impl World {
    fn new(chars: &[&str]) -> World {
        let (mut spec, mut dprof) = load_project(&read("spec/plan_asme.fidproj"), &test_vocab()).unwrap();
        for c in chars {
            spec.create_characteristic(&mut dprof, H, "plan-1", c, false).unwrap();
        }
        let journal = format!(r#"{{"entries":[],"schema":"{JOURNAL_SCHEMA}"}}"#);
        let register = RegisterView::none(&JournalExtract::load(journal.as_bytes()).unwrap());
        World { spec, dprof, table: table(), elements: AngleElements::new(), register }
    }

    fn inputs(&self) -> EvalInputs<'_> {
        EvalInputs {
            spec: &self.spec,
            dprof: &self.dprof,
            table: Some(&self.table),
            elements: &self.elements,
            register: &self.register,
        }
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
    fn new() -> Regs {
        Regs {
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

/// AC-1, AC-4 & AC-5: Analytic direction pairs through `fid check eval-angle --json` and MCP
/// `check_eval_angle` match within 1e-12 rad in both acute and full modes, produce byte-identical
/// CLI/MCP results, validate `CommandDef`, and replay `IDENTICAL`.
#[test]
fn ac1_ac4_ac5_analytic_pairs_cli_mcp_and_exact_replay() {
    let def = angle_command_def().expect("CommandDef must validate");
    assert_eq!(def.id, "check.eval_angle");
    assert_eq!(def.major, 1);

    let doc = fid_canon::parse(&read("dim/angle_analytic.json")).unwrap();
    let pairs = arr(&obj(&doc)["pairs"]);

    let mut char_ids: Vec<String> = Vec::new();
    for p in pairs {
        let pid = st(&obj(p)["id"]);
        char_ids.push(format!("ang-full-{pid}"));
        char_ids.push(format!("ang-acute-{pid}"));
    }
    let char_refs: Vec<&str> = char_ids.iter().map(String::as_str).collect();
    let mut w = World::new(&char_refs);

    for p in pairs {
        let po = obj(p);
        let pid = st(&po["id"]);
        w.elements.insert_directed(&format!("a-{pid}"), ElementDirectionKind::Line, vec3(&po["a"])).unwrap();
        w.elements.insert_directed(&format!("b-{pid}"), ElementDirectionKind::Axis, vec3(&po["b"])).unwrap();
    }

    let mut store = ResultStore::new();
    let mut journal_entries: Vec<(Digest, Principal, Vec<u8>)> = Vec::new();
    let mut entry_counter: u8 = 1;

    for p in pairs {
        let po = obj(p);
        let pid = st(&po["id"]);
        let exp_full: f64 = st(&po["expected_full_rad"]).parse().unwrap();
        let exp_acute: f64 = st(&po["expected_acute_rad"]).parse().unwrap();
        let a_id = format!("a-{pid}");
        let b_id = format!("b-{pid}");

        for (mode_str, exp) in [("full", exp_full), ("acute", exp_acute)] {
            let cid = format!("ang-{mode_str}-{pid}");
            let ev_cli = invoke_cli(
                &store,
                &w.inputs(),
                H,
                &[
                    EVAL_ANGLE_CLI,
                    "--run",
                    "run-1",
                    "--char",
                    &cid,
                    "--a",
                    &a_id,
                    "--b",
                    &b_id,
                    "--mode",
                    mode_str,
                    "--unit",
                    "rad",
                    "--json",
                ],
            )
            .unwrap();
            let mcp_json = format!(
                r#"{{"run":"run-1","char":"{cid}","a":"{a_id}","b":"{b_id}","mode":"{mode_str}","authored_unit":"rad"}}"#
            );
            let ev_mcp = invoke_mcp(&store, &w.inputs(), H, EVAL_ANGLE_MCP, mcp_json.as_bytes()).unwrap();

            // AC-4: byte-identical CLI and MCP canonical JSON outputs & events.
            assert_eq!(ev_cli.to_canonical_bytes().unwrap(), ev_mcp.to_canonical_bytes().unwrap());
            let cli_val = ev_cli.to_value();
            let co = obj(&cli_val);
            assert_eq!(st(&co["schema"]), RESULT_SCHEMA);
            assert_eq!(st(&co["command"]), EVAL_ANGLE_COMMAND);
            let got = unhex(&co["value_rad"]);
            assert!((got - exp).abs() < TOL, "{pid}/{mode_str}: got {got}, exp {exp}");

            let ev_bytes = ev_cli.event().unwrap().to_canonical_bytes().unwrap();
            let hash = Digest::from_bytes([entry_counter; 32]);
            entry_counter = entry_counter.wrapping_add(1);
            commit(&mut store, H, &ev_cli, hash).unwrap();
            journal_entries.push((hash, H, ev_bytes));
        }
    }

    // AC-5: exact replay of the whole AC-1 journal yields IDENTICAL.
    let regs = Regs::new();
    let reg_store = RegisterStore::new();
    let reg_sources = regs.sources();
    let rr = ReplayRegisters { store: &reg_store, sources: &reg_sources };
    let borrowed: Vec<(Digest, Principal, &[u8])> =
        journal_entries.iter().map(|(h, p, b)| (*h, *p, b.as_slice())).collect();
    let rep = replay(&w.inputs(), &rr, &borrowed);
    assert_eq!(rep.outcome, ReplayOutcome::Identical);
    assert_eq!(rep.steps, journal_entries.len());
}

/// AC-2: Pair projected onto the XY plane at -30 degrees about +Z reports -30 degrees (and
/// frame-axis operand `--b-axis +X` works cleanly).
#[test]
fn ac2_projected_xy_minus_30_deg_and_frame_axis_operand() {
    let mut w = World::new(&["ang-proj", "ang-axis"]);
    let doc = fid_canon::parse(&read("dim/angle_analytic.json")).unwrap();
    let case = obj(&arr(&obj(&doc)["projected_cases"])[0]);
    w.elements.insert_directed("line-a", ElementDirectionKind::Line, vec3(&case["a"])).unwrap();
    w.elements.insert_directed("line-b", ElementDirectionKind::Line, vec3(&case["b"])).unwrap();
    w.elements.insert_directed("plane-xy", ElementDirectionKind::PlaneNormal, vec3(&case["plane_normal"])).unwrap();

    let store = ResultStore::new();
    let ev = invoke_cli(
        &store,
        &w.inputs(),
        H,
        &[
            EVAL_ANGLE_CLI,
            "--run",
            "run-1",
            "--char",
            "ang-proj",
            "--a",
            "line-a",
            "--b",
            "line-b",
            "--plane",
            "plane-xy",
            "--mode",
            "full",
            "--unit",
            "deg",
            "--json",
        ],
    )
    .unwrap();
    let v = ev.to_value();
    let o = obj(&v);
    assert_eq!(o["projected"], CValue::Bool(true));
    assert_eq!(st(&o["authored_unit"]), "deg");
    assert!((unhex(&o["value_rad"]) - (-core::f64::consts::FRAC_PI_6)).abs() < TOL);
    assert!((unhex(&o["authored_value"]) - (-30.0)).abs() < 1e-10);

    // Frame axis operand (`--b-axis +X`) + `--dry-run` emits no event.
    let ev_ax = invoke_cli(
        &store,
        &w.inputs(),
        H,
        &[
            EVAL_ANGLE_CLI,
            "--run",
            "run-1",
            "--char",
            "ang-axis",
            "--a",
            "line-b",
            "--b-axis",
            "+X",
            "--plane",
            "plane-xy",
            "--unit",
            "deg",
            "--dry-run",
            "--json",
        ],
    )
    .unwrap();
    assert!(ev_ax.event().is_none());
    assert!((unhex(&obj(&ev_ax.to_value())["authored_value"]) - 30.0).abs() < 1e-10);
}

/// AC-3: A line parallel to the projection plane normal fails with `degenerate_direction` and
/// journals nothing; an undirected element fails with `not_directed_element` and journals nothing.
#[test]
fn ac3_degenerate_and_undirected_elements_fail_without_journaling() {
    let mut w = World::new(&["ang-deg", "ang-undir"]);
    w.elements.insert_directed("line-z", ElementDirectionKind::Line, [0.0, 0.0, 1.0]).unwrap();
    w.elements.insert_directed("line-x", ElementDirectionKind::Line, [1.0, 0.0, 0.0]).unwrap();
    w.elements.insert_directed("plane-xy", ElementDirectionKind::PlaneNormal, [0.0, 0.0, 1.0]).unwrap();
    w.elements.insert_undirected("pt-1").unwrap();

    let store = ResultStore::new();
    let deg_err = invoke_cli(
        &store,
        &w.inputs(),
        H,
        &[
            EVAL_ANGLE_CLI,
            "--run",
            "run-1",
            "--char",
            "ang-deg",
            "--a",
            "line-z",
            "--b",
            "line-x",
            "--plane",
            "plane-xy",
            "--json",
        ],
    )
    .expect_err("line parallel to normal must fail");
    assert_eq!(deg_err.code(), "degenerate_direction");

    let undir_err = invoke_cli(
        &store,
        &w.inputs(),
        H,
        &[EVAL_ANGLE_CLI, "--run", "run-1", "--char", "ang-undir", "--a", "pt-1", "--b", "line-x", "--json"],
    )
    .expect_err("undirected element must fail");
    assert_eq!(undir_err.code(), "not_directed_element");
}
