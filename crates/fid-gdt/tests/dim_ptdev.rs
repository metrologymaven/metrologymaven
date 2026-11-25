//! Acceptance tests for `GDT.DIM.PTDEV` through the command layer (`check.eval_deviation@1`):
//! AC-1..AC-5. Expected values come from `fixtures/gdt/dim/make_ptdev.py` (independent Python
//! reference); none are computed here.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_bus::error::{BusError, Detail, ValidationReason};
use fid_bus::unit::Normalised;
use fid_bus::validate::{Compiled, Hooks, RefLookup};
use fid_canon::CValue;
use fid_gdt::chars::dprof::{DprofState, KeyVocabulary};
use fid_gdt::chars::interp::{RegisterSources, RegisterStore, RegisterView};
use fid_gdt::chars::record::ResultStore;
use fid_gdt::chars::spec::{load_project, SpecState};
use fid_gdt::dim::ptdev::{
    commit, deviation_command_def, invoke_cli, invoke_mcp, replay, EvalError, EvalInputs, PtDevStore,
    EVAL_DEVIATION_CLI, EVAL_DEVIATION_COMMAND, EVAL_DEVIATION_MCP, RESULT_SCHEMA,
};
use fid_gdt::dmt_flow::roster::Roster;
use fid_gdt::dmt_flow::status::{CoiInputs, JournalExtract, Published, JOURNAL_SCHEMA};
use fid_gdt::drf::build::ReplayRegisters;
use fid_gps::dim::ptdev::{DevFeature, NominalPoint, PtDevError, Rigid, COLUMNAR_FORMAT, SIGN_CONVENTION};
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

/// Fixture numbers are written as `repr` strings by `fixtures/gdt/dim/make_ptdev.py`.
fn fnum(v: &CValue) -> f64 {
    st(v).parse().unwrap()
}

fn vec3(v: &CValue) -> [f64; 3] {
    let a = arr(v);
    assert_eq!(a.len(), 3);
    [fnum(&a[0]), fnum(&a[1]), fnum(&a[2])]
}

fn unhex(v: &CValue) -> f64 {
    let s = st(v);
    assert!(s.starts_with("0x") && s.len() == 18, "{s}");
    f64::from_bits(u64::from_str_radix(&s[2..], 16).unwrap())
}

fn close(got: f64, want: f64, what: &str) {
    assert!((got - want).abs() <= TOL, "{what}: {got:e} vs {want:e}");
}

/// The fixture frame's frame-to-world rigid motion.
fn frame_rigid(v: &CValue) -> Rigid {
    let o = obj(v);
    let rows = arr(&o["r"]);
    Rigid::new([vec3(&rows[0]), vec3(&rows[1]), vec3(&rows[2])], vec3(&o["t"])).unwrap()
}

fn plane_inputs(o: &BTreeMap<String, CValue>) -> (Vec<[f64; 3]>, Vec<NominalPoint>) {
    let rows = arr(&o["points"]);
    let pts = rows.iter().map(|r| vec3(&obj(r)["point"])).collect();
    let noms = rows
        .iter()
        .map(|r| {
            let n = obj(&obj(r)["nominal"]);
            NominalPoint { point: vec3(&n["point"]), normal: vec3(&n["normal"]) }
        })
        .collect();
    (pts, noms)
}

fn cylinder(v: &CValue) -> DevFeature {
    let m = obj(v);
    DevFeature::Cylinder {
        axis_point: vec3(&m["axis_point"]),
        axis_dir: vec3(&m["axis_dir"]),
        radius: fnum(&m["radius"]),
    }
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

const PLANE_ARGS: [&str; 12] = [
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
];

/// A world holding the AC-1 plane points and the AC-2 cylinders, with `points` as the measured
/// point set stored under `plane-pts`.
fn plane_world(points: Option<Vec<[f64; 3]>>) -> World {
    let plane_doc = fid_canon::parse(&read("dim/ptdev_plane.json")).unwrap();
    let po = obj(&plane_doc);
    let (pts, noms) = plane_inputs(po);
    let cyl_doc = fid_canon::parse(&read("dim/ptdev_cylinder_feature.json")).unwrap();
    let co = obj(&cyl_doc);

    let mut world = World::new(&["ch-plane-pts", "ch-cyl-feat", "ch-dry"]);
    world.store.insert_frame(st(&obj(&po["frame"])["id"]), frame_rigid(&po["frame"])).unwrap();
    world.store.insert_frame("world", Rigid::IDENTITY).unwrap();
    world.store.insert_point_set("plane-pts", points.unwrap_or(pts)).unwrap();
    world.store.insert_nominal_set("plane-noms", noms).unwrap();
    world.store.insert_feature("cyl-actual", cylinder(&co["actual"])).unwrap();
    world.store.insert_feature("cyl-nominal", cylinder(&co["nominal"])).unwrap();
    world
}

#[test]
fn ac1_ac2_ac5_plane_points_cylinder_feature_and_exact_replay() {
    let plane_doc = fid_canon::parse(&read("dim/ptdev_plane.json")).unwrap();
    let po = obj(&plane_doc);
    let cyl_doc = fid_canon::parse(&read("dim/ptdev_cylinder_feature.json")).unwrap();
    let co = obj(&cyl_doc);
    let world = plane_world(None);

    let mut results = ResultStore::new();
    let mut journal: Vec<(Digest, Principal, Vec<u8>)> = Vec::new();

    // AC-1: plane points in the 30 degree rotated frame, CLI and MCP give the same bytes.
    let ev_pts = invoke_cli(&results, &world.inputs(), H, &PLANE_ARGS).unwrap();
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
    assert_eq!(st(&pm["evaluation_basis"]), "POINTS");
    assert_eq!(pm["attachment"], CValue::Null);
    let items = arr(&pm["items"]);
    let rows = arr(&po["points"]);
    assert_eq!(items.len(), 2);
    assert_eq!(rows.len(), 2);
    for (i, (item, row)) in items.iter().zip(rows).enumerate() {
        let got = obj(item);
        let row = obj(row);
        let want = obj(&row["expected"]);
        for k in ["dn", "dx", "dy", "dz"] {
            close(unhex(&got[k]), fnum(&want[k]), &format!("pt {i} {k}"));
        }
        // AC-1 names the planted offsets themselves: +0.05 mm and -0.02 mm.
        close(unhex(&got["dn"]), fnum(&row["planted_dn"]), &format!("pt {i} planted dn"));
    }
    let sum = obj(&pm["summary"]);
    let want_sum = obj(&po["expected_summary"]);
    for k in ["max_abs_dn_si", "max_dn_si", "min_dn_si"] {
        close(unhex(&sum[k]), fnum(&want_sum[k]), k);
    }
    close(unhex(&pm["value_si"]), fnum(&want_sum["max_abs_dn_si"]), "value_si");

    let b1 = ev_pts.event().unwrap().to_canonical_bytes().unwrap();
    let h1 = entry_digest("ch-plane-pts", &b1);
    commit(&mut results, H, &ev_pts, h1).unwrap();
    journal.push((h1, H, b1));

    // AC-2: cylinder feature versus nominal.
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
    assert_eq!(st(&feat["feature_kind"]), "cylinder");
    let loc = arr(&feat["location"]);
    for key in ["expected", "planted"] {
        let want = obj(&co[key]);
        let want_loc = vec3(&want["location_si"]);
        for (i, w) in want_loc.iter().enumerate() {
            close(unhex(&loc[i]), *w, &format!("{key} location[{i}]"));
        }
        close(unhex(&feat["direction_rad"]), fnum(&want["direction_rad"]), &format!("{key} direction"));
        close(unhex(&feat["size"]), fnum(&want["size_si"]), &format!("{key} size"));
    }

    let b2 = ev_cyl.event().unwrap().to_canonical_bytes().unwrap();
    let h2 = entry_digest("ch-cyl-feat", &b2);
    commit(&mut results, H, &ev_cyl, h2).unwrap();
    journal.push((h2, H, b2));

    // A dry run emits no event.
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
    assert!(ev_dry.record(Digest::TOMBSTONE).is_err());

    // AC-5: exact replay gives IDENTICAL.
    let reg_store = RegisterStore::new();
    let regs_data = Regs::new();
    let sources = regs_data.sources();
    let regs = ReplayRegisters { store: &reg_store, sources: &sources };
    let entries: Vec<(Digest, Principal, &[u8])> = journal.iter().map(|(d, p, b)| (*d, *p, b.as_slice())).collect();
    let rep = replay(&world.inputs(), &regs, &entries);
    assert_eq!(rep.outcome, ReplayOutcome::Identical);
    assert_eq!(rep.steps, 2);

    // Replay over a store whose measured points changed under the same ID is not IDENTICAL.
    let mut moved = plane_inputs(po).0;
    moved[0][2] += 1e-6;
    let changed = plane_world(Some(moved));
    let rep = replay(&changed.inputs(), &regs, &entries);
    assert_eq!(rep.outcome, ReplayOutcome::Divergent);
    assert_eq!(rep.steps, 0);

    // A truncated event does not decode, so it is not replayable.
    let truncated: Vec<(Digest, Principal, &[u8])> = vec![(h1, H, &journal[0].2[..journal[0].2.len() - 1])];
    assert_eq!(replay(&world.inputs(), &regs, &truncated).outcome, ReplayOutcome::NotReplayable);

    // The CommandDef validates cleanly.
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

    // AC-3: a nominal normal of length 1.1 fails with nominal_normal_not_unit naming index 1.
    // The command returns no evaluation, so there is no event to journal.
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
    assert_eq!(err, EvalError::PtDev(PtDevError::NominalNormalNotUnit { index: 1 }));
    assert_eq!(err.code(), "nominal_normal_not_unit");
    assert_eq!(err.to_string(), "nominal_normal_not_unit{1}");

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

    // Mixing the point and feature modes is refused before any evaluation.
    let err_mode = invoke_mcp(
        &results,
        &world.inputs(),
        H,
        EVAL_DEVIATION_MCP,
        br#"{"char":"ch-bad-norm","feature":"f","frame":"world","nominals":"bad-noms","points":"pts","run":"run-1"}"#,
    )
    .unwrap_err();
    assert_eq!(err_mode.code(), "bad_params");
    assert!(results.is_empty());
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

    // The payload is not inline JSON; the columnar attachment digest is in the record inputs.
    let att = ev.attachment().expect("1M points must emit columnar attachment");
    assert_eq!(att.format, COLUMNAR_FORMAT);
    assert_eq!(att.count, n);
    assert_eq!(att.bytes.len(), 9 + 8 + n * 32);
    assert_eq!(att.digest, Digest::from_bytes(fid_canon::digest_bytes(&att.bytes).0));

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

struct NoHooks;

impl Hooks for NoHooks {
    fn literal(&self, _: &str, _: &CValue) -> Result<Normalised, BusError> {
        Err(BusError::Validation(ValidationReason::UnitMismatch, Detail::new("no quantities here")))
    }
    fn entity(&self, _: &str) -> RefLookup {
        RefLookup::Missing
    }
}

#[test]
fn definition_and_results_conform_to_the_published_schemas() {
    let def = deviation_command_def().unwrap();
    let params = Compiled::compile(&def.params_schema).unwrap();
    let result = Compiled::compile(&def.result_schema).unwrap();
    let world = plane_world(None);
    let store = ResultStore::new();
    for raw in [
        r#"{"char":"ch-plane-pts","frame":"rot30_z","nominals":"plane-noms","points":"plane-pts","run":"run-1"}"#,
        r#"{"char":"ch-cyl-feat","feature":"cyl-actual","frame":"world","nominal":"cyl-nominal","run":"run-1"}"#,
        r#"{"char":"ch-dry","dry_run":true,"feature":"cyl-actual","frame":"world","nominal":"cyl-nominal","run":"r"}"#,
    ] {
        let p = fid_canon::parse(raw.as_bytes()).unwrap();
        assert!(params.evaluate(&p, &NoHooks).is_ok(), "{raw}");
        let ev = invoke_mcp(&store, &world.inputs(), H, EVAL_DEVIATION_MCP, raw.as_bytes()).unwrap();
        if let Err(f) = result.evaluate(&ev.to_value(), &NoHooks) {
            panic!("{raw}: {} at {} ({})", f.keyword, f.pointer, f.message);
        }
    }
    for bad in [
        r#"{"char":"ch-dry","frame":"world"}"#,
        r#"{"char":"ch-dry","frame":"world","run":"r","extra":true}"#,
        r#"{"char":"ch-dry","frame":"world","run":"r","dry_run":"yes"}"#,
    ] {
        assert!(params.evaluate(&fid_canon::parse(bad.as_bytes()).unwrap(), &NoHooks).is_err(), "{bad}");
    }
    // The result schema is not vacuous: another sign convention is refused.
    let ev = invoke_cli(&store, &world.inputs(), H, &PLANE_ARGS).unwrap();
    let mut v = ev.to_value();
    if let CValue::Obj(o) = &mut v {
        o.insert("sign_convention".to_owned(), CValue::Str("material_side_negative".to_owned()));
    }
    assert!(result.evaluate(&v, &NoHooks).is_err());
}

/// Evaluates `n` points, each 0.01 mm above its nominal, in the identity frame.
fn eval_n_points(n: usize) -> (World, fid_gdt::dim::ptdev::Evaluated) {
    let pts = vec![[0.0, 0.0, 0.00001]; n];
    let noms = vec![NominalPoint { point: [0.0, 0.0, 0.0], normal: [0.0, 0.0, 1.0] }; n];
    let mut world = World::new(&["ch-n"]);
    world.store.insert_frame("world", Rigid::IDENTITY).unwrap();
    world.store.insert_point_set("pts-n", pts).unwrap();
    world.store.insert_nominal_set("noms-n", noms).unwrap();
    let results = ResultStore::new();
    let args = [
        EVAL_DEVIATION_CLI,
        "--run",
        "run-1",
        "--char",
        "ch-n",
        "--points",
        "pts-n",
        "--nominals",
        "noms-n",
        "--frame",
        "world",
    ];
    let ev = invoke_cli(&results, &world.inputs(), H, &args).unwrap();
    (world, ev)
}

fn items_digest_of(ev: &fid_gdt::dim::ptdev::Evaluated) -> Digest {
    let v = ev.to_value();
    Digest::parse(st(&obj(&obj(&v)["summary"])["items_digest"])).unwrap()
}

#[test]
fn ac4_threshold_boundary_10000_inline_10001_attachment() {
    // At 10,000 points the items are inline; the items digest still binds them.
    let (_w, ev) = eval_n_points(10_000);
    assert!(ev.attachment().is_none());
    let v = ev.to_value();
    let m = obj(&v);
    assert_eq!(m["attachment"], CValue::Null);
    assert_eq!(arr(&m["items"]).len(), 10_000);
    let items = items_digest_of(&ev);
    let e = ev.event().unwrap().to_value();
    let eo = obj(&e);
    assert_eq!(eo["attachment_digest"], CValue::Null);
    assert_eq!(st(&eo["items_digest"]), items.as_str());
    let rec = ev.record(entry_digest("ch-n", &ev.event().unwrap().to_canonical_bytes().unwrap())).unwrap();
    assert!(rec.input_digests().contains(&items));

    // At 10,001 points the payload moves to the attachment, whose digest is the items digest.
    let (_w, ev) = eval_n_points(10_001);
    let att = ev.attachment().expect("10,001 points must emit the columnar attachment");
    assert_eq!(att.count, 10_001);
    let v = ev.to_value();
    let m = obj(&v);
    assert_eq!(m["items"], CValue::Null);
    assert_eq!(st(&obj(&m["attachment"])["digest"]), att.digest.as_str());
    assert_eq!(items_digest_of(&ev), att.digest);
    let e = ev.event().unwrap().to_value();
    let eo = obj(&e);
    assert_eq!(st(&eo["attachment_digest"]), att.digest.as_str());
    assert_eq!(st(&eo["items_digest"]), att.digest.as_str());
}

#[test]
fn ac5_replay_binds_items_not_only_the_summary() {
    // Same points and nominals; the frame stored under `rot30_z` differs. dn and therefore the
    // whole summary are frame-independent, but dx, dy, dz are not.
    let a = plane_world(None);
    let mut b = plane_world(None);
    b.store.insert_frame("rot30_z", Rigid::IDENTITY).unwrap();
    let results = ResultStore::new();
    let ev_a = invoke_cli(&results, &a.inputs(), H, &PLANE_ARGS).unwrap();
    let ev_b = invoke_cli(&results, &b.inputs(), H, &PLANE_ARGS).unwrap();
    let ea = ev_a.event().unwrap().to_value();
    let eb = ev_b.event().unwrap().to_value();
    let (oa, ob) = (obj(&ea), obj(&eb));
    for k in ["count", "max_abs_dn_si", "max_dn_si", "min_dn_si", "value_si", "attachment_digest"] {
        assert_eq!(oa[k], ob[k], "summary field {k} must not depend on the frame");
    }
    assert_ne!(oa["items_digest"], ob["items_digest"], "changed items must change the event");

    // Replay compares the items: an event whose items digest was swapped for B's diverges, even
    // though every summary field is unchanged.
    let reg_store = RegisterStore::new();
    let regs_data = Regs::new();
    let sources = regs_data.sources();
    let regs = ReplayRegisters { store: &reg_store, sources: &sources };
    let good = ev_a.event().unwrap().to_canonical_bytes().unwrap();
    let h = entry_digest("ch-plane-pts", &good);
    let ok: Vec<(Digest, Principal, &[u8])> = vec![(h, H, good.as_slice())];
    assert_eq!(replay(&a.inputs(), &regs, &ok).outcome, ReplayOutcome::Identical);
    let mut forged = ea.clone();
    if let CValue::Obj(o) = &mut forged {
        o.insert("items_digest".to_owned(), ob["items_digest"].clone());
    }
    let forged = fid_canon::to_canonical_bytes(&forged).unwrap();
    let bad: Vec<(Digest, Principal, &[u8])> = vec![(h, H, forged.as_slice())];
    assert_eq!(replay(&a.inputs(), &regs, &bad).outcome, ReplayOutcome::Divergent);
}
