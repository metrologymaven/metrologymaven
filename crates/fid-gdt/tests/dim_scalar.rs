//! Acceptance tests for `GDT.DIM.SCALAR` through the command layer (`check.eval_scalar@1`
//! and `check.eval_minmax@1`): AC-1..AC-6.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::CValue;
use fid_gdt::chars::dprof::{DprofState, KeyVocabulary};
use fid_gdt::chars::interp::{RegisterSources, RegisterStore, RegisterView};
use fid_gdt::chars::record::ResultStore;
use fid_gdt::chars::spec::{load_project, SpecState};
use fid_gdt::dim::scalar::{
    commit, invoke_cli, invoke_mcp, minmax_command_def, replay, scalar_command_def, EvalInputs, ScalarStore,
    EVAL_MINMAX_CLI, EVAL_MINMAX_COMMAND, EVAL_MINMAX_MCP, EVAL_SCALAR_CLI, EVAL_SCALAR_COMMAND, EVAL_SCALAR_MCP,
    RESULT_SCHEMA,
};
use fid_gdt::dmt_flow::roster::Roster;
use fid_gdt::dmt_flow::status::{CoiInputs, JournalExtract, Published, JOURNAL_SCHEMA};
use fid_gdt::drf::build::ReplayRegisters;
use fid_gps::dim::scalar::{QuantityKind, ScalarElement};
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
    let sc = fid_canon::parse(&read("dim/scalar_table.json")).unwrap();
    if let CValue::Obj(o) = &mut base {
        if let Some(CValue::Arr(rows)) = o.get_mut("rows") {
            rows.extend(arr(&obj(&sc)["rows"]).iter().cloned());
        }
    }
    DmtTable::from_value(&base).unwrap()
}

struct World {
    spec: SpecState,
    dprof: DprofState,
    table: DmtTable,
    store: ScalarStore,
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
        World { spec, dprof, table: table(), store: ScalarStore::new(), register }
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
fn ac1_ac4_ac6_scalar_minmax_cli_mcp_and_exact_replay() {
    let props_doc = fid_canon::parse(&read("dim/scalar_props.json")).unwrap();
    let po = obj(&props_doc);
    let cyl_o = obj(&po["cylinder"]);
    let cone_o = obj(&po["cone"]);

    let mm_doc = fid_canon::parse(&read("dim/minmax_set.json")).unwrap();
    let mo = obj(&mm_doc);
    let vals: Vec<f64> = arr(&mo["values"]).iter().map(num).collect();

    let mut world = World::new(&["ch-cyl-len", "ch-cone-ang", "ch-mm-max", "ch-mm-range", "ch-dry"]);
    world
        .store
        .insert_element(
            "cyl-1",
            ScalarElement::Cylinder {
                length: num(&cyl_o["length"]),
                diameter: num(&cyl_o["diameter"]),
                radius: num(&cyl_o["radius"]),
            },
        )
        .unwrap();
    world
        .store
        .insert_element(
            "cone-1",
            ScalarElement::Cone {
                half_angle_rad: num(&cone_o["half_angle_rad"]),
                apex_angle_rad: num(&cone_o["apex_angle_rad"]),
                length: num(&cone_o["length"]),
            },
        )
        .unwrap();
    world.store.insert_value_set("devs-1000", QuantityKind::Length, vals).unwrap();

    let mut results = ResultStore::new();
    let mut journal: Vec<(Digest, Principal, Vec<u8>)> = Vec::new();

    // AC-1a: cylinder.length (40 mm).
    let ev_len = invoke_cli(
        &results,
        &world.inputs(),
        H,
        &[
            EVAL_SCALAR_CLI,
            "--run",
            "run-1",
            "--char",
            "ch-cyl-len",
            "--element",
            "cyl-1",
            "--property",
            "cylinder.length",
            "--unit",
            "mm",
            "--json",
        ],
    )
    .unwrap();
    let ev_len_mcp = invoke_mcp(
        &results,
        &world.inputs(),
        H,
        EVAL_SCALAR_MCP,
        br#"{"char":"ch-cyl-len","element":"cyl-1","property":"cylinder.length","run":"run-1","tolerance_unit":"mm"}"#,
    )
    .unwrap();
    assert_eq!(ev_len.to_canonical_bytes().unwrap(), ev_len_mcp.to_canonical_bytes().unwrap());
    let lv = ev_len.to_value();
    let lm = obj(&lv);
    assert_eq!(st(&lm["schema"]), RESULT_SCHEMA);
    assert_eq!(st(&lm["command"]), EVAL_SCALAR_COMMAND);
    assert_eq!(st(&lm["unit"]), "si:m");
    assert!((unhex(&lm["value_si"]) - 0.04).abs() <= TOL);
    let b1 = ev_len.event().unwrap().to_canonical_bytes().unwrap();
    let h1 = entry_digest("ch-cyl-len", &b1);
    commit(&mut results, H, &ev_len, h1).unwrap();
    journal.push((h1, H, b1));

    // AC-1b: cone.half_angle (15 deg).
    let ev_ang = invoke_cli(
        &results,
        &world.inputs(),
        H,
        &[
            EVAL_SCALAR_CLI,
            "--run",
            "run-1",
            "--char",
            "ch-cone-ang",
            "--element",
            "cone-1",
            "--property",
            "cone.half_angle",
            "--unit",
            "deg",
            "--json",
        ],
    )
    .unwrap();
    let av = ev_ang.to_value();
    let am = obj(&av);
    assert_eq!(st(&am["unit"]), "si:rad");
    assert_eq!(st(&am["quantity_kind"]), "angle");
    assert!((unhex(&am["value_si"]) - 15.0_f64.to_radians()).abs() <= TOL);
    let b2 = ev_ang.event().unwrap().to_canonical_bytes().unwrap();
    let h2 = entry_digest("ch-cone-ang", &b2);
    commit(&mut results, H, &ev_ang, h2).unwrap();
    journal.push((h2, H, b2));

    // AC-4: minmax on 1000 seeded deviations with two equal maxima (173 and 842 -> picks 173).
    let ev_mm = invoke_cli(
        &results,
        &world.inputs(),
        H,
        &[EVAL_MINMAX_CLI, "--run", "run-1", "--char", "ch-mm-max", "--values", "devs-1000", "--stat", "max", "--json"],
    )
    .unwrap();
    let ev_mm_mcp = invoke_mcp(
        &results,
        &world.inputs(),
        H,
        EVAL_MINMAX_MCP,
        br#"{"char":"ch-mm-max","run":"run-1","statistic":"max","values":"devs-1000"}"#,
    )
    .unwrap();
    assert_eq!(ev_mm.to_canonical_bytes().unwrap(), ev_mm_mcp.to_canonical_bytes().unwrap());
    let mv = ev_mm.to_value();
    let mm = obj(&mv);
    assert_eq!(st(&mm["command"]), EVAL_MINMAX_COMMAND);
    assert!((unhex(&mm["value_si"]) - num(&mo["expected_max_si"])).abs() <= TOL);
    let summary = obj(&mm["minmax"]);
    assert!((unhex(&summary["min_si"]) - num(&mo["expected_min_si"])).abs() <= TOL);
    assert!((unhex(&summary["max_si"]) - num(&mo["expected_max_si"])).abs() <= TOL);
    assert!((unhex(&summary["range_si"]) - num(&mo["expected_range_si"])).abs() <= TOL);
    assert_eq!(num(&summary["min_index"]) as u64, 519);
    assert_eq!(num(&summary["max_index"]) as u64, 173);
    assert_eq!(num(&obj(&mm["attained_at"])["index"]) as u64, 173);
    let b3 = ev_mm.event().unwrap().to_canonical_bytes().unwrap();
    let h3 = entry_digest("ch-mm-max", &b3);
    commit(&mut results, H, &ev_mm, h3).unwrap();
    journal.push((h3, H, b3));

    // Dry-run emits no event.
    let ev_dry = invoke_cli(
        &results,
        &world.inputs(),
        H,
        &[
            EVAL_MINMAX_CLI,
            "--run",
            "run-1",
            "--char",
            "ch-dry",
            "--values",
            "devs-1000",
            "--stat",
            "range",
            "--dry-run",
            "--json",
        ],
    )
    .unwrap();
    assert!(ev_dry.event().is_none());

    // AC-6: exact replay gives IDENTICAL.
    let reg_store = RegisterStore::new();
    let regs_data = Regs::new();
    let sources = regs_data.sources();
    let regs = ReplayRegisters { store: &reg_store, sources: &sources };
    let entries: Vec<(Digest, Principal, &[u8])> = journal.iter().map(|(d, p, b)| (*d, *p, b.as_slice())).collect();
    let rep = replay(&world.inputs(), &regs, &entries);
    assert_eq!(rep.outcome, ReplayOutcome::Identical);
    assert_eq!(rep.steps, 3);

    // CommandDefs validate cleanly.
    assert_eq!(scalar_command_def().unwrap().id, "check.eval_scalar");
    assert_eq!(minmax_command_def().unwrap().id, "check.eval_minmax");
}

#[test]
fn ac2_ac3_ac5_error_paths_journal_nothing() {
    let mut world = World::new(&["ch-unk", "ch-qkm", "ch-empty", "ch-nan"]);
    world
        .store
        .insert_element("cyl-1", ScalarElement::Cylinder { length: 0.04, diameter: 0.02, radius: 0.01 })
        .unwrap();
    world
        .store
        .insert_element(
            "cone-1",
            ScalarElement::Cone {
                half_angle_rad: 0.2617993877991494,
                apex_angle_rad: 0.5235987755982988,
                length: 0.03,
            },
        )
        .unwrap();
    world.store.insert_value_set("empty", QuantityKind::Length, Vec::new()).unwrap();
    world.store.insert_value_set("has-nan", QuantityKind::Length, vec![0.001, f64::NAN]).unwrap();
    let results = ResultStore::new();

    // AC-2: unknown_property.
    let e_unk = invoke_cli(
        &results,
        &world.inputs(),
        H,
        &[
            EVAL_SCALAR_CLI,
            "--run",
            "run-1",
            "--char",
            "ch-unk",
            "--element",
            "cyl-1",
            "--property",
            "cylinder.colour",
            "--json",
        ],
    )
    .unwrap_err();
    assert_eq!(e_unk.code(), "unknown_property");

    // AC-3: quantity_kind_mismatch (mm tolerance on cone.half_angle).
    let e_qkm = invoke_cli(
        &results,
        &world.inputs(),
        H,
        &[
            EVAL_SCALAR_CLI,
            "--run",
            "run-1",
            "--char",
            "ch-qkm",
            "--element",
            "cone-1",
            "--property",
            "cone.half_angle",
            "--unit",
            "mm",
            "--json",
        ],
    )
    .unwrap_err();
    assert_eq!(e_qkm.code(), "quantity_kind_mismatch");

    // AC-5: empty_value_set.
    let e_emp = invoke_cli(
        &results,
        &world.inputs(),
        H,
        &[EVAL_MINMAX_CLI, "--run", "run-1", "--char", "ch-empty", "--values", "empty", "--json"],
    )
    .unwrap_err();
    assert_eq!(e_emp.code(), "empty_value_set");

    // REQ-004: nan_value.
    let e_nan = invoke_cli(
        &results,
        &world.inputs(),
        H,
        &[EVAL_MINMAX_CLI, "--run", "run-1", "--char", "ch-nan", "--values", "has-nan", "--json"],
    )
    .unwrap_err();
    assert_eq!(e_nan.code(), "nan_value");
}
