//! Acceptance tests for `GDT.DIM.LINEAR` through the command layer (`check.eval_distance@1`
//! and `check.eval_caliper@1`): AC-1..AC-7.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::CValue;
use fid_gdt::chars::dprof::{DprofState, KeyVocabulary};
use fid_gdt::chars::interp::{RegisterSources, RegisterStore, RegisterView};
use fid_gdt::chars::record::ResultStore;
use fid_gdt::chars::spec::{load_project, SpecState};
use fid_gdt::dim::linear::{
    caliper_command_def, commit, distance_command_def, invoke_cli, invoke_mcp, replay, EvalInputs, LinearStore,
    EVAL_CALIPER_CLI, EVAL_CALIPER_COMMAND, EVAL_CALIPER_MCP, EVAL_DISTANCE_CLI, EVAL_DISTANCE_COMMAND,
    EVAL_DISTANCE_MCP, RESULT_SCHEMA,
};
use fid_gdt::dmt_flow::roster::Roster;
use fid_gdt::dmt_flow::status::{CoiInputs, JournalExtract, Published, JOURNAL_SCHEMA};
use fid_gdt::drf::build::ReplayRegisters;
use fid_gps::dim::linear::LinearElement;
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

fn num(v: &CValue) -> f64 {
    match v {
        CValue::Num(n) => *n,
        _ => panic!("expected number"),
    }
}

fn vec3(v: &CValue) -> [f64; 3] {
    let a = arr(v);
    [num(&a[0]), num(&a[1]), num(&a[2])]
}

fn unhex(v: &CValue) -> f64 {
    let s = st(v);
    assert!(s.starts_with("0x") && s.len() == 18, "{s}");
    f64::from_bits(u64::from_str_radix(&s[2..], 16).unwrap())
}

fn parse_element(v: &CValue) -> LinearElement {
    let o = obj(v);
    match st(&o["kind"]) {
        "point" => LinearElement::Point {
            position: vec3(&o["position"]),
            raw_point: matches!(o.get("raw_point"), Some(CValue::Bool(true))),
        },
        "line" => LinearElement::Line { point: vec3(&o["point"]), direction: vec3(&o["direction"]) },
        "plane" => LinearElement::Plane { centroid: vec3(&o["centroid"]), normal: vec3(&o["normal"]) },
        other => panic!("unexpected element kind: {other}"),
    }
}

fn test_vocab() -> KeyVocabulary {
    let v = fid_canon::parse(&read("dprof/vocabulary.json")).unwrap();
    let keys: Vec<&str> = arr(&obj(&v)["keys"]).iter().map(st).collect();
    KeyVocabulary::new(keys).unwrap()
}

fn table() -> DmtTable {
    let mut base = fid_canon::parse(&read("dmt/synthetic_table_v1.json")).unwrap();
    let lin = fid_canon::parse(&read("dim/linear_table.json")).unwrap();
    if let CValue::Obj(o) = &mut base {
        if let Some(CValue::Arr(rows)) = o.get_mut("rows") {
            rows.extend(arr(&obj(&lin)["rows"]).iter().cloned());
        }
    }
    DmtTable::from_value(&base).unwrap()
}

struct World {
    spec: SpecState,
    dprof: DprofState,
    table: DmtTable,
    store: LinearStore,
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
        World { spec, dprof, table: table(), store: LinearStore::new(), register }
    }

    fn inputs(&self) -> EvalInputs<'_> {
        EvalInputs {
            spec: &self.spec,
            dprof: &self.dprof,
            table: Some(&self.table),
            store: &self.store,
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

fn entry_digest(tag: &str, payload: &[u8]) -> Digest {
    let mut buf = Vec::with_capacity(tag.len() + 1 + payload.len());
    buf.extend_from_slice(tag.as_bytes());
    buf.push(b'\n');
    buf.extend_from_slice(payload);
    Digest::from_bytes(fid_canon::digest_bytes(&buf).0)
}

#[test]
fn ac1_ac2_ac6_ac7_analytic_distance_component_dryrun_and_replay() {
    let cases_doc = fid_canon::parse(&read("dim/linear_analytic.json")).unwrap();
    let exp_doc = fid_canon::parse(&read("dim/expected/linear_analytic.json")).unwrap();
    let expected = obj(&obj(&exp_doc)["cases"]);
    let cases = arr(&obj(&cases_doc)["cases"]);

    let mut char_names: Vec<String> = cases.iter().map(|c| format!("ch-{}", st(&obj(c)["name"]))).collect();
    char_names.push("ch-plane-comp-x".to_owned());
    char_names.push("ch-dry-run".to_owned());
    let char_refs: Vec<&str> = char_names.iter().map(String::as_str).collect();

    let mut world = World::new(&char_refs);
    world.store.insert_frame("part", [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]).unwrap();
    for case in cases {
        let o = obj(case);
        let name = st(&o["name"]);
        world.store.insert_element(&format!("{name}-a"), parse_element(&o["a"])).unwrap();
        world.store.insert_element(&format!("{name}-b"), parse_element(&o["b"])).unwrap();
    }

    let mut results = ResultStore::new();
    let mut journal: Vec<(Digest, Principal, Vec<u8>)> = Vec::new();

    // AC-1: every analytic fixture via CLI + bit-identical MCP parity.
    for case in cases {
        let o = obj(case);
        let name = st(&o["name"]);
        let ch = format!("ch-{name}");
        let a = format!("{name}-a");
        let b = format!("{name}-b");
        let unit = st(&o["unit"]);
        let e = obj(&expected[name]);

        let ev_cli = invoke_cli(
            &results,
            &world.inputs(),
            H,
            &[
                EVAL_DISTANCE_CLI,
                "--run",
                "run-1",
                "--char",
                &ch,
                "--a",
                &a,
                "--b",
                &b,
                "--measurand",
                "3d",
                "--unit",
                unit,
                "--json",
            ],
        )
        .unwrap();

        let mcp_json = format!(
            r#"{{"authored_unit":"{unit}","char":"{ch}","elements":["{a}","{b}"],"measurand":{{"kind":"3d"}},"run":"run-1"}}"#
        );
        let ev_mcp = invoke_mcp(&results, &world.inputs(), H, EVAL_DISTANCE_MCP, mcp_json.as_bytes()).unwrap();
        assert_eq!(ev_cli.to_canonical_bytes().unwrap(), ev_mcp.to_canonical_bytes().unwrap());

        let v = ev_cli.to_value();
        let m = obj(&v);
        assert_eq!(st(&m["schema"]), RESULT_SCHEMA);
        assert_eq!(st(&m["command"]), EVAL_DISTANCE_COMMAND);
        assert_eq!(st(&m["evaluation_basis"]), st(&e["basis"]));
        assert!((unhex(&m["value_si"]) - num(&e["value_si"])).abs() <= TOL, "{name}");
        assert!((unhex(&m["authored_value"]) - num(&e["value_mm"])).abs() <= 1e-9, "{name}");
        let dec: f64 = st(&m["authored_decimal"]).parse().unwrap();
        assert!((dec - num(&e["value_mm"])).abs() <= 1e-6, "{name}");

        let event_bytes = ev_cli.event().unwrap().to_canonical_bytes().unwrap();
        let entry_hash = entry_digest(&ch, &event_bytes);
        commit(&mut results, H, &ev_cli, entry_hash).unwrap();
        journal.push((entry_hash, H, event_bytes));
    }

    // AC-2: same plane pair with measurand component{frame: part, axis: x}.
    let ev_comp = invoke_cli(
        &results,
        &world.inputs(),
        H,
        &[
            EVAL_DISTANCE_CLI,
            "--run",
            "run-1",
            "--char",
            "ch-plane-comp-x",
            "--a",
            "parallel_planes_10mm-a",
            "--b",
            "parallel_planes_10mm-b",
            "--measurand",
            "component:part:x",
            "--unit",
            "mm",
            "--json",
        ],
    )
    .unwrap();
    let mv = ev_comp.to_value();
    let mo = obj(&mv);
    assert!((unhex(&mo["value_si"]) - 0.01).abs() <= TOL);
    assert_eq!(st(&mo["authored_decimal"]), "10.000000");
    let comp_bytes = ev_comp.event().unwrap().to_canonical_bytes().unwrap();
    let comp_hash = entry_digest("ch-plane-comp-x", &comp_bytes);
    commit(&mut results, H, &ev_comp, comp_hash).unwrap();
    journal.push((comp_hash, H, comp_bytes));

    // AC-6: --dry-run returns scratch result and leaves journal & store unchanged.
    let before_len = journal.len();
    let ev_dry = invoke_cli(
        &results,
        &world.inputs(),
        H,
        &[
            EVAL_DISTANCE_CLI,
            "--run",
            "run-1",
            "--char",
            "ch-dry-run",
            "--a",
            "parallel_planes_10mm-a",
            "--b",
            "parallel_planes_10mm-b",
            "--measurand",
            "3d",
            "--dry-run",
            "--json",
        ],
    )
    .unwrap();
    assert!(ev_dry.dry_run);
    assert!(ev_dry.event().is_none());
    assert!(ev_dry.record(Digest::TOMBSTONE).is_err());
    assert_eq!(journal.len(), before_len);
    assert!(results
        .get(
            H,
            &fid_gdt::chars::record::RunId::parse("run-1").unwrap(),
            &fid_gdt::chars::dprof::CharId::parse("ch-dry-run").unwrap()
        )
        .is_err());

    // AC-7: exact replay returns IDENTICAL.
    let reg_store = RegisterStore::new();
    let regs_data = Regs::new();
    let sources = regs_data.sources();
    let regs = ReplayRegisters { store: &reg_store, sources: &sources };
    let entries: Vec<(Digest, Principal, &[u8])> = journal.iter().map(|(d, p, b)| (*d, *p, b.as_slice())).collect();
    let rep = replay(&world.inputs(), &regs, &entries);
    assert_eq!(rep.outcome, ReplayOutcome::Identical);
    assert_eq!(rep.steps, journal.len());
}

#[test]
fn ac3_missing_and_unknown_measurand_journal_nothing() {
    let mut world = World::new(&["ch-missing", "ch-unknown-plane", "ch-unknown-frame", "ch-unknown-axis"]);
    world.store.insert_element("p1", LinearElement::Point { position: [0.0, 0.0, 0.0], raw_point: true }).unwrap();
    world.store.insert_element("p2", LinearElement::Point { position: [0.01, 0.0, 0.0], raw_point: true }).unwrap();
    world.store.insert_frame("part", [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]).unwrap();
    let results = ResultStore::new();

    // AC-3: missing measurand -> measurand_missing.
    let err = invoke_cli(
        &results,
        &world.inputs(),
        H,
        &[EVAL_DISTANCE_CLI, "--run", "run-1", "--char", "ch-missing", "--a", "p1", "--b", "p2", "--json"],
    )
    .unwrap_err();
    assert_eq!(err.code(), "measurand_missing");

    // MCP missing measurand -> measurand_missing.
    let err_mcp = invoke_mcp(
        &results,
        &world.inputs(),
        H,
        EVAL_DISTANCE_MCP,
        br#"{"char":"ch-missing","elements":["p1","p2"],"run":"run-1"}"#,
    )
    .unwrap_err();
    assert_eq!(err_mcp.code(), "measurand_missing");

    // Unknown plane / frame / axis -> measurand_unknown.
    let err_plane = invoke_cli(
        &results,
        &world.inputs(),
        H,
        &[
            EVAL_DISTANCE_CLI,
            "--run",
            "run-1",
            "--char",
            "ch-unknown-plane",
            "--a",
            "p1",
            "--b",
            "p2",
            "--measurand",
            "projected:no_such_plane",
        ],
    )
    .unwrap_err();
    assert_eq!(err_plane.code(), "measurand_unknown");

    let err_frame = invoke_cli(
        &results,
        &world.inputs(),
        H,
        &[
            EVAL_DISTANCE_CLI,
            "--run",
            "run-1",
            "--char",
            "ch-unknown-frame",
            "--a",
            "p1",
            "--b",
            "p2",
            "--measurand",
            "component:no_such_frame:x",
        ],
    )
    .unwrap_err();
    assert_eq!(err_frame.code(), "measurand_unknown");

    let err_axis = invoke_cli(
        &results,
        &world.inputs(),
        H,
        &[
            EVAL_DISTANCE_CLI,
            "--run",
            "run-1",
            "--char",
            "ch-unknown-axis",
            "--a",
            "p1",
            "--b",
            "p2",
            "--measurand",
            "component:part:q",
        ],
    )
    .unwrap_err();
    assert_eq!(err_axis.code(), "measurand_unknown");
}

#[test]
fn ac4_ac5_caliper_and_non_parallel_planes_plus_command_defs() {
    let mut world = World::new(&["ch-cal", "ch-nonpar"]);
    let cal_doc = fid_canon::parse(&read("dim/caliper_box.json")).unwrap();
    let co = obj(&cal_doc);
    let pts: Vec<[f64; 3]> = arr(&co["points"]).iter().map(vec3).collect();
    world.store.insert_point_set("box8", pts).unwrap();

    let alpha = 0.01_f64;
    world
        .store
        .insert_element("pl1", LinearElement::Plane { centroid: [0.0, 0.0, 0.0], normal: [0.0, 0.0, 1.0] })
        .unwrap();
    world
        .store
        .insert_element(
            "pl2",
            LinearElement::Plane { centroid: [0.0, 0.0, 0.01], normal: [alpha.sin(), 0.0, alpha.cos()] },
        )
        .unwrap();

    let mut results = ResultStore::new();
    let mut journal: Vec<(Digest, Principal, Vec<u8>)> = Vec::new();

    // AC-4: caliper on 20x10x5 mm box along x.
    let ev_cal = invoke_cli(
        &results,
        &world.inputs(),
        H,
        &[
            EVAL_CALIPER_CLI,
            "--run",
            "run-1",
            "--char",
            "ch-cal",
            "--points",
            "box8",
            "--dir",
            "1,0,0",
            "--unit",
            "mm",
            "--json",
        ],
    )
    .unwrap();
    let ev_cal_mcp = invoke_mcp(
        &results,
        &world.inputs(),
        H,
        EVAL_CALIPER_MCP,
        br#"{"authored_unit":"mm","char":"ch-cal","direction":[1,0,0],"points":"box8","run":"run-1"}"#,
    )
    .unwrap();
    assert_eq!(ev_cal.to_canonical_bytes().unwrap(), ev_cal_mcp.to_canonical_bytes().unwrap());

    let cv = ev_cal.to_value();
    let cm = obj(&cv);
    assert_eq!(st(&cm["command"]), EVAL_CALIPER_COMMAND);
    assert_eq!(st(&cm["evaluation_basis"]), "POINTS");
    assert!((unhex(&cm["value_si"]) - 0.02).abs() <= TOL);
    assert_eq!(st(&cm["authored_decimal"]), "20.000000");
    let ext = arr(&cm["extreme_indices"]);
    assert_eq!(num(&ext[0]) as u64, 0);
    assert_eq!(num(&ext[1]) as u64, 4);

    let cal_bytes = ev_cal.event().unwrap().to_canonical_bytes().unwrap();
    let cal_hash = entry_digest("ch-cal", &cal_bytes);
    commit(&mut results, H, &ev_cal, cal_hash).unwrap();
    journal.push((cal_hash, H, cal_bytes));

    // AC-5: planes 0.01 rad apart carry non_parallel_planes flag with angle 0.01 rad.
    let ev_np = invoke_cli(
        &results,
        &world.inputs(),
        H,
        &[
            EVAL_DISTANCE_CLI,
            "--run",
            "run-1",
            "--char",
            "ch-nonpar",
            "--a",
            "pl1",
            "--b",
            "pl2",
            "--measurand",
            "3d",
            "--json",
        ],
    )
    .unwrap();
    let nv = ev_np.to_value();
    let nm = obj(&nv);
    let flags = arr(&nm["flags"]);
    assert_eq!(flags.len(), 1);
    let fl = obj(&flags[0]);
    assert_eq!(st(&fl["code"]), "non_parallel_planes");
    assert!((unhex(&fl["angle_rad"]) - 0.01).abs() <= 1e-12);

    let np_bytes = ev_np.event().unwrap().to_canonical_bytes().unwrap();
    let np_hash = entry_digest("ch-nonpar", &np_bytes);
    commit(&mut results, H, &ev_np, np_hash).unwrap();
    journal.push((np_hash, H, np_bytes));

    // Exact replay over both caliper and non-parallel distance events.
    let reg_store = RegisterStore::new();
    let regs_data = Regs::new();
    let sources = regs_data.sources();
    let regs = ReplayRegisters { store: &reg_store, sources: &sources };
    let entries: Vec<(Digest, Principal, &[u8])> = journal.iter().map(|(d, p, b)| (*d, *p, b.as_slice())).collect();
    let rep = replay(&world.inputs(), &regs, &entries);
    assert_eq!(rep.outcome, ReplayOutcome::Identical);
    assert_eq!(rep.steps, 2);

    // CommandDefs validate cleanly.
    let d_def = distance_command_def().unwrap();
    assert_eq!(d_def.id, "check.eval_distance");
    let c_def = caliper_command_def().unwrap();
    assert_eq!(c_def.id, "check.eval_caliper");
}
