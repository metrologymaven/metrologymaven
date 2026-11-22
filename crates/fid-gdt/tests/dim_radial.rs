//! Acceptance criteria of spec node GDT.DIM.RADIAL through the command layer
//! (`check.eval_diameter@1`, `check.eval_radius@1`): AC-1 (LS circle, cylinder and sphere
//! through `fid check eval-diameter --json` / `eval-radius --json` within 1e-12 m), AC-3 (cone
//! without gauge position, nothing journaled), AC-4 (`fit_method_mismatch`), AC-5 under the
//! maintainer rule (a refused fit is never a value), AC-6 (exact replay `IDENTICAL`), and the
//! ResultRecord the commands build (`GDT.CHAR.RECORD`, D-045 label). Expected values come from
//! the independent reference `fixtures/gdt/dim/make_radial.py` (planted geometry); error paths
//! are hand-labelled from the leaf's rules. AC-2 (cone at a gauge) is open: the cone fit is not
//! on main. Synthetic data only.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_bus::error::{BusError, Detail, ValidationReason};
use fid_bus::unit::Normalised;
use fid_bus::validate::{Compiled, Hooks, RefLookup};
use fid_canon::CValue;
use fid_gdt::chars::dprof::{DprofState, KeyVocabulary};
use fid_gdt::chars::interp::{RegisterSources, RegisterStore, RegisterView, PROVISIONAL_LABEL_TEXT};
use fid_gdt::chars::record::{ResultStore, RunId};
use fid_gdt::chars::spec::{load_project, SpecState};
use fid_gdt::dim::radial::{
    commit, diameter_command_def, eval_radial, invoke_cli, invoke_mcp, radius_command_def, replay, EvalError,
    EvalInputs, EvalRequest, Evaluated, RadialEvent, RadialFeatures, EVAL_DIAMETER_CLI, EVAL_DIAMETER_COMMAND,
    EVAL_DIAMETER_MCP, EVAL_RADIUS_CLI, EVAL_RADIUS_COMMAND, EVAL_RADIUS_MCP, RESULT_SCHEMA,
};
use fid_gdt::dmt_flow::roster::Roster;
use fid_gdt::dmt_flow::status::{CoiInputs, JournalExtract, Published, JOURNAL_SCHEMA};
use fid_gdt::drf::build::ReplayRegisters;
use fid_gdt::requires_provisional_label;
use fid_gps::dim::radial::{Gauge, Measure};
use fid_gps::dmt::DmtTable;
use fid_io_reg::Principal;
use fid_replay::ReplayOutcome;
use fid_types::Digest;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

const H: Principal = Principal::Human;
const A: Principal = Principal::Agent;
const TOL: f64 = 1e-12;
const CHARS: [&str; 4] = ["dia-1", "dia-2", "rad-1", "rad-2"];

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

fn unhex(v: &CValue) -> f64 {
    let s = st(v);
    assert!(s.starts_with("0x") && s.len() == 18, "{s}");
    f64::from_bits(u64::from_str_radix(&s[2..], 16).unwrap())
}

fn entry(n: u8) -> Digest {
    Digest::from_bytes([n; 32])
}

/// The reference features of `radial_analytic.json`, by feature ID.
fn reference() -> BTreeMap<String, BTreeMap<String, CValue>> {
    let doc = fid_canon::parse(&read("dim/radial_analytic.json")).unwrap();
    arr(&obj(&doc)["features"]).iter().map(|f| (st(&obj(f)["feature"]).to_owned(), obj(f).clone())).collect()
}

fn num(f: &BTreeMap<String, CValue>, k: &str) -> f64 {
    st(&f[k]).parse().unwrap()
}

fn test_vocab() -> KeyVocabulary {
    let v = fid_canon::parse(&read("dprof/vocabulary.json")).unwrap();
    let keys: Vec<&str> = arr(&obj(&v)["keys"]).iter().map(st).collect();
    KeyVocabulary::new(keys).unwrap()
}

/// The synthetic DMT table of `GDT.RES.SCHEMA` (whose rows the plan's defaults profile names)
/// with the synthetic `diameter` and `radius` rows of `radial_table.json` added.
fn table() -> DmtTable {
    let mut base = fid_canon::parse(&read("dmt/synthetic_table_v1.json")).unwrap();
    let radial = fid_canon::parse(&read("dim/radial_table.json")).unwrap();
    if let CValue::Obj(o) = &mut base {
        if let Some(CValue::Arr(rows)) = o.get_mut("rows") {
            rows.extend(arr(&obj(&radial)["rows"]).iter().cloned());
        }
    }
    DmtTable::from_value(&base).unwrap()
}

struct World {
    spec: SpecState,
    dprof: DprofState,
    table: DmtTable,
    features: RadialFeatures,
    register: RegisterView,
}

fn world() -> World {
    let (mut spec, mut dprof) = load_project(&read("spec/plan_asme.fidproj"), &test_vocab()).unwrap();
    for c in CHARS {
        spec.create_characteristic(&mut dprof, H, "plan-1", c, false).unwrap();
    }
    let mut features = RadialFeatures::new();
    for (name, f) in reference() {
        let xyz: Vec<f64> = arr(&f["xyz"]).iter().map(|x| st(x).parse().unwrap()).collect();
        features.insert(&name, st(&f["fit"]), xyz).unwrap();
    }
    let journal = format!(r#"{{"entries":[],"schema":"{JOURNAL_SCHEMA}"}}"#);
    let register = RegisterView::none(&JournalExtract::load(journal.as_bytes()).unwrap());
    World { spec, dprof, table: table(), features, register }
}

impl World {
    fn inputs(&self) -> EvalInputs<'_> {
        EvalInputs {
            spec: &self.spec,
            dprof: &self.dprof,
            table: Some(&self.table),
            features: &self.features,
            register: &self.register,
        }
    }

    fn eval(
        &self,
        store: &ResultStore,
        p: Principal,
        measure: Measure,
        char: &str,
        feature: &str,
        gauge: Option<Gauge>,
    ) -> Result<Evaluated, EvalError> {
        let variant = reference()[feature].get("variant").map(|v| st(v).to_owned());
        let req = EvalRequest { run: "run-1", char, feature, variant: variant.as_deref(), gauge };
        eval_radial(store, &self.inputs(), p, measure, &req, false)
    }
}

/// The register inputs the replay check needs (a `none` pin never reads them).
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

fn json(ev: &Evaluated) -> CValue {
    fid_canon::parse(&ev.to_json_bytes().unwrap()).unwrap()
}

// ------------------------------------------------------------------ AC-1

#[test]
fn ac1_values_of_the_ls_circle_cylinder_and_sphere_through_the_cli() {
    let w = world();
    let mut seen = 0;
    for (name, f) in reference().iter().filter(|(n, _)| n.as_str() != "f-cone") {
        let variant = st(&f["variant"]);
        for (sub, want, measure) in
            [(EVAL_DIAMETER_CLI, num(f, "diameter"), "diameter"), (EVAL_RADIUS_CLI, num(f, "radius"), "radius")]
        {
            let store = ResultStore::new();
            let args = [sub, "--run", "run-1", "--char", "dia-1", "--feature", name, "--variant", variant, "--json"];
            let ev = invoke_cli(&store, &w.inputs(), H, &args).unwrap();
            let v = json(&ev);
            let o = obj(&v);
            let got = unhex(&o["value"]);
            assert!((got - want).abs() <= TOL, "{name} {measure}: {got:e} against {want:e}");
            assert_eq!(st(&o["measure"]), measure);
            assert_eq!(st(&o["unit"]), "si:m");
            assert_eq!(st(&o["schema"]), RESULT_SCHEMA);
            // REQ-003: the fit that produced the feature is recorded.
            assert_eq!(st(&obj(&o["fit"])["algorithm"]), st(&f["fit"]));
            assert_eq!(st(&obj(&o["chain"])["variant"]), variant);
        }
        seen += 1;
    }
    assert_eq!(seen, 4);
}

#[test]
fn ac1_mcp_and_cli_give_the_same_bytes_and_event() {
    let w = world();
    let store = ResultStore::new();
    let cli = invoke_cli(
        &store,
        &w.inputs(),
        H,
        &[EVAL_RADIUS_CLI, "--run", "run-1", "--char", "rad-1", "--feature", "f-sphere", "--variant", "ls_sphere"],
    )
    .unwrap();
    let raw = br#"{"char":"rad-1","feature":"f-sphere","run":"run-1","variant":"ls_sphere"}"#;
    let mcp = invoke_mcp(&store, &w.inputs(), H, EVAL_RADIUS_MCP, raw).unwrap();
    assert_eq!(cli.to_json_bytes().unwrap(), mcp.to_json_bytes().unwrap());
    assert_eq!(cli.event().unwrap().canonical_bytes().unwrap(), mcp.event().unwrap().canonical_bytes().unwrap());
    assert_eq!(mcp.event().unwrap().command(), EVAL_RADIUS_COMMAND);
    let d = invoke_mcp(&store, &w.inputs(), H, EVAL_DIAMETER_MCP, raw).unwrap();
    assert_eq!(d.event().unwrap().command(), EVAL_DIAMETER_COMMAND);
    assert_eq!(d.value().value().to_bits(), (2.0 * mcp.value().value()).to_bits());
}

// ------------------------------------------------------------------ the ResultRecord

#[test]
fn the_record_carries_the_fit_the_basis_and_the_provisional_label() {
    let w = world();
    let mut store = ResultStore::new();
    let ev = w.eval(&store, H, Measure::Diameter, "dia-1", "f-cylinder", None).unwrap();
    let digest = commit(&mut store, H, &ev, entry(7)).unwrap();
    let rec = store.get(H, &RunId::parse("run-1").unwrap(), &fid_gdt::chars::dprof::CharId::parse("dia-1").unwrap());
    let rec = rec.unwrap();
    assert_eq!(rec.digest(), digest);
    let v = fid_canon::parse(rec.canonical_json()).unwrap();
    let o = obj(&v);
    assert_eq!(st(&o["algorithm"]), "fit.cylinder.ls@1.0.0");
    assert_eq!(st(&o["evaluation_basis"]), "POINTS");
    assert_eq!(st(&o["measurand"]), "diameter of f-cylinder");
    assert_eq!(st(&o["dmt_row"]), "dmt:asme/diameter/none@1");
    assert_eq!(st(&o["journal_entry"]), entry(7).as_str());
    let value = obj(&o["value"]);
    assert_eq!(st(&value["unit"]), "si:m");
    assert!((unhex(&value["value"]) - 0.025).abs() <= TOL);
    assert_eq!(arr(&o["input_digests"]).len(), 1);
    assert_eq!(st(&arr(&o["input_digests"])[0]), ev.points_digest().as_str());
    // D-045: nothing claimed, the value is not decided, and the label is provisional.
    let u = obj(&o["uncertainty"]);
    assert_eq!(st(&u["class"]), "UEC-0");
    assert_eq!(u["claimed"], CValue::Bool(false));
    assert_eq!(st(&obj(&o["decision"])["source"]), "gdt_not_decidable");
    assert_eq!(st(&o["interpretation"]), "provisional");
    assert!(requires_provisional_label(rec));
    assert_eq!(ev.label(), PROVISIONAL_LABEL_TEXT);
    // One record per (run, characteristic): a second evaluation and a second commit refuse.
    let e = w.eval(&store, H, Measure::Diameter, "dia-1", "f-sphere", None).unwrap_err();
    assert_eq!(e.code(), "duplicate_result");
    assert_eq!(commit(&mut store, H, &ev, entry(8)).unwrap_err().code(), "duplicate_result");
}

#[test]
fn a_dry_run_has_no_event_and_no_record() {
    let w = world();
    let mut store = ResultStore::new();
    let raw = br#"{"char":"dia-1","dry_run":true,"feature":"f-circle3","run":"run-1"}"#;
    let ev = invoke_mcp(&store, &w.inputs(), H, EVAL_DIAMETER_MCP, raw).unwrap();
    assert!(ev.dry_run());
    assert!(ev.event().is_none());
    assert_eq!(obj(&json(&ev))["dry_run"], CValue::Bool(true));
    assert_eq!(commit(&mut store, H, &ev, entry(1)).unwrap_err().to_string(), "bad_params{dry_run}");
    assert!(store.is_empty());
    // The row's default variant is the LS circle in space.
    assert_eq!(ev.value().algorithm(), "fit.circle3.ls@1.0.0");
}

#[test]
fn an_agent_commits_to_the_agent_branch() {
    let w = world();
    let mut store = ResultStore::new();
    let ev = w.eval(&store, A, Measure::Radius, "rad-1", "f-circle2", None).unwrap();
    assert_eq!(ev.branch().as_str(), "agent");
    assert_eq!(commit(&mut store, H, &ev, entry(2)).unwrap_err().to_string(), "bad_params{principal_mismatch}");
    commit(&mut store, A, &ev, entry(2)).unwrap();
    let (run, char) = (RunId::parse("run-1").unwrap(), fid_gdt::chars::dprof::CharId::parse("rad-1").unwrap());
    assert!(store.get(A, &run, &char).is_ok());
    assert_eq!(store.get(H, &run, &char).unwrap_err().code(), "not_found");
}

// ------------------------------------------------------------------ AC-2 (open), AC-3, AC-4, AC-5

#[test]
fn ac3_a_cone_without_gauge_position_fails_and_journals_nothing() {
    let w = world();
    let store = ResultStore::new();
    for tool in [EVAL_DIAMETER_MCP, EVAL_RADIUS_MCP] {
        let raw = br#"{"char":"dia-1","feature":"f-cone","run":"run-1","variant":"ls_cone"}"#;
        let e = invoke_mcp(&store, &w.inputs(), H, tool, raw).unwrap_err();
        assert_eq!(e.to_string(), "gauge_position_missing");
    }
    assert!(store.is_empty());
}

/// AC-2 waits on the cone fit (`KRN.LSQ.AXIS.CONE`, not on main): with a gauge the cone is
/// refused, never estimated.
#[test]
fn ac2_open_a_cone_with_a_gauge_is_refused_until_the_cone_fit_lands() {
    let w = world();
    let store = ResultStore::new();
    let args = [
        EVAL_DIAMETER_CLI,
        "--run",
        "run-1",
        "--char",
        "dia-1",
        "--feature",
        "f-cone",
        "--variant",
        "ls_cone",
        "--gauge-from-apex",
        "0.01",
    ];
    let e = invoke_cli(&store, &w.inputs(), H, &args).unwrap_err();
    assert_eq!(e.to_string(), "fit_unavailable{fit.cone.ls@1.0.0}");
    // A gauge on a circle is refused.
    let e = w.eval(&store, H, Measure::Diameter, "dia-1", "f-circle3", Some(Gauge::FromApex(0.01))).unwrap_err();
    assert_eq!(e.to_string(), "gauge_not_applicable");
}

#[test]
fn ac4_ls_feature_under_a_maximum_inscribed_chain_is_refused() {
    let w = world();
    let store = ResultStore::new();
    let raw = br#"{"char":"dia-1","feature":"f-circle3","run":"run-1","variant":"mi_circle3"}"#;
    let e = invoke_mcp(&store, &w.inputs(), H, EVAL_DIAMETER_MCP, raw).unwrap_err();
    assert_eq!(e.to_string(), "fit_method_mismatch{fit.circle3.ls@1.0.0:fit.circle3.mi@1.0.0}");
    // The cylinder under the circle chain, and a chain with two association steps.
    let raw = br#"{"char":"dia-1","feature":"f-cylinder","run":"run-1","variant":"ls_circle3"}"#;
    let e = invoke_mcp(&store, &w.inputs(), H, EVAL_DIAMETER_MCP, raw).unwrap_err();
    assert_eq!(e.code(), "fit_method_mismatch");
    let raw = br#"{"char":"dia-1","feature":"f-circle3","run":"run-1","variant":"two_assoc"}"#;
    let e = invoke_mcp(&store, &w.inputs(), H, EVAL_DIAMETER_MCP, raw).unwrap_err();
    assert_eq!(e.to_string(), "ambiguous_association{2}");
}

/// AC-5 under the maintainer rule: a fit `fid-fit` refuses (here a sphere from coplanar
/// points) passes its refusal through and gives no value, no event and no record. The flagged
/// cylinder (`local_optimum_only` / `suspect_axis`) is refused the same way through the `Fit`
/// trait; its mapping is tested in `fid-gps`.
#[test]
fn ac5_a_refused_fit_is_never_a_value() {
    let mut w = world();
    let flat = vec![0.0, 0.0, 0.0, 0.01, 0.0, 0.0, 0.0, 0.01, 0.0, 0.01, 0.01, 0.0, 0.005, 0.002, 0.0];
    w.features.insert("f-flat", "fit.sphere.ls@1.0.0", flat).unwrap();
    let store = ResultStore::new();
    let raw = br#"{"char":"dia-1","feature":"f-flat","run":"run-1","variant":"ls_sphere"}"#;
    let e = invoke_mcp(&store, &w.inputs(), H, EVAL_DIAMETER_MCP, raw).unwrap_err();
    assert_eq!(e.to_string(), "fit_refused{coplanar}");
}

#[test]
fn other_refusals_pass_through() {
    let w = world();
    let store = ResultStore::new();
    let call = |raw: &[u8]| invoke_mcp(&store, &w.inputs(), H, EVAL_DIAMETER_MCP, raw).unwrap_err().to_string();
    // No characteristic with that ID: the governing-spec gate refuses.
    assert_eq!(
        call(br#"{"char":"nope-1","feature":"f-circle3","run":"run-1"}"#).split('{').next(),
        Some("char_unknown")
    );
    assert_eq!(call(br#"{"char":"dia-1","feature":"f-none","run":"run-1"}"#), "measurement_missing{f-none}");
    let e = call(br#"{"char":"dia-1","feature":"f-circle3","run":"run-1","variant":"nope"}"#);
    assert_eq!(e.split('{').next(), Some("unknown_variant"), "{e}");
    assert_eq!(call(br#"{"char":"dia-1","feature":"f-circle3"}"#), "bad_params{missing parameter}");
    assert_eq!(call(br#"{"char":"dia-1","feature":"f-circle3","run":"run-1","x":1}"#), "bad_params{unknown parameter}");
    assert_eq!(call(br#"{"char":"dia-1","char":"dia-2","feature":"f","run":"r"}"#), "bad_params{duplicate key}");
    assert_eq!(
        call(br#"{"char":"dia-1","feature":"f-cone","gauge":{"from_plane":1},"run":"run-1"}"#),
        "bad_params{gauge is not {from_apex: number}}"
    );
    let inputs = EvalInputs { table: None, ..w.inputs() };
    let req = EvalRequest { run: "run-1", char: "dia-1", feature: "f-circle3", variant: None, gauge: None };
    let e = eval_radial(&store, &inputs, H, Measure::Radius, &req, false).unwrap_err();
    assert_eq!(e.code(), "dmt_not_loaded");
    let e = invoke_cli(&store, &w.inputs(), H, &["eval-size", "--run", "r"]).unwrap_err();
    assert_eq!(e.to_string(), "bad_params{unknown subcommand}");
    let e = invoke_cli(&store, &w.inputs(), H, &[EVAL_DIAMETER_CLI, "--gauge-from-apex", "ten"]).unwrap_err();
    assert_eq!(e.to_string(), "bad_params{gauge is not a number}");
}

// ------------------------------------------------------------------ AC-6 exact replay

fn events(w: &World) -> (ResultStore, Vec<(Principal, Digest, Vec<u8>)>) {
    let mut store = ResultStore::new();
    let mut out = Vec::new();
    let calls = [
        (H, Measure::Diameter, "dia-1", "f-circle3"),
        (H, Measure::Diameter, "dia-2", "f-cylinder"),
        (H, Measure::Radius, "rad-1", "f-sphere"),
        (A, Measure::Radius, "rad-2", "f-circle2"),
    ];
    for (i, (p, m, c, f)) in calls.into_iter().enumerate() {
        let ev = w.eval(&store, p, m, c, f, None).unwrap();
        let hash = entry(10 + i as u8);
        let bytes = ev.event().unwrap().canonical_bytes().unwrap();
        assert_eq!(RadialEvent::from_canonical_bytes(&bytes).unwrap().canonical_bytes().unwrap(), bytes);
        commit(&mut store, p, &ev, hash).unwrap();
        out.push((p, hash, bytes));
    }
    (store, out)
}

#[test]
fn ac6_exact_replay_is_identical_and_rebuilds_the_same_records() {
    let w = world();
    let (live, evs) = events(&w);
    let regs = Regs::new();
    let sources = regs.sources();
    let rstore = RegisterStore::new();
    let r = replay(&ResultStore::new(), &w.inputs(), &ReplayRegisters { store: &rstore, sources: &sources }, &evs);
    assert_eq!(r.outcome, ReplayOutcome::Identical);
    assert_eq!(r.steps.len(), 4);
    let run = RunId::parse("run-1").unwrap();
    for (p, c) in [(H, "dia-1"), (H, "dia-2"), (H, "rad-1"), (A, "rad-2")] {
        let c = fid_gdt::chars::dprof::CharId::parse(c).unwrap();
        assert_eq!(
            r.store.get(p, &run, &c).unwrap().canonical_json(),
            live.get(p, &run, &c).unwrap().canonical_json(),
            "{c:?}"
        );
    }
}

#[test]
fn replay_refuses_forged_moved_or_tampered_events() {
    let w = world();
    let (_, evs) = events(&w);
    let regs = Regs::new();
    let sources = regs.sources();
    let rstore = RegisterStore::new();
    let regs = ReplayRegisters { store: &rstore, sources: &sources };
    let run = |evs: &[(Principal, Digest, Vec<u8>)], inp: &EvalInputs<'_>| {
        replay(&ResultStore::new(), inp, &regs, evs).outcome
    };
    // An event under an entry of another principal.
    let mut forged = evs.clone();
    forged[0].0 = A;
    assert_eq!(run(&forged, &w.inputs()), ReplayOutcome::Divergent);
    // A changed value (re-encoded canonically, so it parses).
    let text = String::from_utf8(evs[0].2.clone()).unwrap();
    let ev = RadialEvent::from_canonical_bytes(&evs[0].2).unwrap();
    let bits = format!("0x{:016x}", ev.value().to_bits());
    let other = format!("0x{:016x}", (ev.value() * 2.0).to_bits());
    let mut tampered = evs.clone();
    tampered[0].2 = text.replace(&bits, &other).into_bytes();
    assert!(RadialEvent::from_canonical_bytes(&tampered[0].2).is_ok());
    assert_eq!(run(&tampered, &w.inputs()), ReplayOutcome::Divergent);
    // Bytes that are not an event.
    let mut junk = evs.clone();
    junk[1].2 = b"{}".to_vec();
    assert_eq!(run(&junk, &w.inputs()), ReplayOutcome::NotReplayable);
    // The measured points are no longer the ones the event read.
    let mut moved = RadialFeatures::new();
    for (name, f) in reference() {
        let mut xyz: Vec<f64> = arr(&f["xyz"]).iter().map(|x| st(x).parse().unwrap()).collect();
        xyz[0] += 1e-9;
        moved.insert(&name, st(&f["fit"]), xyz).unwrap();
    }
    let inp = EvalInputs { features: &moved, ..w.inputs() };
    assert_eq!(run(&evs, &inp), ReplayOutcome::NotReplayable);
    // Replaying the same event twice: the second is a duplicate record.
    let twice = vec![evs[0].clone(), evs[0].clone()];
    let r = replay(&ResultStore::new(), &w.inputs(), &regs, &twice);
    assert_eq!(r.steps, vec![ReplayOutcome::Identical, ReplayOutcome::Divergent]);
}

#[test]
fn events_are_read_strictly() {
    let w = world();
    let (_, evs) = events(&w);
    let good = String::from_utf8(evs[0].2.clone()).unwrap();
    let bad = |s: &str| RadialEvent::from_canonical_bytes(s.as_bytes()).unwrap_err().to_string();
    assert_eq!(bad(&good.replace("\"working\"", "\"agent\"")), "event_invalid{branch_principal_mismatch}");
    assert_eq!(bad(&good.replace("check.eval_diameter@1", "check.eval_size@1")), "event_invalid{unknown_command}");
    assert_eq!(bad(&good.replacen('{', "{ ", 1)), "event_invalid{not_canonical}");
    let tail = ",\"variant\":\"ls_circle3\"}";
    assert!(good.ends_with(tail), "{good}");
    let seven = format!("{},\"variant\":7}}", &good[..good.len() - tail.len()]);
    assert_eq!(bad(&seven), "event_invalid{bad_variant}");
    assert_eq!(bad("[]"), "event_invalid{wrong_type}");
}

// ------------------------------------------------------------------ schemas

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
fn definitions_and_results_conform_to_the_published_schemas() {
    let w = world();
    let store = ResultStore::new();
    for (def, measure, tool) in [
        (diameter_command_def().unwrap(), Measure::Diameter, EVAL_DIAMETER_MCP),
        (radius_command_def().unwrap(), Measure::Radius, EVAL_RADIUS_MCP),
    ] {
        let params = Compiled::compile(&def.params_schema).unwrap();
        let result = Compiled::compile(&def.result_schema).unwrap();
        for (feature, variant) in [
            ("f-circle2", "ls_circle2"),
            ("f-circle3", "ls_circle3"),
            ("f-cylinder", "ls_cylinder"),
            ("f-sphere", "ls_sphere"),
        ] {
            let raw = format!(r#"{{"char":"dia-1","feature":"{feature}","run":"run-1","variant":"{variant}"}}"#);
            let p = fid_canon::parse(raw.as_bytes()).unwrap();
            assert!(params.evaluate(&p, &NoHooks).is_ok(), "{raw}");
            let ev = invoke_mcp(&store, &w.inputs(), H, tool, raw.as_bytes()).unwrap();
            assert_eq!(ev.value().measure(), measure);
            if let Err(f) = result.evaluate(&json(&ev), &NoHooks) {
                panic!("{tool} {feature}: {} at {} ({})", f.keyword, f.pointer, f.message);
            }
        }
        for bad in [
            r#"{"char":"dia-1","feature":"f-1"}"#,
            r#"{"char":"dia-1","feature":"f-1","run":"r","gauge":{"from_apex":0}}"#,
            r#"{"char":"dia-1","feature":"f-1","run":"r","extra":true}"#,
        ] {
            assert!(params.evaluate(&fid_canon::parse(bad.as_bytes()).unwrap(), &NoHooks).is_err(), "{bad}");
        }
        // The schema is not vacuous: a result naming another fit is refused.
        let ev = invoke_mcp(
            &store,
            &w.inputs(),
            H,
            tool,
            br#"{"char":"dia-1","feature":"f-sphere","run":"r","variant":"ls_sphere"}"#,
        )
        .unwrap();
        let mut v = json(&ev);
        if let CValue::Obj(o) = &mut v {
            if let Some(CValue::Obj(f)) = o.get_mut("fit") {
                f.insert("algorithm".to_owned(), CValue::Str("fit.sphere.mc@1.0.0".to_owned()));
            }
        }
        assert!(result.evaluate(&v, &NoHooks).is_err());
    }
}
