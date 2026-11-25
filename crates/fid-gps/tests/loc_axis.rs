//! Spec node GDT.LOC.DERIV.AXIS (`fid_gps::loc::derive::axis`): AC-1 (the least-squares axis
//! of the tilted cylinder `fixtures/gdt/loc/axis_tilted_cyl.json` within 1e-12 rad and 1e-12 m,
//! extent 30 mm), AC-2 (the direction follows the nominal hemisphere), AC-3 (the cone of
//! `fixtures/gdt/loc/axis_cone.json` within 1e-12) and AC-4 (a fit stub flagging
//! `local_optimum_only` gives a flagged element), plus the error paths of the pinned procedure.
//! Expected axes come from the independent reference `fixtures/gdt/loc/make_axis_fixtures.py`
//! (the generating geometry); the extent and error cases below are hand-labelled from the
//! leaf's rules. AC-5 (the ORS-GDT derived-axis cases) waits for `UNC.ORS.GDT`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::CValue;
use fid_gps::dmt::{AlgId, Operator, OperatorChain, OperatorKind, Param};
use fid_gps::loc::derive::axis::{
    associate, axis_from_fit, derive_axis_with_fit, supported_fits, AxialFit, AXIS_REDUCTION_ALG, FIT_SEED,
};
use fid_gps::loc::derive::{
    derive, Certificate, DerivationReason, DeriveCall, DeriveError, DeriveFlag, DeriveRegistry, DeriveRequest,
    DerivedElement, FeatureKind, ImplError, Optimality, Produced,
};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

const LS_CYL: &str = "fit.cylinder.ls@1.0.0";
const LS_CONE: &str = "fit.cone.ls@1.0.0";
const MC_CYL: &str = "fit.cylinder.mc@1.0.0";

// ------------------------------------------------------------------ helpers

fn fixture(name: &str) -> CValue {
    let path: PathBuf = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/gdt/loc").join(name);
    fid_canon::parse(&std::fs::read(&path).unwrap()).unwrap()
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

fn hexf(v: &CValue) -> f64 {
    let s = st(v);
    assert!(s.len() == 18 && s.starts_with("0x"), "{s}");
    f64::from_bits(u64::from_str_radix(&s[2..], 16).unwrap())
}

fn p3(v: &CValue) -> [f64; 3] {
    let a = arr(v);
    assert_eq!(a.len(), 3);
    [hexf(&a[0]), hexf(&a[1]), hexf(&a[2])]
}

fn alg(s: &str) -> AlgId {
    AlgId::parse(s).unwrap()
}

fn op(kind: OperatorKind, a: &str) -> Operator {
    Operator::new(kind, alg(a), BTreeMap::new()).unwrap()
}

/// partition, then the given association steps, the axis reduction and an evaluation.
fn chain_with(assocs: &[&str]) -> OperatorChain {
    let mut steps = vec![op(OperatorKind::Partition, "syn.partition.whole_feature@1.0.0")];
    for a in assocs {
        steps.push(op(OperatorKind::Association, a));
    }
    steps.push(op(OperatorKind::Reduction, AXIS_REDUCTION_ALG));
    steps.push(op(OperatorKind::Evaluation, "syn.evaluation.zone@1.0.0"));
    OperatorChain::new("default", steps).unwrap()
}

fn chain(fit: &str) -> OperatorChain {
    chain_with(&[fit])
}

struct Case {
    name: String,
    nominal: [f64; 3],
    point: [f64; 3],
    direction: [f64; 3],
    extent: f64,
}

struct Fixture {
    kind: FeatureKind,
    points: Vec<[f64; 3]>,
    cases: Vec<Case>,
    tol_len: f64,
    tol_ang: f64,
}

fn load(name: &str) -> Fixture {
    let v = fixture(name);
    let o = obj(&v);
    let tol = obj(&o["tolerance"]);
    let cases = arr(&o["cases"])
        .iter()
        .map(|c| {
            let c = obj(c);
            let e = obj(&c["expected"]);
            Case {
                name: st(&c["name"]).to_owned(),
                nominal: p3(&c["nominal_direction"]),
                point: p3(&e["point"]),
                direction: p3(&e["direction"]),
                extent: hexf(&e["extent"]),
            }
        })
        .collect();
    Fixture {
        kind: FeatureKind::parse(st(&o["feature_kind"])).unwrap(),
        points: arr(&o["points"]).iter().map(p3).collect(),
        cases,
        tol_len: hexf(&tol["length_m"]),
        tol_ang: hexf(&tol["angle_rad"]),
    }
}

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn norm(a: [f64; 3]) -> f64 {
    (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt()
}

/// The angle between two unit vectors, accurate for small angles (2 asin(|a - b| / 2)).
fn angle(a: [f64; 3], b: [f64; 3]) -> f64 {
    2.0 * (norm(sub(a, b)) / 2.0).asin()
}

fn axis_of(e: &DerivedElement) -> ([f64; 3], [f64; 3], f64) {
    match e {
        DerivedElement::Axis(a) => (a.point(), a.direction(), a.extent()),
        other => panic!("not an axis: {other:?}"),
    }
}

fn request(points: &[[f64; 3]], kind: FeatureKind, nominal: Option<[f64; 3]>) -> DeriveRequest<'_> {
    DeriveRequest { points, feature_kind: kind, nominal_direction: nominal }
}

fn check_case(fx: &Fixture, c: &Case, fit: &str) {
    let req = request(&fx.points, fx.kind, Some(c.nominal));
    let d = DeriveRegistry::builtin().derive(&req, &chain(fit)).unwrap();
    assert_eq!(d.alg().as_str(), AXIS_REDUCTION_ALG, "{}", c.name);
    let (point, direction, extent) = axis_of(d.element());
    let ang = angle(direction, c.direction);
    let dist = norm(sub(point, c.point));
    assert!(ang <= fx.tol_ang, "{}: direction off by {ang:e} rad", c.name);
    assert!(dist <= fx.tol_len, "{}: point off by {dist:e} m", c.name);
    assert!((extent - c.extent).abs() <= fx.tol_len, "{}: extent {extent} vs {}", c.name, c.extent);
    // REQ-003: the direction is in the nominal hemisphere.
    assert!((direction[0] * c.nominal[0] + direction[1] * c.nominal[1]) + direction[2] * c.nominal[2] >= 0.0);
    // Least squares has no global certificate: the axis is always flagged (D-045).
    assert_eq!(d.optimality(), &Optimality::LocalOnly, "{}", c.name);
    assert!(d.flags().contains(&DeriveFlag::LocalOptimumOnly), "{}", c.name);
}

// ------------------------------------------------------------------ registration

#[test]
fn builtin_registers_the_axis_under_its_reduction() {
    let r = DeriveRegistry::builtin();
    assert_eq!(r.algs().map(|a| a.as_str().to_owned()).collect::<Vec<_>>(), vec![AXIS_REDUCTION_ALG.to_owned()]);
    assert!(r.get(&alg(AXIS_REDUCTION_ALG)).is_some());
    assert_eq!(supported_fits(), vec![LS_CONE, LS_CYL, MC_CYL]);
    assert_eq!(FIT_SEED, 0);
}

// ------------------------------------------------------------------ AC-1, AC-2: tilted cylinder

#[test]
fn ac1_ac2_tilted_cylinder_least_squares() {
    let fx = load("axis_tilted_cyl.json");
    assert_eq!(fx.kind, FeatureKind::Cylinder);
    assert_eq!(fx.points.len(), 48);
    assert_eq!(fx.cases.len(), 2);
    for c in &fx.cases {
        check_case(&fx, c, LS_CYL);
    }
    // AC-1's extent is 30 mm.
    assert!((fx.cases[0].extent - 0.03).abs() <= 1e-15);
}

#[test]
fn ac2_direction_flips_with_the_nominal() {
    let fx = load("axis_tilted_cyl.json");
    let reg = DeriveRegistry::builtin();
    let up = reg.derive(&request(&fx.points, fx.kind, Some([0.0, 0.0, 1.0])), &chain(LS_CYL)).unwrap();
    let down = reg.derive(&request(&fx.points, fx.kind, Some([0.0, 0.0, -1.0])), &chain(LS_CYL)).unwrap();
    let (pu, du, eu) = axis_of(up.element());
    let (pd, dd, ed) = axis_of(down.element());
    assert_eq!(dd, [-du[0], -du[1], -du[2]]);
    assert!(norm(sub(pu, pd)) <= 1e-15);
    assert!((eu - ed).abs() <= 1e-15);
}

#[test]
fn ac1_fit_algorithm_id_is_returned() {
    // REQ-001: the axis comes with the fit's algorithm ID (the chain's association).
    let fx = load("axis_tilted_cyl.json");
    let req = request(&fx.points, fx.kind, Some([0.0, 0.0, 1.0]));
    let ch = chain(LS_CYL);
    let red = ch.steps().iter().find(|s| s.kind() == OperatorKind::Reduction).unwrap();
    let call = DeriveCall { request: &req, chain: &ch, reduction: red };
    let (fit, produced) = derive_axis_with_fit(&call).unwrap();
    assert_eq!(fit.alg().as_str(), LS_CYL);
    assert_eq!(produced.optimality(), &Optimality::LocalOnly);
    assert_eq!(associate(&call).unwrap(), fit);
}

#[test]
fn derivation_is_repeatable_bit_for_bit() {
    let fx = load("axis_tilted_cyl.json");
    let req = request(&fx.points, fx.kind, Some([0.0, 0.0, 1.0]));
    let a = DeriveRegistry::builtin().derive(&req, &chain(LS_CYL)).unwrap();
    let b = DeriveRegistry::builtin().derive(&req, &chain(LS_CYL)).unwrap();
    assert_eq!(a.to_cvalue(), b.to_cvalue());
}

// ------------------------------------------------------------------ AC-3: cone

#[test]
fn ac3_cone_least_squares() {
    let fx = load("axis_cone.json");
    assert_eq!(fx.kind, FeatureKind::Cone);
    assert_eq!(fx.points.len(), 50);
    assert_eq!(fx.cases.len(), 2);
    for c in &fx.cases {
        check_case(&fx, c, LS_CONE);
    }
}

// ------------------------------------------------------------------ AC-4: local optimum flag

/// A fit stub that returns the generating axis of the tilted cylinder and flags it
/// `local_optimum_only`, run through the axis step and `derive()`.
fn stub_flagged(call: &DeriveCall<'_>) -> Result<Produced, ImplError> {
    let fx = load("axis_tilted_cyl.json");
    let c = &fx.cases[0];
    let fit = AxialFit::uncertified(alg("syn.association.stub_cylinder@1.0.0"), c.point, c.direction, true);
    axis_from_fit(call.request.points, call.request.nominal_direction, &fit)
}

/// The same stub without the flag: with no global certificate the axis is still flagged.
fn stub_unflagged(call: &DeriveCall<'_>) -> Result<Produced, ImplError> {
    let fx = load("axis_tilted_cyl.json");
    let c = &fx.cases[0];
    let fit = AxialFit::uncertified(alg("syn.association.stub_cylinder@1.0.0"), c.point, c.direction, false);
    axis_from_fit(call.request.points, call.request.nominal_direction, &fit)
}

#[test]
fn ac4_stub_flagging_local_optimum_only_carries_the_flag() {
    let fx = load("axis_tilted_cyl.json");
    let req = request(&fx.points, fx.kind, Some([0.0, 0.0, 1.0]));
    let ch = OperatorChain::new(
        "default",
        vec![
            op(OperatorKind::Association, "syn.association.stub_cylinder@1.0.0"),
            op(OperatorKind::Reduction, "syn.axis.stub@1.0.0"),
        ],
    )
    .unwrap();
    for f in [stub_flagged as fid_gps::loc::derive::DeriveFn, stub_unflagged] {
        let mut r = DeriveRegistry::new();
        r.register(alg("syn.axis.stub@1.0.0"), f).unwrap();
        let d = derive(&r, &req, &ch).unwrap();
        assert_eq!(d.optimality(), &Optimality::LocalOnly);
        assert_eq!(d.flags().iter().copied().collect::<Vec<_>>(), vec![DeriveFlag::LocalOptimumOnly]);
        let flags = obj(&d.to_cvalue())["flags"].clone();
        assert_eq!(flags, CValue::Arr(vec![CValue::Str("local_optimum_only".to_owned())]));
        let (point, direction, extent) = axis_of(d.element());
        let c = &fx.cases[0];
        assert!(norm(sub(point, c.point)) <= 1e-15);
        assert_eq!(direction, c.direction);
        assert!((extent - 0.03).abs() <= 1e-15);
    }
}

// ------------------------------------------------------------------ MCC: certified only on evidence

/// The tilted cylinder's points with their distance from the generating axis scaled by
/// `1 + 5e-4 * ((7 i mod 5) - 2)` (a radial form error of up to 0.1 %): fid-fit's envelope
/// does not certify on noise-free data (every point is a contact), so its test needs form.
fn wavy_cylinder(fx: &Fixture) -> Vec<[f64; 3]> {
    let (c0, d0) = (fx.cases[0].point, fx.cases[0].direction);
    fx.points
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let w = sub(*p, c0);
            let t = (w[0] * d0[0] + w[1] * d0[1]) + w[2] * d0[2];
            let e = 1.0 + 5e-4 * (((i * 7) % 5) as f64 - 2.0);
            [0, 1, 2].map(|k| c0[k] + t * d0[k] + (w[k] - t * d0[k]) * e)
        })
        .collect()
}

#[test]
fn minimum_circumscribed_cylinder_certifies_only_with_a_passed_cross_check() {
    let fx = load("axis_tilted_cyl.json");
    let ch = chain(MC_CYL);
    let red = ch.steps().iter().find(|s| s.kind() == OperatorKind::Reduction).unwrap();
    // Noise-free data: the envelope's certificate fails, so the derivation fails closed.
    let exact = request(&fx.points, fx.kind, Some([0.0, 0.0, 1.0]));
    let e = DeriveRegistry::builtin().derive(&exact, &ch).unwrap_err();
    assert_eq!(e, DeriveError::DerivationFailed { reason: DerivationReason::FitFailed });
    // With form error: a certified strict local minimum whose global cross-check passed.
    let pts = wavy_cylinder(&fx);
    let req = request(&pts, fx.kind, Some([0.0, 0.0, 1.0]));
    let call = DeriveCall { request: &req, chain: &ch, reduction: red };
    let fit = associate(&call).unwrap();
    assert!(!fit.local_optimum_only());
    assert_eq!(fit.optimality(), Optimality::Certified { cert: Certificate::GlobalFit { by: alg(MC_CYL) } });
    let d = DeriveRegistry::builtin().derive(&req, &ch).unwrap();
    assert_eq!(d.optimality(), &fit.optimality());
    assert!(d.flags().is_empty());
    // Sanity only (the envelope of the wavy data is not the generating cylinder): the axis is
    // near the generating one and the extent near 30 mm.
    let (_, direction, extent) = axis_of(d.element());
    assert!(angle(direction, fx.cases[0].direction) <= 1e-3);
    assert!((extent - 0.03).abs() <= 1e-4);
}

// ------------------------------------------------------------------ REQ-002: extent (hand-labelled)

#[test]
fn req2_extent_is_the_range_of_projections_and_the_point_its_midpoint() {
    // Axis along +x through (0, 1, 2); projections 3, -1, 5, -1, 0 (the minimum ties: first wins,
    // same value). Extent 6, midpoint t = 2, so the point is (2, 1, 2). Off-axis offsets do
    // not matter.
    let pts = [[3.0, 4.0, 2.0], [-1.0, 1.0, -7.0], [5.0, 1.0, 2.5], [-1.0, 0.0, 0.0], [0.0, 9.0, 9.0]];
    let fit = AxialFit::uncertified(alg(LS_CYL), [0.0, 1.0, 2.0], [1.0, 0.0, 0.0], false);
    let p = axis_from_fit(&pts, Some([0.3, 5.0, -2.0]), &fit).unwrap();
    let (point, direction, extent) = axis_of(p.element());
    assert_eq!((point, direction, extent), ([2.0, 1.0, 2.0], [1.0, 0.0, 0.0], 6.0));
    // A nominal in the other hemisphere flips the direction; the extent and point stay.
    let p = axis_from_fit(&pts, Some([-1.0, 0.0, 0.0]), &fit).unwrap();
    assert_eq!(axis_of(p.element()), ([2.0, 1.0, 2.0], [-1.0, 0.0, 0.0], 6.0));
    // A nominal perpendicular to the axis (dot 0) keeps it (non-negative); none keeps it too.
    let p = axis_from_fit(&pts, Some([0.0, 1.0, 0.0]), &fit).unwrap();
    assert_eq!(axis_of(p.element()).1, [1.0, 0.0, 0.0]);
    let p = axis_from_fit(&pts, None, &fit).unwrap();
    assert_eq!(axis_of(p.element()).1, [1.0, 0.0, 0.0]);
    // One point: extent 0 at that point's foot.
    let p = axis_from_fit(&[[4.0, -3.0, 8.0]], None, &fit).unwrap();
    assert_eq!(axis_of(p.element()), ([4.0, 1.0, 2.0], [1.0, 0.0, 0.0], 0.0));
    // No points.
    assert_eq!(axis_from_fit(&[], None, &fit).unwrap_err(), ImplError::Failed(DerivationReason::TooFewPoints));
    // A non-unit fit direction cannot make an axis.
    let bad = AxialFit::uncertified(alg(LS_CYL), [0.0; 3], [2.0, 0.0, 0.0], false);
    assert_eq!(axis_from_fit(&pts, None, &bad).unwrap_err(), ImplError::Failed(DerivationReason::FitFailed));
}

// ------------------------------------------------------------------ error paths

fn unsupported(alg_id: &str) -> DeriveError {
    DeriveError::Unsupported { alg: alg(alg_id) }
}

#[test]
fn error_unsupported_feature_kind_and_chain_shapes() {
    let fx = load("axis_tilted_cyl.json");
    let reg = DeriveRegistry::builtin();
    let nominal = Some([0.0, 0.0, 1.0]);
    // A width has no axis.
    let e = reg.derive(&request(&fx.points, FeatureKind::Width, nominal), &chain(LS_CYL)).unwrap_err();
    assert_eq!(e, unsupported(AXIS_REDUCTION_ALG));
    let req = request(&fx.points, FeatureKind::Cylinder, nominal);
    // No association, or two, before the reduction.
    assert_eq!(reg.derive(&req, &chain_with(&[])).unwrap_err(), unsupported(AXIS_REDUCTION_ALG));
    assert_eq!(reg.derive(&req, &chain_with(&[LS_CYL, MC_CYL])).unwrap_err(), unsupported(AXIS_REDUCTION_ALG));
    // An association after the reduction does not count.
    let after = OperatorChain::new(
        "default",
        vec![op(OperatorKind::Reduction, AXIS_REDUCTION_ALG), op(OperatorKind::Association, LS_CYL)],
    )
    .unwrap();
    assert_eq!(reg.derive(&req, &after).unwrap_err(), unsupported(AXIS_REDUCTION_ALG));
    // Fits this derivation does not run (not yet in fid-fit, or not axial).
    for f in ["fit.cylinder.mz@1.0.0", "fit.cylinder.mi@1.0.0", "fit.plane.ls@1.0.0", "fit.cylinder.ls@2.0.0"] {
        assert_eq!(reg.derive(&req, &chain(f)).unwrap_err(), unsupported(f), "{f}");
    }
    // A cone fit on a cylinder, a cylinder fit on a cone.
    assert_eq!(reg.derive(&req, &chain(LS_CONE)).unwrap_err(), unsupported(LS_CONE));
    let cone = request(&fx.points, FeatureKind::Cone, nominal);
    assert_eq!(reg.derive(&cone, &chain(LS_CYL)).unwrap_err(), unsupported(LS_CYL));
    assert_eq!(reg.derive(&cone, &chain(MC_CYL)).unwrap_err(), unsupported(MC_CYL));
}

#[test]
fn error_parameters_on_the_steps_are_unsupported() {
    let fx = load("axis_tilted_cyl.json");
    let req = request(&fx.points, FeatureKind::Cylinder, Some([0.0, 0.0, 1.0]));
    let params: BTreeMap<String, Param> = [("seed".to_owned(), Param::Int(7))].into_iter().collect();
    let with_assoc = OperatorChain::new(
        "default",
        vec![
            Operator::new(OperatorKind::Association, alg(LS_CYL), params.clone()).unwrap(),
            op(OperatorKind::Reduction, AXIS_REDUCTION_ALG),
        ],
    )
    .unwrap();
    assert_eq!(DeriveRegistry::builtin().derive(&req, &with_assoc).unwrap_err(), unsupported(LS_CYL));
    let with_red = OperatorChain::new(
        "default",
        vec![
            op(OperatorKind::Association, LS_CYL),
            Operator::new(OperatorKind::Reduction, alg(AXIS_REDUCTION_ALG), params).unwrap(),
        ],
    )
    .unwrap();
    assert_eq!(DeriveRegistry::builtin().derive(&req, &with_red).unwrap_err(), unsupported(AXIS_REDUCTION_ALG));
}

#[test]
fn error_fit_failure_is_derivation_failed_fit_failed() {
    let reg = DeriveRegistry::builtin();
    let failed = DeriveError::DerivationFailed { reason: DerivationReason::FitFailed };
    let nominal = Some([0.0, 0.0, 1.0]);
    // Too few points for a cylinder (fid-fit needs 5) and for a cone (6).
    let few = [[0.01, 0.0, 0.0], [0.0, 0.01, 0.001], [-0.01, 0.0, 0.002]];
    assert_eq!(reg.derive(&request(&few, FeatureKind::Cylinder, nominal), &chain(LS_CYL)).unwrap_err(), failed);
    assert_eq!(reg.derive(&request(&few, FeatureKind::Cone, nominal), &chain(LS_CONE)).unwrap_err(), failed);
    assert_eq!(reg.derive(&request(&few, FeatureKind::Cylinder, nominal), &chain(MC_CYL)).unwrap_err(), failed);
    // Collinear data.
    let line: Vec<[f64; 3]> = (0..12).map(|i| [0.0, 0.0, f64::from(i) * 0.001]).collect();
    assert_eq!(reg.derive(&request(&line, FeatureKind::Cylinder, nominal), &chain(LS_CYL)).unwrap_err(), failed);
    // No points.
    assert_eq!(reg.derive(&request(&[], FeatureKind::Cylinder, nominal), &chain(LS_CYL)).unwrap_err(), failed);
    // A subnormal coordinate is refused by fid-fit's point view.
    let fx = load("axis_tilted_cyl.json");
    let mut pts = fx.points.clone();
    pts[3][1] = f64::from_bits(1);
    assert_eq!(reg.derive(&request(&pts, FeatureKind::Cylinder, nominal), &chain(LS_CYL)).unwrap_err(), failed);
}

#[test]
fn error_non_finite_input_is_refused_before_the_fit() {
    let fx = load("axis_tilted_cyl.json");
    let mut pts = fx.points.clone();
    pts[5][2] = f64::NAN;
    let e = DeriveRegistry::builtin()
        .derive(&request(&pts, FeatureKind::Cylinder, Some([0.0, 0.0, 1.0])), &chain(LS_CYL))
        .unwrap_err();
    assert_eq!(e, DeriveError::NonFiniteInput { field: "points", index: Some(5) });
}
