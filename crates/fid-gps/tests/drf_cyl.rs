//! The cylindrical path of spec node GDT.DRF.BUILD (`fid_gps::drf::cyl_sim`): DRFs from the
//! measured points of planar faces and external cylindrical bosses, each simulated in precedence
//! order (tangential planes; minimum circumscribed cylinders of `KRN.ENV.MCC.CYL` through the
//! `Fit` trait's gate). AC-2 (plane A, axis B perpendicular to it: B {tx, ty}, free {rz}), AC-4
//! (a single primary axis: free {tz, rz}), the other precedences of an axis datum, the seeded
//! precedence faults, and the typed refusals (internal cylinder, oval cross-section, an
//! orientation the envelope cannot hold, flagged fits). Expected frames, DoF tables, radii and
//! contacts come from the independent exact reference `fixtures/gdt/drf/cyl/make_cases.py`
//! (`cases.json`); refusals are hand-labelled. Synthetic data only.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::CValue;
use fid_fit::contract::{Centred, Constraint, FitParams, Method, PointsView};
use fid_fit::contract::{Fit, FittedFeature};
use fid_fit::mcc_cyl::MccCylinderFit;
use fid_fit::mcc_cyl::{fit_mcc_cylinder, CrossOutcome, MccCylinder};
use fid_gps::dmt::{AlgId, DmtKey};
use fid_gps::drf::build::build_drf;
use fid_gps::drf::build::{DatumKind, Dof, DofSet, DrfError, SimGeometry};
use fid_gps::drf::cyl_sim::{
    accept_cylinder, build_measured_drf, cylinder_params, mcc_algorithm_id, simulate_cylinder, CylinderSide,
    MeasuredFeature, MeasuredKind, Simulators, MCC_SEED,
};
use fid_gps::drf::plane_sim::{MeasuredDrf, SimError};
use std::collections::BTreeMap;
use std::path::Path;

/// Origin, axes, axis points and directions within 1e-12 m and 1e-12 rad (AC-1's tolerance).
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

fn ints(v: &CValue) -> Vec<u32> {
    arr(v)
        .iter()
        .map(|x| match x {
            CValue::Num(n) => *n as u32,
            _ => panic!("integer"),
        })
        .collect()
}

#[derive(Clone)]
struct Datum {
    label: String,
    kind: MeasuredKind,
    xyz: Vec<f64>,
    nominal: [f64; 3],
}

#[derive(Clone)]
struct Case {
    name: String,
    algs: Vec<AlgId>,
    row: DmtKey,
    datums: Vec<Datum>,
    expect: CValue,
}

fn cases() -> Vec<Case> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/gdt/drf/cyl/cases.json");
    let doc = fid_canon::parse(&std::fs::read(path).unwrap()).unwrap();
    assert_eq!(st(&obj(&doc)["schema"]), "fid.test-drf-cyl/1");
    arr(&obj(&doc)["cases"])
        .iter()
        .map(|c| {
            let c = obj(c);
            Case {
                name: st(&c["name"]).to_owned(),
                algs: arr(&c["algs"]).iter().map(|a| AlgId::parse(st(a)).unwrap()).collect(),
                row: DmtKey::parse(st(&c["row"])).unwrap(),
                datums: arr(&c["datums"])
                    .iter()
                    .map(|d| {
                        let d = obj(d);
                        let kind = match st(&d["kind"]) {
                            "plane" => MeasuredKind::Plane,
                            "cylinder" => MeasuredKind::Cylinder(CylinderSide::parse(st(&d["side"])).unwrap()),
                            k => panic!("{k}"),
                        };
                        Datum {
                            label: st(&d["label"]).to_owned(),
                            kind,
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

fn case(name: &str) -> Case {
    cases().into_iter().find(|c| c.name == name).unwrap()
}

fn sims(c: &Case) -> Simulators {
    let algs: Vec<&AlgId> = c.algs.iter().collect();
    Simulators::from_association(&algs).unwrap()
}

fn run_with(c: &Case, s: &Simulators) -> Result<MeasuredDrf, SimError> {
    let features: Vec<MeasuredFeature<'_>> = c
        .datums
        .iter()
        .map(|d| MeasuredFeature { label: &d.label, xyz: &d.xyz, nominal: d.nominal, kind: d.kind })
        .collect();
    build_measured_drf(&features, s, &c.row)
}

fn run(c: &Case) -> Result<MeasuredDrf, SimError> {
    run_with(c, &sims(c))
}

fn close(a: [f64; 3], b: [f64; 3], tol: f64) -> bool {
    (0..3).all(|i| (a[i] - b[i]).abs() <= tol)
}

fn check_ok(c: &Case, m: &MeasuredDrf) {
    let e = obj(&c.expect);
    let drf = m.drf();
    assert!(close(drf.origin(), v3(&e["origin"]), TOL), "{}: origin {:?}", c.name, drf.origin());
    for (k, ax) in arr(&e["axes"]).iter().enumerate() {
        assert!(close(drf.axes()[k], v3(ax), TOL), "{}: axis {k} {:?}", c.name, drf.axes()[k]);
    }
    let dofs = arr(&e["dofs"]);
    assert_eq!(drf.datums().len(), dofs.len(), "{}", c.name);
    let rows = arr(&e["datums"]);
    for (k, (row, want)) in drf.datums().iter().zip(dofs).enumerate() {
        assert_eq!(row.constrained, dofset(want), "{}: {}", c.name, row.label);
        assert_eq!(row.dmt_row, c.row);
        let r = obj(&rows[k]);
        let ev = &m.evidence()[k];
        let sim = &m.datums()[k];
        assert!((ev.objective - num(&r["objective"])).abs() <= TOL, "{}: objective {k} {}", c.name, ev.objective);
        assert_eq!(ev.optimality, st(&r["optimality"]), "{}: {k}", c.name);
        assert_eq!(ev.orientation.as_str(), st(&r["orientation"]), "{}: {k}", c.name);
        assert_eq!(ev.points as usize, c.datums[k].xyz.len() / 3);
        assert!(!ev.local_optimum_only);
        match st(&r["kind"]) {
            "plane" => {
                assert_eq!(row.kind, DatumKind::Plane);
                assert_eq!(row.alg, c.algs[0]);
                assert!(ev.seed.is_none() && ev.cross_check.is_none());
            }
            "cylinder" => {
                assert_eq!(row.kind, DatumKind::Axis);
                assert_eq!(row.alg.as_str(), mcc_algorithm_id());
                assert_eq!(ev.contacts, ints(&r["contacts"]), "{}: contacts {k}", c.name);
                assert!(ev.certificate.is_some(), "{}: certified", c.name);
                assert_eq!(ev.seed, Some(MCC_SEED));
                assert!(ev.max_violation <= fid_fit::mcc_cyl::INSIDE_TOL);
                let free = ev.orientation.as_str() == "free";
                assert_eq!(ev.cross_check, free.then_some("passed"), "{}", c.name);
                assert_eq!(ev.cross_check_work.is_some(), free, "{}", c.name);
                let SimGeometry::Axis { point, direction } = sim.geometry else { panic!("{}: axis", c.name) };
                assert!(close(direction, v3(&r["direction"]), TOL), "{}: direction {k} {direction:?}", c.name);
                assert!(close(point, v3(&r["point"]), TOL), "{}: point {k} {point:?}", c.name);
            }
            other => panic!("{other}"),
        }
    }
    assert_eq!(drf.free(), dofset(&e["free"]), "{}", c.name);
}

#[test]
fn every_case_matches_the_reference() {
    let all = cases();
    assert!(all.len() >= 15);
    for c in &all {
        let e = obj(&c.expect);
        let got = run(c);
        if let Some(code) = e.get("error") {
            let err = got.err().unwrap_or_else(|| panic!("{}: expected a refusal", c.name));
            assert_eq!(err.code(), st(code), "{}: {err}", c.name);
            match &err {
                SimError::Drf(DrfError::DegenerateDatumPair { first, second }) => {
                    assert_eq!(first, st(&e["first"]), "{}", c.name);
                    assert_eq!(second, st(&e["second"]), "{}", c.name);
                }
                SimError::Fit { label, code } => {
                    assert_eq!(label, st(&e["label"]), "{}", c.name);
                    assert_eq!(*code, st(&e["code"]), "{}", c.name);
                }
                SimError::UnsupportedDatumType { label }
                | SimError::OrientationUnsupported { label }
                | SimError::ObliqueDatum { label } => assert_eq!(label, st(&e["label"]), "{}", c.name),
                other => panic!("{}: unexpected {other}", c.name),
            }
            continue;
        }
        let m = got.unwrap_or_else(|err| panic!("{}: {err}", c.name));
        check_ok(c, &m);
    }
}

/// AC-2: plane A, then boss B perpendicular to it: A {tz, rx, ry}, B {tx, ty}, free {rz}.
/// B is held perpendicular to A (direction fixed at the frame's z: the global envelope of the
/// convex problem), not fitted freely.
#[test]
fn ac2_plane_then_perpendicular_axis() {
    let c = case("ab");
    let m = run(&c).unwrap();
    let d = m.drf().datums();
    assert_eq!(d[0].constrained, DofSet::of(&[Dof::Tz, Dof::Rx, Dof::Ry]));
    assert_eq!(d[1].constrained, DofSet::of(&[Dof::Tx, Dof::Ty]));
    assert_eq!(m.drf().free(), DofSet::of(&[Dof::Rz]));
    assert_eq!(m.evidence()[1].optimality, "global");
    // Seeded fault: B's free envelope (tilted 3e-4 rad from A's normal) would put the origin
    // elsewhere; the build matches the held axis only.
    let wrong = v3(&obj(&c.expect)["free_axis_origin"]);
    assert!(!close(m.drf().origin(), wrong, 1e-9));
}

/// AC-4: a single primary axis constrains {tx, ty, rx, ry}; free {tz, rz}.
#[test]
fn ac4_single_primary_axis() {
    let m = run(&case("b_axis")).unwrap();
    assert_eq!(m.drf().datums()[0].constrained, DofSet::of(&[Dof::Tx, Dof::Ty, Dof::Rx, Dof::Ry]));
    assert_eq!(m.drf().free(), DofSet::of(&[Dof::Tz, Dof::Rz]));
    assert_eq!(m.evidence()[0].optimality, "strict_local_cross_checked");
    assert_eq!(m.evidence()[0].contacts.len(), 5);
}

/// Seeded faults, wrong precedence involving cylinders: A|B against B|A (plane primary or axis
/// primary) and A|B|C against A|C|B and B|A|C (which boss clocks, which locates). Each order
/// builds its own reference frame (`every_case_matches_the_reference`) and the frames differ
/// by far more than the tolerance, so code that sorted, reversed or ignored the order fails.
#[test]
fn seeded_faults_wrong_precedence_with_cylinders_are_visible() {
    for (good, faults) in [("ab", &["ba"][..]), ("abc", &["acb", "bac"][..])] {
        let built = run(&case(good)).unwrap();
        for f in faults {
            let fc = case(f);
            let e = obj(&fc.expect);
            let axes = arr(&e["axes"]);
            let differs = (0..3).any(|k| !close(built.drf().axes()[k], v3(&axes[k]), 1e-6))
                || !close(built.drf().origin(), v3(&e["origin"]), 1e-6);
            assert!(differs, "{f} is indistinguishable from {good}");
            let wrong = run(&fc).unwrap();
            assert_eq!(wrong.drf().datums()[0].label, fc.datums[0].label);
            assert_ne!(wrong.drf(), built.drf());
        }
    }
    // The DoF tables of the axis-primary orders.
    let ba = run(&case("ba")).unwrap();
    assert_eq!(ba.drf().datums()[1].constrained, DofSet::of(&[Dof::Tz]));
    assert_eq!(ba.drf().free(), DofSet::of(&[Dof::Rz]));
    let bs = run(&case("bs")).unwrap();
    assert_eq!(bs.drf().datums()[1].constrained, DofSet::of(&[Dof::Rz]));
    assert_eq!(bs.drf().free(), DofSet::of(&[Dof::Tz]));
}

/// D0: the same inputs give bit-identical frames and evidence.
#[test]
fn deterministic() {
    let c = case("bac");
    assert_eq!(run(&c).unwrap(), run(&c).unwrap());
}

/// A cylinder needs the chain's cylinder simulator, a plane its plane simulator
/// (`unsupported_simulator{none}`), and an internal cylinder is refused even before that.
#[test]
fn every_datum_needs_its_simulator() {
    let c = case("ab");
    let planes_only = Simulators::from_association(&[&c.algs[0]]).unwrap();
    assert_eq!(run_with(&c, &planes_only).unwrap_err().to_string(), "unsupported_simulator{none}");
    let cyl_only = Simulators::from_association(&[&c.algs[1]]).unwrap();
    assert_eq!(run_with(&c, &cyl_only).unwrap_err().to_string(), "unsupported_simulator{none}");
    assert!(run_with(&case("b_axis"), &cyl_only).is_ok());
    assert_eq!(run_with(&case("a_h_internal"), &planes_only).unwrap_err().to_string(), "unsupported_datum_type_p1a{H}");
    // A non-unit cylinder nominal is refused like a plane's.
    let mut c = case("b_axis");
    c.datums[0].nominal = [0.0, 0.0, 2.0];
    assert_eq!(run(&c).unwrap_err().to_string(), "nominal_invalid{B}");
}

/// Oval cross-sections fail closed in `KRN.ENV.MCC.CYL` (a degenerate active set: four rows
/// for five parameters free, two for three fixed); the DRF surfaces it typed, as primary and
/// as a secondary held perpendicular to A, and never builds a frame from it.
#[test]
fn an_oval_boss_is_refused_typed() {
    assert_eq!(run(&case("o_oval")).unwrap_err().to_string(), "simulation_failed{O:solver}");
    assert_eq!(run(&case("a_o_oval")).unwrap_err().to_string(), "simulation_failed{O:solver}");
}

fn envelope(xyz: &[f64], fixed: Option<[f64; 3]>) -> (MccCylinder, f64) {
    let pts = PointsView::new(xyz, None).unwrap();
    let mut fp = FitParams::new(Method::MinimumCircumscribed);
    if let Some(d) = fixed {
        fp.constraints.push(Constraint::FixedAxisDirection(d));
    }
    let m = fit_mcc_cylinder(&pts, &fp, Some(MCC_SEED)).unwrap();
    (m, Centred::new(&pts).unwrap().map().scale())
}

/// The gate: an envelope is a datum only through the `Fit` trait's refusal and the re-check.
/// On boss B's real record, any flag, an unchecked or failed cross-check, or an optimality that
/// does not match the orientation is `flagged_fit{label}`; the clean record passes.
#[test]
fn a_flagged_or_unchecked_envelope_is_never_a_datum() {
    let b = case("b_axis").datums[0].clone();
    let z = run(&case("ab")).unwrap().drf().axes()[2];
    for fixed in [None, Some(z)] {
        let (m, scale) = envelope(&b.xyz, fixed);
        let is_fixed = fixed.is_some();
        let (_, ev) = accept_cylinder("B", &m, scale, is_fixed).unwrap();
        assert_eq!(ev.optimality, if is_fixed { "global" } else { "strict_local_cross_checked" });
        let refused = |m: &MccCylinder, f: bool| accept_cylinder("B", m, scale, f).unwrap_err().to_string();
        let mut f = m.clone();
        f.local_optimum_only = true;
        assert_eq!(refused(&f, is_fixed), "flagged_fit{B}");
        let mut f = m.clone();
        f.suspect_axis = true;
        assert_eq!(refused(&f, is_fixed), "flagged_fit{B}");
        let mut f = m.clone();
        f.certificate.set_local_optimum_only().unwrap();
        assert_eq!(refused(&f, is_fixed), "flagged_fit{B}");
        if !is_fixed {
            // A strict local minimum without a passed global cross-check (StrictLocal
            // unverified) is never accepted.
            let mut f = m.clone();
            f.cross_check_passed = false;
            assert_eq!(refused(&f, false), "flagged_fit{B}");
            let mut f = m.clone();
            f.cross_check = None;
            assert_eq!(refused(&f, false), "flagged_fit{B}");
            for o in [CrossOutcome::Better, CrossOutcome::Inconclusive] {
                let mut f = m.clone();
                if let Some(c) = f.cross_check.as_mut() {
                    c.outcome = o;
                }
                assert_eq!(refused(&f, false), "flagged_fit{B}", "{o:?}");
            }
        }
        assert_eq!(refused(&m, !is_fixed), "flagged_fit{B}");
    }
}

/// The copied sequence cannot drift from the trait: for every cylinder of every built case,
/// `MccCylinderFit.fit` on the same points, parameters (free, or the direction the frame held)
/// and seed equals the accepted contract result, and that result is the datum's axis.
#[test]
fn the_accepted_envelope_is_the_fit_traits_result() {
    let mut n = 0;
    for c in cases().iter().filter(|c| !obj(&c.expect).contains_key("error")) {
        let m = run(c).unwrap();
        for (k, d) in c.datums.iter().enumerate() {
            let MeasuredKind::Cylinder(_) = d.kind else { continue };
            let fixed = if k == 0 {
                None
            } else {
                // Every cylinder after the primary in these cases is held at the prefix's z,
                // signed by its nominal.
                let z = build_drf(&m.datums()[..k]).unwrap().axes()[2];
                let s = if (0..3).map(|i| z[i] * d.nominal[i]).sum::<f64>() < 0.0 { -1.0 } else { 1.0 };
                Some([s * z[0], s * z[1], s * z[2]])
            };
            assert_eq!(m.evidence()[k].orientation.as_str(), if fixed.is_some() { "fixed" } else { "free" });
            let pts = PointsView::new(&d.xyz, None).unwrap();
            let via_trait = MccCylinderFit.fit(&pts, &cylinder_params(fixed), Some(MCC_SEED)).unwrap();
            let (accepted, ev) = simulate_cylinder(&d.label, &d.xyz, fixed).unwrap();
            assert_eq!(accepted, via_trait, "{}: {}", c.name, d.label);
            assert_eq!(ev, m.evidence()[k], "{}: {}", c.name, d.label);
            let FittedFeature::Cylinder { p, d: dir, .. } = via_trait.feature else { panic!("cylinder") };
            let SimGeometry::Axis { point, direction } = m.datums()[k].geometry else { panic!("axis") };
            assert_eq!(point, p, "{}", c.name);
            assert!(direction == dir || direction == [-dir[0], -dir[1], -dir[2]], "{}", c.name);
            n += 1;
        }
    }
    assert!(n >= 10, "{n}");
}
