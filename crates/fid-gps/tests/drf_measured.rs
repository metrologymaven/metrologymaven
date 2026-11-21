//! The measured planar path of spec node GDT.DRF.BUILD (`fid_gps::drf::plane_sim`): DRFs from
//! the measured points of planar datum faces, each simulated by the certified tangential plane
//! of `fid-fit` in precedence order. Expected frames, DoF tables, objectives and refusals come
//! from the independent exact reference `fixtures/gdt/drf/measured/make_cases.py`
//! (`cases.json`); the seeded faults (wrong datum precedence) are checked to be visible.
//! Synthetic data only.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::CValue;
use fid_gps::dmt::{AlgId, DmtKey};
use fid_gps::drf::build::{Dof, DofSet};
use fid_gps::drf::plane_sim::{build_measured_planar_drf, MeasuredDrf, MeasuredPlane, SimError, SIDE_TOL};
use std::collections::BTreeMap;
use std::path::Path;

/// Origin and axes within 1e-12 m and 1e-12 rad of the reference (AC-1).
const TOL: f64 = 1e-12;

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

fn num(v: &CValue) -> f64 {
    st(v).parse::<f64>().unwrap()
}

fn v3(v: &CValue) -> [f64; 3] {
    let a = arr(v);
    [num(&a[0]), num(&a[1]), num(&a[2])]
}

fn dofset(v: &CValue) -> DofSet {
    DofSet::of(&arr(v).iter().map(|n| Dof::parse(st(n)).unwrap()).collect::<Vec<_>>())
}

struct Datum {
    label: String,
    xyz: Vec<f64>,
    nominal: [f64; 3],
}

struct Case {
    name: String,
    alg: AlgId,
    row: DmtKey,
    datums: Vec<Datum>,
    expect: CValue,
}

fn cases() -> Vec<Case> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/gdt/drf/measured/cases.json");
    let doc = fid_canon::parse(&std::fs::read(path).unwrap()).unwrap();
    assert_eq!(st(&obj(&doc)["schema"]), "fid.test-drf-measured/1");
    arr(&obj(&doc)["cases"])
        .iter()
        .map(|c| {
            let c = obj(c);
            Case {
                name: st(&c["name"]).to_owned(),
                alg: AlgId::parse(st(&c["alg"])).unwrap(),
                row: DmtKey::parse(st(&c["row"])).unwrap(),
                datums: arr(&c["datums"])
                    .iter()
                    .map(|d| {
                        let d = obj(d);
                        Datum {
                            label: st(&d["label"]).to_owned(),
                            xyz: arr(&d["xyz"]).iter().map(num).collect(),
                            nominal: v3(&d["nominal"]),
                        }
                    })
                    .collect(),
                expect: c["expect"].clone(),
            }
        })
        .collect()
}

fn run(c: &Case) -> Result<MeasuredDrf, SimError> {
    let planes: Vec<MeasuredPlane<'_>> =
        c.datums.iter().map(|d| MeasuredPlane { label: &d.label, xyz: &d.xyz, nominal_outward: d.nominal }).collect();
    build_measured_planar_drf(&planes, &c.alg, &c.row)
}

fn close(a: [f64; 3], b: [f64; 3], tol: f64) -> bool {
    (0..3).all(|i| (a[i] - b[i]).abs() <= tol)
}

fn case(name: &str) -> Case {
    cases().into_iter().find(|c| c.name == name).unwrap()
}

#[test]
fn every_case_matches_the_reference() {
    let all = cases();
    assert!(all.len() >= 10);
    for c in &all {
        let e = obj(&c.expect);
        let got = run(c);
        if let Some(code) = e.get("error") {
            let err = got.err().unwrap_or_else(|| panic!("{}: expected a refusal", c.name));
            assert_eq!(err.code(), st(code), "{}: {err}", c.name);
            match &err {
                SimError::Drf(fid_gps::drf::build::DrfError::DegenerateDatumPair { first, second }) => {
                    assert_eq!(first, st(&e["first"]), "{}", c.name);
                    assert_eq!(second, st(&e["second"]), "{}", c.name);
                }
                SimError::ObliqueDatum { label } => assert_eq!(label, st(&e["label"]), "{}", c.name),
                SimError::Fit { label, code } => {
                    assert_eq!(label, st(&e["label"]), "{}", c.name);
                    assert_eq!(*code, st(&e["code"]), "{}", c.name);
                }
                SimError::UnsupportedSimulator { alg } => assert_eq!(alg, c.alg.as_str()),
                other => panic!("{}: unexpected {other}", c.name),
            }
            continue;
        }
        let m = got.unwrap_or_else(|err| panic!("{}: {err}", c.name));
        let drf = m.drf();
        assert!(close(drf.origin(), v3(&e["origin"]), TOL), "{}: origin {:?}", c.name, drf.origin());
        let axes = arr(&e["axes"]);
        for (k, ax) in axes.iter().enumerate() {
            assert!(close(drf.axes()[k], v3(ax), TOL), "{}: axis {k} {:?}", c.name, drf.axes()[k]);
        }
        let dofs = arr(&e["dofs"]);
        assert_eq!(drf.datums().len(), dofs.len(), "{}", c.name);
        for (row, want) in drf.datums().iter().zip(dofs) {
            assert_eq!(row.constrained, dofset(want), "{}: {}", c.name, row.label);
            assert_eq!(row.alg, c.alg);
            assert_eq!(row.dmt_row, c.row);
        }
        assert_eq!(drf.free(), dofset(&e["free"]), "{}", c.name);
        let objectives = arr(&e["objectives"]);
        for (k, ev) in m.evidence().iter().enumerate() {
            assert!((ev.objective - num(&objectives[k])).abs() <= TOL, "{}: objective {k} {}", c.name, ev.objective);
            assert_eq!(ev.optimality, st(&arr(&e["optimality"])[k]), "{}", c.name);
            assert_eq!(ev.orientation.as_str(), st(&arr(&e["orientation"])[k]), "{}", c.name);
            assert!(ev.max_violation <= SIDE_TOL);
            assert_eq!(ev.points as usize, c.datums[k].xyz.len() / 3);
            // Results are certified only: a free normal carries its certificate.
            assert_eq!(ev.certificate.is_some(), ev.optimality == "strict_local", "{}", c.name);
            assert!(ev.candidates >= 1);
        }
    }
}

/// Seeded fault, wrong datum precedence: the faces of `abc_minimax` taken in the order B, A, C
/// give another frame. The reference frames differ by far more than the tolerance, and the
/// build of each order matches its own reference only, so code that reordered or ignored the
/// precedence fails `every_case_matches_the_reference`.
#[test]
fn seeded_fault_wrong_precedence_is_visible() {
    let abc = case("abc_minimax");
    let bac = case("bac_minimax");
    let acb = case("acb_minimax");
    let built = run(&abc).unwrap();
    for fault in [&bac, &acb] {
        let e = obj(&fault.expect);
        let axes = arr(&e["axes"]);
        let differs = (0..3).any(|k| !close(built.drf().axes()[k], v3(&axes[k]), 1e-6))
            || !close(built.drf().origin(), v3(&e["origin"]), 1e-9);
        assert!(differs, "{} is indistinguishable from abc_minimax", fault.name);
    }
    // The same faces in the fault order really build the fault's frame, not A|B|C's.
    let wrong = run(&bac).unwrap();
    assert!(!close(wrong.drf().axes()[2], built.drf().axes()[2], 1e-6));
    assert_eq!(wrong.drf().datums()[0].label, "B");
}

/// D0: the same inputs give bit-identical frames and evidence.
#[test]
fn deterministic() {
    let c = case("abc_l1");
    assert_eq!(run(&c).unwrap(), run(&c).unwrap());
}

/// Nominals must be unit; a NaN coordinate is refused by `fid-fit`'s point check.
#[test]
fn bad_inputs_are_refused() {
    let mut c = case("a_minimax");
    c.datums[0].nominal = [0.0, 0.0, 2.0];
    assert_eq!(run(&c).unwrap_err().code(), "nominal_invalid");
    c.datums[0].nominal = [f64::NAN, 0.0, 1.0];
    assert_eq!(run(&c).unwrap_err().code(), "nominal_invalid");
    let mut c = case("a_minimax");
    c.datums[0].xyz[4] = f64::NAN;
    let e = run(&c).unwrap_err();
    assert_eq!(e.code(), "simulation_failed");
    assert_eq!(e.to_string(), "simulation_failed{A:non_finite}");
    let c = Case { datums: Vec::new(), ..case("a_minimax") };
    assert_eq!(run(&c).unwrap_err().code(), "no_datums");
}

/// The DMT table inputs of the command tests load, and their association step names the
/// simulator the measured path recognises (or, for `minzone`, refuses).
#[test]
fn frame_tables_load_and_name_their_simulator() {
    use fid_gps::dmt::{DmtTable, OperatorKind};
    use fid_gps::drf::plane_sim::variant_of;
    for (name, known) in [("minimax", true), ("l1", true), ("l2", true), ("minzone", false)] {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join(format!("../../fixtures/gdt/drf/measured/frame_table_{name}.json"));
        let table = DmtTable::load(&std::fs::read(path).unwrap()).unwrap();
        let row = table.get("dmt:asme/datum_frame/rmb@1").unwrap();
        assert_eq!(row.modifiers(), ["rmb".to_owned()]);
        let chain = row.variant("planar").unwrap();
        let assoc: Vec<_> = chain.steps().iter().filter(|s| s.kind() == OperatorKind::Association).collect();
        assert_eq!(assoc.len(), 1);
        assert_eq!(variant_of(assoc[0].alg()).is_some(), known, "{name}");
    }
}
