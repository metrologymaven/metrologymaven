//! Acceptance tests for `GDT.DIM.PTDEV` through the command layer (`check.eval_deviation@1`):
//! AC-1..AC-5.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::CValue;
use fid_gdt::chars::dprof::{DprofState, KeyVocabulary};
use fid_gdt::chars::interp::{RegisterSources, RegisterStore, RegisterView};
use fid_gdt::chars::record::ResultStore;
use fid_gdt::chars::spec::{load_project, SpecState};
use fid_gdt::dim::ptdev::{
    commit, deviation_command_def, invoke_cli, invoke_mcp, replay, EvalInputs, PtDevStore, EVAL_DEVIATION_CLI,
    EVAL_DEVIATION_COMMAND, EVAL_DEVIATION_MCP, RESULT_SCHEMA,
};
use fid_gdt::dmt_flow::roster::Roster;
use fid_gdt::dmt_flow::status::{CoiInputs, JournalExtract, Published, JOURNAL_SCHEMA};
use fid_gdt::drf::build::ReplayRegisters;
use fid_gps::dim::ptdev::{DevFeature, NominalPoint, Rigid, COLUMNAR_FORMAT, SIGN_CONVENTION};
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

fn rot_z(deg: f64) -> Rigid {
    let rad = deg.to_radians();
    let c = rad.cos();
    let s = rad.sin();
    Rigid::new([[c, -s, 0.0], [s, c, 0.0], [0.0, 0.0, 1.0]], [0.0, 0.0, 0.0]).unwrap()
}

fn test_vocab() -> KeyVocabulary {
    let v = fid_canon::parse(&read("dprof/vocabulary.json")).unwrap();
    let keys: Vec<&str> = arr(&obj(&v)["keys"]).iter().map(st).collect();
    KeyVocabulary::new(keys).unwrap()
}

fn table() -> DmtTable {
    let mut base = fid_canon::parse(&read("dmt/synthetic_table_v1.json")).unwrap();
    let pt = fid_canon::parse(&read("dim/ptdev_table.json")).unwrap();
    if let CValue::Obj(o) = &mut base {
        if let Some(CValue::Arr(rows)) = o.get_mut("rows") {
            rows.extend(arr(&obj(&pt)["rows"]).iter().cloned());
        }
    }
    DmtTable::from_value(&base).unwrap()
}

struct World {
    spec: SpecState,
    dprof: DprofState,
    table: DmtTable,
    store: PtDevStore,
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
        World { spec, dprof, table: table(), store: PtDevStore::new(), register }
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
fn ac1_ac2_ac5_plane_points_cylinder_feature_and_exact_replay() {
    let plane_doc = fid_canon::parse(&read("dim/ptdev_plane.json")).unwrap();
    let po = obj(&plane_doc);
    let deg = num(&po["frame_rotation_z_deg"]);
    let pts: Vec<[f64; 3]> = arr(&po["points"]).iter().map(vec3).collect();
    let noms: Vec<NominalPoint> = arr(&po["nominals"])
        .iter()
        .map(|n| {
            let no = obj(n);
            NominalPoint { point: vec3(&no["point"]), normal: vec3(&no["normal"]) }
        })
        .collect();

    let cyl_doc = fid_canon::parse(&read("dim/ptdev_cylinder_feature.json")).unwrap();
    let co = obj(&cyl_doc);
    let parse_cyl = |v: &CValue| {
        let m = obj(v);
        DevFeature::Cylinder {
            axis_point: vec3(&m["axis_point"]),
            axis_dir: vec3(&m["axis_dir"]),
            radius: num(&m["radius"]),
        }
    };

    let mut world = World::new(&["ch-plane-pts", "ch-cyl-feat", "ch-dry"]);
    world.store.insert_frame("rot30_z", rot_z(deg)).unwrap();
    world.store.insert_frame("world", Rigid::IDENTITY).unwrap();
    world.store.insert_point_set("plane-pts", pts).unwrap();
    world.store.insert_nominal_set("plane-noms", noms).unwrap();
    world.store.insert_feature("cyl-actual", parse_cyl(&co["actual"])).unwrap();
    world.store.insert_feature("cyl-nominal", parse_cyl(&co["nominal"])).unwrap();

    let mut results = ResultStore::new();
    let mut journal: Vec<(Digest, Principal, Vec<u8>)> = Vec::new();

    // AC-1: plane points in 30 deg rotated frame + CLI/MCP parity.
    let ev_pts = invoke_cli(
        &results,
        &world.inputs(),
        H,
        &[
            EVAL_DEVIATION_CLI,
            "--run",
            "run-1",
            "--char",
            "ch-plane-pts",
            "--points",
            "plane-pts",
            "--nominals",
            "plane-noms",
            "--frame",
            "rot30_z",
            "--json",
        ],
    )
    .unwrap();
    let ev_pts_mcp = invoke_mcp(
        &results,
        &world.inputs(),
        H,
        EVAL_DEVIATION_MCP,
        br#"{"char":"ch-plane-pts","frame":"rot30_z","nominals":"plane-noms","points":"plane-pts","run":"run-1"}"#,
    )
    .unwrap();
    assert_eq!(ev_pts.to_canonical_bytes().unwrap(), ev_pts_mcp.to_canonical_bytes().unwrap());

    let pv = ev_pts.to_value();
    let pm = obj(&pv);
    assert_eq!(st(&pm["schema"]), RESULT_SCHEMA);
    assert_eq!(st(&pm["command"]), EVAL_DEVIATION_COMMAND);
    assert_eq!(st(&pm["sign_convention"]), SIGN_CONVENTION);
    assert_eq!(st(&pm["frame"]), "rot30_z");
    assert_eq!(pm["attachment"], CValue::Null);
    let items = arr(&pm["items"]);
    assert_eq!(items.len(), 2);
    let c = deg.to_radians().cos();
    let s = deg.to_radians().sin();
    let i0 = obj(&items[0]);
    assert!((unhex(&i0["dn"]) - 0.00005).abs() <= TOL);
    assert!((unhex(&i0["dx"]) - 0.00005 * c).abs() <= TOL);
    assert!((unhex(&i0["dy"]) - (-0.00005 * s)).abs() <= TOL);
    let i1 = obj(&items[1]);
    assert!((unhex(&i1["dn"]) - (-0.00002)).abs() <= TOL);

    let b1 = ev_pts.event().unwrap().to_canonical_bytes().unwrap();
    let h1 = entry_digest("ch-plane-pts", &b1);
    commit(&mut results, H, &ev_pts, h1).unwrap();
    journal.push((h1, H, b1));

    // AC-2: cylinder feature vs nominal.
    let ev_cyl = invoke_cli(
        &results,
        &world.inputs(),
        H,
        &[
            EVAL_DEVIATION_CLI,
            "--run",
            "run-1",
            "--char",
            "ch-cyl-feat",
            "--feature",
            "cyl-actual",
            "--nominal",
            "cyl-nominal",
            "--frame",
            "world",
            "--json",
        ],
    )
    .unwrap();
    let fv = ev_cyl.to_value();
    let fm = obj(&fv);
    assert_eq!(st(&fm["evaluation_basis"]), "FEATURES");
    let feat = obj(&fm["feature"]);
    let loc = arr(&feat["location"]);
    assert!((unhex(&loc[0]) - 0.00001).abs() <= TOL);
    assert!(unhex(&loc[1]).abs() <= TOL);
    assert!(unhex(&loc[2]).abs() <= TOL);
    assert!((unhex(&feat["direction_rad"]) - 0.0001).abs() <= TOL);
    assert!((unhex(&feat["size"]) - 0.000003).abs() <= TOL);

    let b2 = ev_cyl.event().unwrap().to_canonical_bytes().unwrap();
    let h2 = entry_digest("ch-cyl-feat", &b2);
    commit(&mut results, H, &ev_cyl, h2).unwrap();
    journal.push((h2, H, b2));

    // Dry-run emits no event.
    let ev_dry = invoke_cli(
        &results,
        &world.inputs(),
        H,
        &[
            EVAL_DEVIATION_CLI,
            "--run",
            "run-1",
            "--char",
            "ch-dry",
            "--feature",
            "cyl-actual",
            "--nominal",
            "cyl-nominal",
            "--frame",
            "world",
            "--dry-run",
            "--json",
        ],
    )
    .unwrap();
    assert!(ev_dry.event().is_none());

    // AC-5: exact replay gives IDENTICAL.
    let reg_store = RegisterStore::new();
    let regs_data = Regs::new();
    let sources = regs_data.sources();
    let regs = ReplayRegisters { store: &reg_store, sources: &sources };
    let entries: Vec<(Digest, Principal, &[u8])> = journal.iter().map(|(d, p, b)| (*d, *p, b.as_slice())).collect();
    let rep = replay(&world.inputs(), &regs, &entries);
    assert_eq!(rep.outcome, ReplayOutcome::Identical);
    assert_eq!(rep.steps, 2);

    // CommandDef validates cleanly.
    assert_eq!(deviation_command_def().unwrap().id, "check.eval_deviation");
}

#[test]
fn ac3_non_unit_normal_and_unknown_frame_journal_nothing() {
    let mut world = World::new(&["ch-bad-norm", "ch-bad-frame"]);
    world.store.insert_frame("world", Rigid::IDENTITY).unwrap();
    world.store.insert_point_set("pts", vec![[0.0, 0.0, 0.0], [0.001, 0.0, 0.0]]).unwrap();
    world
        .store
        .insert_nominal_set(
            "bad-noms",
            vec![
                NominalPoint { point: [0.0, 0.0, 0.0], normal: [0.0, 0.0, 1.0] },
                NominalPoint { point: [0.0, 0.0, 0.0], normal: [1.1, 0.0, 0.0] },
            ],
        )
        .unwrap();
    let results = ResultStore::new();

    // AC-3: nominal normal of length 1.1 -> nominal_normal_not_unit naming point index 1.
    let err = invoke_cli(
        &results,
        &world.inputs(),
        H,
        &[
            EVAL_DEVIATION_CLI,
            "--run",
            "run-1",
            "--char",
            "ch-bad-norm",
            "--points",
            "pts",
            "--nominals",
            "bad-noms",
            "--frame",
            "world",
            "--json",
        ],
    )
    .unwrap_err();
    assert_eq!(err.code(), "nominal_normal_not_unit");
    assert!(err.to_string().contains('1'), "error must name index 1: {err}");

    let err_frame = invoke_cli(
        &results,
        &world.inputs(),
        H,
        &[
            EVAL_DEVIATION_CLI,
            "--run",
            "run-1",
            "--char",
            "ch-bad-frame",
            "--points",
            "pts",
            "--nominals",
            "bad-noms",
            "--frame",
            "missing-frame",
        ],
    )
    .unwrap_err();
    assert_eq!(err_frame.code(), "frame_unknown");
}

#[test]
fn ac4_one_million_points_uses_columnar_attachment_in_record_inputs() {
    let n = 1_000_000usize;
    let pts = vec![[0.0, 0.0, 0.00001]; n];
    let noms = vec![NominalPoint { point: [0.0, 0.0, 0.0], normal: [0.0, 0.0, 1.0] }; n];

    let mut world = World::new(&["ch-1m"]);
    world.store.insert_frame("world", Rigid::IDENTITY).unwrap();
    world.store.insert_point_set("pts-1m", pts).unwrap();
    world.store.insert_nominal_set("noms-1m", noms).unwrap();

    let mut results = ResultStore::new();
    let ev = invoke_cli(
        &results,
        &world.inputs(),
        H,
        &[
            EVAL_DEVIATION_CLI,
            "--run",
            "run-1",
            "--char",
            "ch-1m",
            "--points",
            "pts-1m",
            "--nominals",
            "noms-1m",
            "--frame",
            "world",
            "--json",
        ],
    )
    .unwrap();

    // Payload is NOT inline JSON; columnar attachment digest is in ResultRecord inputs.
    let att = ev.attachment().expect("1M points must emit columnar attachment");
    assert_eq!(att.format, COLUMNAR_FORMAT);
    assert_eq!(att.count, n);
    assert_eq!(att.bytes.len(), 9 + 8 + n * 32);

    let v = ev.to_value();
    let m = obj(&v);
    assert_eq!(m["items"], CValue::Null);
    let att_json = obj(&m["attachment"]);
    assert_eq!(st(&att_json["digest"]), att.digest.as_str());
    assert_eq!(num(&att_json["count"]) as usize, n);

    let event_bytes = ev.event().unwrap().to_canonical_bytes().unwrap();
    let entry_hash = entry_digest("ch-1m", &event_bytes);
    let rec = ev.record(entry_hash).unwrap();
    assert!(
        rec.input_digests().contains(&att.digest),
        "ResultRecord input_digests must contain columnar attachment digest"
    );
    commit(&mut results, H, &ev, entry_hash).unwrap();
}
