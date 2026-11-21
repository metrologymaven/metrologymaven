//! Acceptance criteria of spec node GDT.DRF.BUILD through the command layer
//! (`gdt.build_drf@1`, `gdt.get_drf@1`): AC-1 (A|B|C from labels), AC-3 (parallel pair
//! refused, nothing journaled), AC-5 (get returns the canonical bytes unchanged), AC-7 (exact
//! replay), the P1a gates (cylindrical datums and MMB/LMB refused, typed), the D-045 label and
//! interpretation block, and the seeded faults (wrong datum precedence, wrong material-condition
//! handling). Frames and refusals are checked against the independent exact reference
//! `fixtures/gdt/drf/measured/make_cases.py` (`cases.json`); error paths are hand-labelled from
//! the leaf's rules. Synthetic data only.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_bus::error::{BusError, Detail, ValidationReason};
use fid_bus::unit::Normalised;
use fid_bus::validate::{Compiled, Hooks, RefLookup};
use fid_canon::CValue;
use fid_gdt::chars::dprof::{DprofState, KeyVocabulary};
use fid_gdt::chars::interp::{RegisterSources, RegisterStore, RegisterView, PROVISIONAL_LABEL_TEXT};
use fid_gdt::chars::spec::{load_project, SpecState};
use fid_gdt::dmt_flow::roster::Roster;
use fid_gdt::dmt_flow::status::{CoiInputs, JournalExtract, Published, Purpose, JOURNAL_SCHEMA};
use fid_gdt::drf::build::{
    build_command_def, build_drf, drf_label, get_command_def, get_drf, invoke_cli, invoke_mcp, replay, BuildError,
    BuildInputs, BuildRequest, Built, DatumRef, DrfEvent, DrfStore, FeaturePoints, ReplayRegisters, BUILD_COMMAND,
    BUILD_DRF_CLI, BUILD_DRF_MCP, DRF_SCHEMA, GET_COMMAND, GET_DRF_CLI, GET_DRF_MCP,
};
use fid_gdt::drf::datum::{DatumModel, DefineRequest, LabelPattern, TargetRequest};
use fid_gdt::requires_provisional_label_bytes;
use fid_gps::dmt::DmtTable;
use fid_io_reg::Principal;
use fid_replay::ReplayOutcome;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

const H: Principal = Principal::Human;
const A: Principal = Principal::Agent;
const CHAR: &str = "drf-1";
/// AC-1: origin and axes within 1e-12 m and 1e-12 rad of the reference.
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
        _ => panic!("object"),
    }
}

fn arr(v: &CValue) -> &[CValue] {
    match v {
        CValue::Arr(a) => a,
        _ => panic!("array"),
    }
}

fn st(v: &CValue) -> &str {
    match v {
        CValue::Str(s) => s,
        _ => panic!("string"),
    }
}

fn dec(v: &CValue) -> f64 {
    st(v).parse::<f64>().unwrap()
}

fn unhex(v: &CValue) -> f64 {
    let s = st(v);
    assert!(s.starts_with("0x") && s.len() == 18, "{s}");
    f64::from_bits(u64::from_str_radix(&s[2..], 16).unwrap())
}

fn hex3(v: &CValue) -> [f64; 3] {
    let a = arr(v);
    [unhex(&a[0]), unhex(&a[1]), unhex(&a[2])]
}

fn dec3(v: &CValue) -> [f64; 3] {
    let a = arr(v);
    [dec(&a[0]), dec(&a[1]), dec(&a[2])]
}

fn close(a: [f64; 3], b: [f64; 3]) -> bool {
    (0..3).all(|i| (a[i] - b[i]).abs() <= TOL)
}

fn names(v: &CValue) -> Vec<String> {
    arr(v).iter().map(|x| st(x).to_owned()).collect()
}

/// The reference cases, by name.
fn cases() -> BTreeMap<String, CValue> {
    let doc = fid_canon::parse(&read("drf/measured/cases.json")).unwrap();
    arr(&obj(&doc)["cases"]).iter().map(|c| (st(&obj(c)["name"]).to_owned(), c.clone())).collect()
}

/// Datum label of the command tests -> (fixture face label, plan feature). `E` is the bottom
/// face `A2` (parallel to `A`), `K` a cylindrical datum feature.
const DATUMS: [(&str, &str, &str); 5] =
    [("A", "A", "fa"), ("B", "B", "fb"), ("C", "C", "fc"), ("E", "A2", "fe"), ("D", "D", "fd")];

fn test_vocab() -> KeyVocabulary {
    let v = fid_canon::parse(&read("dprof/vocabulary.json")).unwrap();
    let keys: Vec<&str> = arr(&obj(&v)["keys"]).iter().map(st).collect();
    KeyVocabulary::new(keys).unwrap()
}

struct World {
    spec: SpecState,
    dprof: DprofState,
    table: DmtTable,
    datums: DatumModel,
    points: FeaturePoints,
    register: RegisterView,
}

/// The synthetic DMT table of `GDT.RES.SCHEMA` (whose rows the plan's defaults profile names)
/// with the synthetic `datum_frame/rmb` row of `frame_table_<name>.json` added (`none`: no
/// frame row).
fn table(name: &str) -> DmtTable {
    let mut base = fid_canon::parse(&read("dmt/synthetic_table_v1.json")).unwrap();
    if name == "none" {
        return DmtTable::from_value(&base).unwrap();
    }
    let frame = fid_canon::parse(&read(&format!("drf/measured/frame_table_{name}.json"))).unwrap();
    let row = arr(&obj(&frame)["rows"])[0].clone();
    if let CValue::Obj(o) = &mut base {
        if let Some(CValue::Arr(rows)) = o.get_mut("rows") {
            rows.push(row);
        }
    }
    DmtTable::from_value(&base).unwrap()
}

fn world(table_name: &str) -> World {
    let (mut spec, mut dprof) = load_project(&read("spec/plan_asme.fidproj"), &test_vocab()).unwrap();
    spec.create_characteristic(&mut dprof, H, "plan-1", CHAR, false).unwrap();
    let mut datums = DatumModel::new("plan-1").unwrap();
    let pat = LabelPattern::parse("[A-Z]{1,3}").unwrap();
    let mut points = FeaturePoints::new();
    // Every face of every case, keyed by fixture label.
    let mut faces: BTreeMap<String, (Vec<f64>, [f64; 3])> = BTreeMap::new();
    for c in cases().values() {
        for d in arr(&obj(c)["datums"]) {
            let d = obj(d);
            let xyz: Vec<f64> = arr(&d["xyz"]).iter().map(dec).collect();
            faces.insert(st(&d["label"]).to_owned(), (xyz, dec3(&d["nominal"])));
        }
    }
    for (i, (label, face, feature)) in DATUMS.iter().enumerate() {
        let face_id = format!("face:{}", i + 1);
        datums.add_face(&face_id).unwrap();
        let face_ids = [face_id.as_str()];
        datums.add_feature(feature, "plane", &face_ids).unwrap();
        let req =
            DefineRequest { label, feature, faces: &face_ids, feature_type: "plane", targets: &[], replace: false };
        datums.define_datum(H, &pat, &req, false).unwrap();
        let (xyz, nominal) = faces[*face].clone();
        points.insert(feature, xyz, nominal).unwrap();
    }
    datums.add_face("face:9").unwrap();
    datums.add_feature("fk", "cylinder", &["face:9"]).unwrap();
    let req = DefineRequest {
        label: "K",
        feature: "fk",
        faces: &["face:9"],
        feature_type: "cylinder",
        targets: &[],
        replace: false,
    };
    datums.define_datum(H, &pat, &req, false).unwrap();
    // A planar datum without measured points.
    datums.add_face("face:10").unwrap();
    datums.add_feature("fm", "plane", &["face:10"]).unwrap();
    let req = DefineRequest {
        label: "M",
        feature: "fm",
        faces: &["face:10"],
        feature_type: "plane",
        targets: &[],
        replace: false,
    };
    datums.define_datum(H, &pat, &req, false).unwrap();
    // A planar datum with a point datum target (GDT.DRF.COMMON's job, refused here).
    datums.add_face("face:11").unwrap();
    datums.add_feature("ft", "plane", &["face:11"]).unwrap();
    datums.add_geometry("pt:1", "point", "face:11").unwrap();
    let targets = [TargetRequest { id: "T1", kind: "point", geometry: "pt:1" }];
    let req = DefineRequest {
        label: "T",
        feature: "ft",
        faces: &["face:11"],
        feature_type: "plane",
        targets: &targets,
        replace: false,
    };
    datums.define_datum(H, &pat, &req, false).unwrap();
    let (xyz, nominal) = faces["A"].clone();
    points.insert("ft", xyz, nominal).unwrap();
    let journal = format!(r#"{{"entries":[],"schema":"{JOURNAL_SCHEMA}"}}"#);
    let register = RegisterView::none(&JournalExtract::load(journal.as_bytes()).unwrap());
    World { spec, dprof, table: table(table_name), datums, points, register }
}

/// The register inputs of `GDT.CHAR.INTERP`'s currency fixtures: the ROWFLOW journal that
/// built registers R1 and R2, their roster, the conflict inputs, and a published list.
struct Regs {
    journal: JournalExtract,
    rosters: Vec<Roster>,
    coi: CoiInputs,
    published: Vec<u8>,
    prior: Published,
}

impl Regs {
    /// `published`: `v1` (R1 latest) or `v2` (R2 published after R1).
    fn currency(published: &str) -> Regs {
        Regs {
            journal: JournalExtract::load(&read("interp/currency/journal.json")).unwrap(),
            rosters: vec![Roster::load(&read("status/roster.json")).unwrap()],
            coi: CoiInputs::load(&read("status/reviewers.toml"), &read("status/profiles.toml")).unwrap(),
            published: read(&format!("interp/currency/published_{published}.json")),
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

fn replay_with(w: &World, regs: &Regs, store: &RegisterStore, events: &[(Principal, Vec<u8>)]) -> ReplayOutcome {
    let sources = regs.sources();
    replay(&DrfStore::new(), &w.inputs(), &ReplayRegisters { store, sources: &sources }, events).outcome
}

impl World {
    fn inputs(&self) -> BuildInputs<'_> {
        BuildInputs {
            spec: &self.spec,
            dprof: &self.dprof,
            table: Some(&self.table),
            datums: &self.datums,
            points: &self.points,
            register: &self.register,
        }
    }

    fn build(&self, store: &mut DrfStore, p: Principal, datums: &[&str], dry_run: bool) -> Result<Built, BuildError> {
        build_drf(store, &self.inputs(), p, &BuildRequest { char: CHAR, datums }, dry_run)
    }
}

fn doc(b: &Built) -> CValue {
    fid_canon::parse(b.drf_bytes()).unwrap()
}

/// The frame of a built DRF against a reference case.
fn assert_frame(b: &Built, case: &str) {
    let all = cases();
    let e = obj(&obj(&all[case])["expect"]);
    let d = doc(b);
    let d = obj(&d);
    assert!(close(hex3(&d["origin"]), dec3(&e["origin"])), "{case}: origin");
    let axes = obj(&d["axes"]);
    for (k, name) in ["x", "y", "z"].iter().enumerate() {
        assert!(close(hex3(&axes[*name]), dec3(&arr(&e["axes"])[k])), "{case}: axis {name}");
    }
    let rows = arr(&d["datums"]);
    let want = arr(&e["dofs"]);
    assert_eq!(rows.len(), want.len());
    for (r, w) in rows.iter().zip(want) {
        assert_eq!(names(&obj(r)["constrained"]), names(w), "{case}");
    }
    assert_eq!(names(&d["free"]), names(&e["free"]), "{case}");
}

// ------------------------------------------------------------------ AC-1, AC-5, AC-7

#[test]
fn ac1_block_abc_from_labels_through_the_cli() {
    let w = world("minimax");
    let mut store = DrfStore::new();
    let out = invoke_cli(&mut store, &w.inputs(), H, &[BUILD_DRF_CLI, "--char", CHAR, "--datums", "A,B,C", "--json"])
        .unwrap();
    let b = out.built().unwrap().clone();
    assert_frame(&b, "abc_minimax");
    let d = doc(&b);
    let d = obj(&d);
    assert_eq!(st(&d["schema"]), DRF_SCHEMA);
    assert_eq!(st(&d["values"]), "measured");
    assert_eq!(st(&d["char"]), CHAR);
    let rows = arr(&d["datums"]);
    for (r, (label, prec)) in rows.iter().zip([("A", "primary"), ("B", "secondary"), ("C", "tertiary")]) {
        let r = obj(r);
        assert_eq!(st(&r["label"]), label);
        assert_eq!(st(&r["precedence"]), prec);
        assert_eq!(st(&r["boundary"]), "rmb");
        assert_eq!(st(&r["simulator"]), "fit.plane.tan_minimax@1.0.0");
        assert_eq!(st(&r["dmt_row"]), "dmt:asme/datum_frame/rmb@1");
    }
    // Results are certified only: the primary carries its certificate.
    let fit0 = obj(&obj(&rows[0])["fit"]);
    assert_eq!(st(&fit0["optimality"]), "strict_local");
    assert!(st(&fit0["certificate"]).starts_with("sha256:"));
    assert_eq!(st(&obj(&obj(&rows[1])["fit"])["optimality"]), "closed_form_enumerated");
    assert_eq!(st(&obj(&obj(&rows[2])["fit"])["optimality"]), "closed_form");
    // Stored, journaled, and the result bytes embed the document.
    assert_eq!(store.len(fid_gdt::chars::dprof::Branch::Working), 1);
    let ev = b.event().unwrap();
    assert_eq!(ev.datums(), ["A", "B", "C"]);
    assert_eq!(ev.drf_id(), b.id());
    let res = fid_canon::parse(&out.to_json_bytes().unwrap()).unwrap();
    assert_eq!(fid_canon::to_canonical_bytes(&obj(&res)["drf"]).unwrap(), b.drf_bytes());
    assert_eq!(st(&obj(&res)["id"]), b.id().as_str());
}

#[test]
fn ac1_l1_variant_and_two_datum_frame() {
    let w = world("l1");
    let mut store = DrfStore::new();
    assert_frame(&w.build(&mut store, H, &["A", "B", "C"], false).unwrap(), "abc_l1");
    let w = world("minimax");
    let b = w.build(&mut store, H, &["A", "B"], false).unwrap();
    assert_frame(&b, "ab_minimax");
    let b = w.build(&mut store, H, &["A"], false).unwrap();
    assert_frame(&b, "a_minimax");
}

#[test]
fn ac5_get_returns_the_canonical_bytes_unchanged() {
    let w = world("minimax");
    let mut store = DrfStore::new();
    let b = w.build(&mut store, H, &["A", "B", "C"], false).unwrap();
    let id = b.id().as_str().to_owned();
    assert_eq!(get_drf(&store, H, &id).unwrap(), b.drf_bytes());
    let cli = invoke_cli(&mut store, &w.inputs(), H, &[GET_DRF_CLI, "--id", &id, "--json"]).unwrap();
    assert_eq!(cli.to_json_bytes().unwrap(), b.drf_bytes());
    let raw = format!(r#"{{"id":"{id}"}}"#);
    let mcp = invoke_mcp(&mut store, &w.inputs(), H, GET_DRF_MCP, raw.as_bytes()).unwrap();
    assert_eq!(mcp.to_json_bytes().unwrap(), b.drf_bytes());
    assert_eq!(get_drf(&store, H, "sha256:00").unwrap_err().code(), "drf_unknown");
    assert_eq!(GET_COMMAND, "gdt.get_drf@1");
}

#[test]
fn ac7_exact_replay_of_ac1_is_identical() {
    let w = world("minimax");
    let mut store = DrfStore::new();
    let b = w.build(&mut store, H, &["A", "B", "C"], false).unwrap();
    let bytes = b.event().unwrap().canonical_bytes().unwrap();
    assert_eq!(DrfEvent::from_canonical_bytes(&bytes).unwrap(), *b.event().unwrap());
    let regs = Regs::currency("v1");
    let none = RegisterStore::new();
    let sources = regs.sources();
    let r = replay(
        &DrfStore::new(),
        &w.inputs(),
        &ReplayRegisters { store: &none, sources: &sources },
        &[(H, bytes.clone())],
    );
    assert_eq!(r.outcome, ReplayOutcome::Identical);
    assert_eq!(r.store, store);
    // The entry's principal binds the event: a forged principal diverges.
    assert_eq!(replay_with(&w, &regs, &none, &[(A, bytes.clone())]), ReplayOutcome::Divergent);
    // Unreadable or tampered bytes are not replayable.
    let tampered = String::from_utf8(bytes.clone()).unwrap().replace("\"A\",\"B\",\"C\"", "\"A\",\"C\",\"B\"");
    assert_ne!(tampered.as_bytes(), bytes.as_slice());
    assert_eq!(replay_with(&w, &regs, &none, &[(H, tampered.into_bytes())]), ReplayOutcome::Divergent);
    assert_eq!(replay_with(&w, &regs, &none, &[(H, b"{}".to_vec())]), ReplayOutcome::NotReplayable);
    // Measurements that changed since are not replayable.
    let mut w2 = world("minimax");
    w2.points.insert("fa", vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 1.0]).unwrap();
    assert_eq!(replay_with(&w2, &regs, &none, &[(H, bytes)]), ReplayOutcome::NotReplayable);
}

/// AC-7 through a verified register: a DRF built while R1 was the latest published register
/// pins R1, and still replays exactly after R2 is published (replay loads R1 from the store
/// and re-checks it for `Purpose::Replay`, never the caller's current view). Without R1 in the
/// store it is not replayable, and R1 is no longer valid for a new evaluation.
#[test]
fn ac7_replay_through_the_pinned_register_survives_a_newer_one() {
    let mut w = world("minimax");
    let r1 = read("interp/currency/register_r1.json");
    let v1 = Regs::currency("v1");
    w.register = RegisterView::load(&r1, &v1.sources(), Purpose::Evaluate);
    assert!(w.register.is_verified());
    let mut store = DrfStore::new();
    let b = w.build(&mut store, H, &["A", "B", "C"], false).unwrap();
    assert_eq!(b.event().unwrap().pin().verification(), "verified");
    let d = doc(&b);
    assert_eq!(st(&obj(&d)["interpretation_register"]), w.register.digest().unwrap().as_str());
    let bytes = b.event().unwrap().canonical_bytes().unwrap();

    let v2 = Regs::currency("v2");
    // R1 is stale for a new evaluation once R2 is published ...
    assert_eq!(RegisterView::load(&r1, &v2.sources(), Purpose::Evaluate).code(), "stale_register");
    // ... and the caller's current view does not matter to replay: give it none.
    w.register = RegisterView::none(&v2.journal);
    let mut registers = RegisterStore::new();
    registers.put(&r1).unwrap();
    assert_eq!(replay_with(&w, &v2, &registers, &[(H, bytes.clone())]), ReplayOutcome::Identical);
    assert_eq!(replay_with(&w, &v2, &RegisterStore::new(), &[(H, bytes)]), ReplayOutcome::NotReplayable);
}

#[test]
fn cli_and_mcp_give_the_same_result_and_event() {
    let w = world("minimax");
    let (mut s1, mut s2) = (DrfStore::new(), DrfStore::new());
    let c = invoke_cli(&mut s1, &w.inputs(), H, &[BUILD_DRF_CLI, "--char", CHAR, "--datums", "A,B,C"]).unwrap();
    let raw = format!(r#"{{"char":"{CHAR}","datums":["A","B","C"]}}"#);
    let m = invoke_mcp(&mut s2, &w.inputs(), H, BUILD_DRF_MCP, raw.as_bytes()).unwrap();
    assert_eq!(c.to_json_bytes().unwrap(), m.to_json_bytes().unwrap());
    assert_eq!(
        c.built().unwrap().event().unwrap().canonical_bytes().unwrap(),
        m.built().unwrap().event().unwrap().canonical_bytes().unwrap()
    );
    // D0: a second run gives the same bytes.
    let again = w.build(&mut DrfStore::new(), H, &["A", "B", "C"], false).unwrap();
    assert_eq!(again.drf_bytes(), c.built().unwrap().drf_bytes());
}

// ------------------------------------------------------------------ AC-3 and the refusals

fn refused(w: &World, datums: &[&str]) -> String {
    let mut store = DrfStore::new();
    let e = w.build(&mut store, H, datums, false).unwrap_err();
    // A refusal stores and journals nothing.
    assert!(store.is_empty(fid_gdt::chars::dprof::Branch::Working));
    e.to_string()
}

#[test]
fn ac3_parallel_pair_is_refused_and_journals_nothing() {
    let w = world("minimax");
    assert_eq!(refused(&w, &["A", "E"]), "degenerate_datum_pair{A,E}");
}

#[test]
fn ac2_ac4_cylindrical_datums_are_refused_typed_until_their_simulator_exists() {
    let w = world("minimax");
    // AC-2 (plane A, axis B) and AC-4 (a single primary axis) need a cylindrical datum
    // simulator (KRN.ENV.MCC.CYL / KRN.ENV.TAN.CYL), which is not built: a typed refusal,
    // never a made-up axis.
    assert_eq!(refused(&w, &["A", "K"]), "unsupported_datum_type_p1a{K}");
    assert_eq!(refused(&w, &["K"]), "unsupported_datum_type_p1a{K}");
    let mut store = DrfStore::new();
    let e = w.build(&mut store, H, &["K"], false).unwrap_err();
    assert!(matches!(e, BuildError::UnsupportedDatumType { feature_type: "cylinder", .. }));
}

#[test]
fn request_and_input_errors() {
    let w = world("minimax");
    assert_eq!(refused(&w, &[]), "no_datums");
    assert_eq!(refused(&w, &["A", "B", "C", "D"]), "too_many_datums{4}");
    assert_eq!(refused(&w, &["A", "A"]), "duplicate_datum_label{A}");
    assert_eq!(refused(&w, &["a"]), "bad_datum_reference{bad_label}");
    assert_eq!(refused(&w, &["A(X)"]), "bad_datum_reference{bad_modifier}");
    assert_eq!(refused(&w, &[""]), "bad_datum_reference{empty}");
    assert_eq!(refused(&w, &["Z"]), "datum_undefined{Z}");
    assert_eq!(refused(&w, &["A", "M"]), "measurement_missing{fm}");
    assert_eq!(refused(&w, &["A", "D"]), "oblique_datum_unsupported{D}");
    let mut store = DrfStore::new();
    let e = build_drf(&mut store, &w.inputs(), H, &BuildRequest { char: "nope", datums: &["A"] }, false).unwrap_err();
    assert_eq!(e.code(), "char_unknown");
    let e =
        build_drf(&mut store, &w.inputs(), H, &BuildRequest { char: "bad id!", datums: &["A"] }, false).unwrap_err();
    assert_eq!(e.to_string(), "bad_params{bad_char_id}");
    let inp = BuildInputs { table: None, ..w.inputs() };
    let e = build_drf(&mut store, &inp, H, &BuildRequest { char: CHAR, datums: &["A"] }, false).unwrap_err();
    assert_eq!(e.code(), "dmt_not_loaded");
    assert_eq!(refused(&world("minzone"), &["A"]), "unsupported_simulator{syn.assoc.min_zone@0.1.0}");
    assert_eq!(refused(&world("l2"), &["A"]), "simulation_failed{A:not_certifiable}");
    // Parameter grammar on both surfaces.
    let mut store = DrfStore::new();
    let cli = |store: &mut DrfStore, args: &[&str]| invoke_cli(store, &w.inputs(), H, args).unwrap_err().to_string();
    assert_eq!(cli(&mut store, &["nope"]), "bad_params{unknown subcommand}");
    assert_eq!(cli(&mut store, &[BUILD_DRF_CLI, "--char", CHAR]), "bad_params{missing parameter}");
    assert_eq!(cli(&mut store, &[BUILD_DRF_CLI, "--id", "x"]), "bad_params{unknown option}");
    assert_eq!(cli(&mut store, &[GET_DRF_CLI, "--char", CHAR]), "bad_params{unknown option}");
    assert_eq!(cli(&mut store, &[BUILD_DRF_CLI, "--char"]), "bad_params{option needs a value}");
    assert_eq!(cli(&mut store, &[BUILD_DRF_CLI, "--json", "--json"]), "bad_params{repeated option}");
    let mcp = |store: &mut DrfStore, tool: &str, raw: &str| {
        invoke_mcp(store, &w.inputs(), H, tool, raw.as_bytes()).unwrap_err().to_string()
    };
    assert_eq!(
        mcp(&mut store, BUILD_DRF_MCP, r#"{"char":"drf-1","datums":["A"],"x":1}"#),
        "bad_params{unknown parameter}"
    );
    assert_eq!(
        mcp(&mut store, BUILD_DRF_MCP, r#"{"char":"drf-1","char":"drf-1","datums":["A"]}"#),
        "bad_params{duplicate key}"
    );
    assert_eq!(
        mcp(&mut store, BUILD_DRF_MCP, r#"{"char":"drf-1","datums":"A"}"#),
        "bad_params{datums is not a string array}"
    );
    assert_eq!(mcp(&mut store, "gdt_other", "{}"), "bad_params{unknown tool}");
    assert!(store.is_empty(fid_gdt::chars::dprof::Branch::Working));
}

#[test]
fn dry_run_returns_the_drf_and_changes_nothing() {
    let w = world("minimax");
    let mut store = DrfStore::new();
    let d = w.build(&mut store, H, &["A", "B", "C"], true).unwrap();
    assert!(d.dry_run());
    assert!(d.event().is_none());
    assert!(store.is_empty(fid_gdt::chars::dprof::Branch::Working));
    let real = w.build(&mut store, H, &["A", "B", "C"], false).unwrap();
    assert_eq!(d.drf_bytes(), real.drf_bytes());
    let out = invoke_cli(
        &mut DrfStore::new(),
        &w.inputs(),
        H,
        &[BUILD_DRF_CLI, "--char", CHAR, "--datums", "A", "--dry-run"],
    )
    .unwrap();
    assert!(out.built().unwrap().dry_run());
}

// ------------------------------------------------------------------ seeded faults

/// Seeded fault, wrong datum precedence: B|A|C is another frame than A|B|C, and each matches
/// its own reference. Code that sorted, reversed or ignored the order fails one of these.
#[test]
fn seeded_fault_wrong_precedence() {
    let w = world("minimax");
    let mut store = DrfStore::new();
    let abc = w.build(&mut store, H, &["A", "B", "C"], false).unwrap();
    let bac = w.build(&mut store, H, &["B", "A", "C"], false).unwrap();
    let acb = w.build(&mut store, H, &["A", "C", "B"], false).unwrap();
    assert_frame(&abc, "abc_minimax");
    assert_frame(&bac, "bac_minimax");
    assert_frame(&acb, "acb_minimax");
    assert_ne!(abc.id(), bac.id());
    assert_ne!(abc.id(), acb.id());
    let z = |b: &Built| hex3(&obj(&obj(&doc(b))["axes"])["z"]);
    assert!((0..3).any(|k| (z(&abc)[k] - z(&bac)[k]).abs() > 1e-6));
}

/// Seeded fault, wrong material-condition handling: `A(M)` and `B(L)` are refused, typed,
/// before anything is simulated, stored or journaled; reading them as RMB would have built
/// the RMB frame (shown below), so a silent fallback is visible.
#[test]
fn seeded_fault_material_boundary_is_never_read_as_rmb() {
    let w = world("minimax");
    assert_eq!(refused(&w, &["A(M)"]), "unsupported_material_boundary{A}");
    assert_eq!(refused(&w, &["A", "B(L)", "C"]), "unsupported_material_boundary{B}");
    assert_eq!(refused(&w, &["A", "B", "C(M)"]), "unsupported_material_boundary{C}");
    let mut store = DrfStore::new();
    let e = w.build(&mut store, H, &["A", "B(L)"], false).unwrap_err();
    assert!(matches!(e, BuildError::UnsupportedMaterialBoundary { boundary: "lmb", .. }));
    // The refused request differs from the RMB request only in the modifier.
    assert!(w.build(&mut store, H, &["A", "B"], false).is_ok());
    let r = DatumRef::parse("B(L)").unwrap();
    assert_eq!((r.label().as_str(), r.boundary().as_str(), r.text()), ("B", "lmb", "B(L)".to_owned()));
    // The boundary gate runs before label resolution: even an undeclared datum at MMB is a
    // material-boundary refusal.
    assert_eq!(refused(&w, &["Z(M)"]), "unsupported_material_boundary{Z}");
}

// ------------------------------------------------------------------ D-045

#[test]
fn every_drf_carries_the_interpretation_block_and_shows_the_provisional_label() {
    let w = world("minimax");
    let mut store = DrfStore::new();
    let b = w.build(&mut store, H, &["A", "B", "C"], false).unwrap();
    let d = doc(&b);
    let d = obj(&d);
    assert_eq!(st(&d["interpretation"]), "provisional");
    assert_eq!(st(&d["interpretation_register"]), "none");
    let rules = arr(&d["interpretation_rules"]);
    let kinds: Vec<&str> = rules.iter().map(|r| st(&obj(r)["kind"])).collect();
    assert_eq!(kinds, ["dmt_row", "profile"]);
    for r in rules {
        assert_eq!(st(&obj(r)["status"]), "provisional");
        assert_eq!(st(&obj(r)["reason"]), "no_register");
    }
    assert_eq!(st(&obj(&rules[0])["rule"]), "dmt:asme/datum_frame/rmb@1");
    // Every surface asks the D-045 predicate.
    assert!(requires_provisional_label_bytes(b.drf_bytes()));
    assert_eq!(drf_label(b.drf_bytes()), PROVISIONAL_LABEL_TEXT);
    assert_eq!(b.label(), PROVISIONAL_LABEL_TEXT);
    let res = fid_canon::parse(&b.to_json_bytes().unwrap()).unwrap();
    assert_eq!(st(&obj(&res)["label"]), "interpretation: provisional");
    // The event pins the register consulted.
    assert_eq!(b.event().unwrap().pin().verification(), "none");
}

// ------------------------------------------------------------------ branches

#[test]
fn agent_drfs_stay_on_the_overlay() {
    let w = world("minimax");
    let mut store = DrfStore::new();
    let mine = w.build(&mut store, H, &["A", "B"], false).unwrap();
    let theirs = w.build(&mut store, A, &["A", "B", "C"], false).unwrap();
    assert_eq!(theirs.branch(), fid_gdt::chars::dprof::Branch::Agent);
    assert_eq!(store.len(fid_gdt::chars::dprof::Branch::Working), 1);
    assert!(get_drf(&store, H, theirs.id().as_str()).is_err());
    assert_eq!(get_drf(&store, A, theirs.id().as_str()).unwrap(), theirs.drf_bytes());
    assert_eq!(get_drf(&store, A, mine.id().as_str()).unwrap(), mine.drf_bytes());
}

// ------------------------------------------------------------------ published schemas

struct NoHooks;

impl Hooks for NoHooks {
    fn literal(&self, _: &str, _: &CValue) -> Result<Normalised, BusError> {
        Err(BusError::Validation(ValidationReason::UnitMismatch, Detail::new("no quantities here")))
    }
    fn entity(&self, _: &str) -> RefLookup {
        RefLookup::Missing
    }
}

fn valid(schema: &Compiled, v: &CValue, what: &str) {
    if let Err(f) = schema.evaluate(v, &NoHooks) {
        panic!("{what}: {} at {} ({})", f.keyword, f.pointer, f.message);
    }
}

#[test]
fn definitions_and_results_conform_to_the_published_schemas() {
    let b = build_command_def().unwrap();
    let g = get_command_def().unwrap();
    assert_eq!(format!("{}@{}", b.id, b.major), BUILD_COMMAND);
    assert_eq!(format!("{}@{}", g.id, g.major), GET_COMMAND);
    assert_eq!(b.side_effect.as_str(), "W2");
    assert_eq!(g.side_effect.as_str(), "R0");
    assert_eq!(b.determinism.as_str(), "D0");
    assert_eq!(b.effects, [BUILD_COMMAND]);
    let geo = fid_registry::Catalogue::geo_frame().unwrap();
    for s in [&b.params_schema, &b.result_schema, &g.params_schema, &g.result_schema] {
        assert_eq!(fid_registry::check_schema(s, &geo), vec![]);
    }
    let (bp, br) = (Compiled::compile(&b.params_schema).unwrap(), Compiled::compile(&b.result_schema).unwrap());
    let (gp, gr) = (Compiled::compile(&g.params_schema).unwrap(), Compiled::compile(&g.result_schema).unwrap());

    let w = world("minimax");
    let mut store = DrfStore::new();
    for datums in [&["A"][..], &["A", "B"][..], &["A", "B", "C"][..]] {
        let built = w.build(&mut store, H, datums, false).unwrap();
        valid(&br, &fid_canon::parse(&built.to_json_bytes().unwrap()).unwrap(), "build result");
        valid(&gr, &fid_canon::parse(built.drf_bytes()).unwrap(), "get result");
        let raw = format!(r#"{{"id":"{}"}}"#, built.id().as_str());
        valid(&gp, &fid_canon::parse(raw.as_bytes()).unwrap(), "get params");
    }
    let ok = fid_canon::parse(br#"{"char":"drf-1","datums":["A","B(M)","C"],"dry_run":true}"#).unwrap();
    valid(&bp, &ok, "build params");
    for bad in [
        r#"{"char":"drf-1","datums":[]}"#,
        r#"{"char":"drf-1","datums":["A","B","C","D"]}"#,
        r#"{"char":"drf-1","datums":["a"]}"#,
        r#"{"char":"drf-1","datums":["A(X)"]}"#,
        r#"{"char":"drf-1","datums":["A","A"]}"#,
        r#"{"datums":["A"]}"#,
        r#"{"char":"drf-1","datums":["A"],"extra":1}"#,
    ] {
        assert!(bp.evaluate(&fid_canon::parse(bad.as_bytes()).unwrap(), &NoHooks).is_err(), "{bad}");
    }
    // The schema is not vacuous: a document claiming other values is refused.
    let built = w.build(&mut store, H, &["A"], false).unwrap();
    let mut d = fid_canon::parse(built.drf_bytes()).unwrap();
    if let CValue::Obj(o) = &mut d {
        o.insert("values".to_owned(), CValue::Str("claimed".to_owned()));
    }
    assert!(gr.evaluate(&d, &NoHooks).is_err());
}

// ------------------------------------------------------------------ refusal paths (review of #370)

/// `rule_not_admitted{rule}`: the frame row's content was only ever proposed by an agent, as
/// journaled for real in `journal_agent_draft.json` (a D-045 draft), so no DRF is built from
/// it, whatever the register; the same row with no such journal entry is provisional.
#[test]
fn a_draft_frame_row_is_not_admitted() {
    let mut w = world("minimax");
    let journal = JournalExtract::load(&read("drf/measured/journal_agent_draft.json")).unwrap();
    w.register = RegisterView::none(&journal);
    assert_eq!(refused(&w, &["A"]), "rule_not_admitted{dmt:asme/datum_frame/rmb@1}");
    // The L1 row has other content: the agent's proposal does not make it a draft.
    let mut w = world("l1");
    w.register = RegisterView::none(&journal);
    assert!(w.build(&mut DrfStore::new(), H, &["A"], false).is_ok());
}

#[test]
fn a_chain_that_does_not_resolve_is_refused() {
    assert_eq!(refused(&world("none"), &["A"]), "chain_not_resolved{UNRESOLVED}");
}

#[test]
fn datums_with_targets_are_refused() {
    assert_eq!(refused(&world("minimax"), &["A", "T"]), "datum_targets_unsupported{T}");
}
