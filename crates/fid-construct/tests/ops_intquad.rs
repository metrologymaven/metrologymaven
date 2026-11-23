//! Acceptance tests AC-1..AC-4 for `KRN.CONS.OPS.INTQUAD` (`fid_construct::ops_intquad`).
//! Uses `include_str!` so every test runs identically on x86-64, ARM64 and `wasm32-wasip1`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_construct::contract::{
    harness, run, ConstructError, ConstructParams, Construction, ElementGeom, ElementId, Inputs, Measure, QuantityKind,
    UndefinedReason,
};
use fid_construct::ops_intquad::{
    Branch, IntersectCircleCircle, IntersectLineCone, IntersectLineCylinder, IntersectLineSphere, IntersectPlaneCone,
    IntersectPlaneCylinder, IntersectPlaneSphere, IntersectSphereSphere, PARAM_BRANCH,
};
use fid_fit::contract::FittedFeature;

const LINE_SPHERE_TSV: &str = include_str!("../../../fixtures/cons/intquad-v1/line_sphere.tsv");
const LINE_CYLINDER_TSV: &str = include_str!("../../../fixtures/cons/intquad-v1/line_cylinder.tsv");
const LINE_CONE_TSV: &str = include_str!("../../../fixtures/cons/intquad-v1/line_cone.tsv");
const PLANE_SPHERE_TSV: &str = include_str!("../../../fixtures/cons/intquad-v1/plane_sphere.tsv");
const PLANE_CYLINDER_TSV: &str = include_str!("../../../fixtures/cons/intquad-v1/plane_cylinder.tsv");
const PLANE_CONE_TSV: &str = include_str!("../../../fixtures/cons/intquad-v1/plane_cone.tsv");
const CIRCLE_CIRCLE_TSV: &str = include_str!("../../../fixtures/cons/intquad-v1/circle_circle.tsv");
const SPHERE_SPHERE_TSV: &str = include_str!("../../../fixtures/cons/intquad-v1/sphere_sphere.tsv");

fn id(n: u8) -> ElementId {
    ElementId::parse(&format!("00000000-0000-7000-8000-0000000000{n:02}")).unwrap()
}

fn parse_rows(tsv: &str) -> Vec<Vec<f64>> {
    tsv.lines()
        .filter(|l| !l.trim().is_empty() && !l.starts_with('#'))
        .map(|l| l.split('\t').map(|t| t.parse::<f64>().unwrap()).collect())
        .collect()
}

fn v3(r: &[f64], i: usize) -> [f64; 3] {
    [r[i], r[i + 1], r[i + 2]]
}

fn dist3(a: [f64; 3], b: [f64; 3]) -> f64 {
    let dx = a[0] - b[0];
    let dy = a[1] - b[1];
    let dz = a[2] - b[2];
    (dx * dx + dy * dy + dz * dz).sqrt()
}

fn scale3(a: [f64; 3]) -> f64 {
    (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt().max(1.0)
}

fn params_br(b: Branch) -> ConstructParams {
    ConstructParams::new(1e-8, 1e-8).unwrap().with(PARAM_BRANCH, QuantityKind::Count, b.as_param()).unwrap()
}

#[test]
fn ac1_all_eight_variants_match_500_seeded_cases_within_1e_12() {
    let p_none = ConstructParams::new(1e-8, 1e-8).unwrap();
    let p1 = params_br(Branch::First);
    let p2 = params_br(Branch::Second);

    // 1. line_sphere
    let ls = IntersectLineSphere::new(id(1), id(2)).unwrap();
    let rows = parse_rows(LINE_SPHERE_TSV);
    assert_eq!(rows.len(), 500);
    for (i, r) in rows.iter().enumerate() {
        let g = [
            ElementGeom::Feature(FittedFeature::Line3 { p: v3(r, 0), d: v3(r, 3) }),
            ElementGeom::Feature(FittedFeature::Sphere { c: v3(r, 6), r: r[9] }),
        ];
        let FittedFeature::Point { c: c1 } = *run(&ls, &g, &p1).unwrap().output().as_feature().unwrap() else {
            panic!()
        };
        let FittedFeature::Point { c: c2 } = *run(&ls, &g, &p2).unwrap().output().as_feature().unwrap() else {
            panic!()
        };
        assert!(dist3(c1, v3(r, 10)) <= 1e-12 * scale3(v3(r, 10)), "line_sphere b1 {i}");
        assert!(dist3(c2, v3(r, 13)) <= 1e-12 * scale3(v3(r, 13)), "line_sphere b2 {i}");
    }

    // 2. line_cylinder
    let lcyl = IntersectLineCylinder::new(id(1), id(2)).unwrap();
    let rows = parse_rows(LINE_CYLINDER_TSV);
    assert_eq!(rows.len(), 500);
    for (i, r) in rows.iter().enumerate() {
        let g = [
            ElementGeom::Feature(FittedFeature::Line3 { p: v3(r, 0), d: v3(r, 3) }),
            ElementGeom::Feature(FittedFeature::Cylinder { p: v3(r, 6), d: v3(r, 9), r: r[12] }),
        ];
        let FittedFeature::Point { c: c1 } = *run(&lcyl, &g, &p1).unwrap().output().as_feature().unwrap() else {
            panic!()
        };
        let FittedFeature::Point { c: c2 } = *run(&lcyl, &g, &p2).unwrap().output().as_feature().unwrap() else {
            panic!()
        };
        assert!(dist3(c1, v3(r, 13)) <= 1e-12 * scale3(v3(r, 13)), "line_cyl b1 {i}");
        assert!(dist3(c2, v3(r, 16)) <= 1e-12 * scale3(v3(r, 16)), "line_cyl b2 {i}");
    }

    // 3. line_cone
    let lcon = IntersectLineCone::new(id(1), id(2)).unwrap();
    let rows = parse_rows(LINE_CONE_TSV);
    assert_eq!(rows.len(), 500);
    for (i, r) in rows.iter().enumerate() {
        let g = [
            ElementGeom::Feature(FittedFeature::Line3 { p: v3(r, 0), d: v3(r, 3) }),
            ElementGeom::Feature(FittedFeature::Cone { apex: v3(r, 6), d: v3(r, 9), half_angle: r[12] }),
        ];
        let FittedFeature::Point { c: c1 } = *run(&lcon, &g, &p1).unwrap().output().as_feature().unwrap() else {
            panic!()
        };
        let FittedFeature::Point { c: c2 } = *run(&lcon, &g, &p2).unwrap().output().as_feature().unwrap() else {
            panic!()
        };
        assert!(dist3(c1, v3(r, 13)) <= 1e-11 * scale3(v3(r, 13)), "line_cone b1 {i}");
        assert!(dist3(c2, v3(r, 16)) <= 1e-11 * scale3(v3(r, 16)), "line_cone b2 {i}");
    }

    // 4. plane_sphere
    let ps = IntersectPlaneSphere::new(id(1), id(2)).unwrap();
    let rows = parse_rows(PLANE_SPHERE_TSV);
    assert_eq!(rows.len(), 500);
    for (i, r) in rows.iter().enumerate() {
        let g = [
            ElementGeom::Feature(FittedFeature::Plane { p: v3(r, 0), n: v3(r, 3) }),
            ElementGeom::Feature(FittedFeature::Sphere { c: v3(r, 6), r: r[9] }),
        ];
        let FittedFeature::Circle3 { c, n, r: cr } = *run(&ps, &g, &p_none).unwrap().output().as_feature().unwrap()
        else {
            panic!()
        };
        assert!(dist3(c, v3(r, 10)) <= 1e-12 * scale3(v3(r, 10)), "plane_sphere c {i}");
        assert!(dist3(n, v3(r, 13)) <= 1e-12, "plane_sphere n {i}");
        assert!((cr - r[16]).abs() <= 1e-12 * r[16].max(1.0), "plane_sphere r {i}");
    }

    // 5. plane_cylinder
    let pcyl = IntersectPlaneCylinder::new(id(1), id(2)).unwrap();
    let rows = parse_rows(PLANE_CYLINDER_TSV);
    assert_eq!(rows.len(), 500);
    for (i, r) in rows.iter().enumerate() {
        let g = [
            ElementGeom::Feature(FittedFeature::Plane { p: v3(r, 0), n: v3(r, 3) }),
            ElementGeom::Feature(FittedFeature::Cylinder { p: v3(r, 6), d: v3(r, 9), r: r[12] }),
        ];
        let FittedFeature::Ellipse { c, d, a, b, .. } =
            *run(&pcyl, &g, &p_none).unwrap().output().as_feature().unwrap()
        else {
            panic!()
        };
        assert!(dist3(c, v3(r, 13)) <= 1e-12 * scale3(v3(r, 13)), "plane_cyl c {i}");
        assert!(dist3(d, v3(r, 16)) <= 1e-12, "plane_cyl d {i}");
        assert!((a - r[19]).abs() <= 1e-12 * r[19].max(1.0), "plane_cyl a {i}");
        assert!((b - r[20]).abs() <= 1e-12 * r[20].max(1.0), "plane_cyl b {i}");
    }

    // 6. plane_cone
    let pcon = IntersectPlaneCone::new(id(1), id(2)).unwrap();
    let rows = parse_rows(PLANE_CONE_TSV);
    assert_eq!(rows.len(), 500);
    for (i, r) in rows.iter().enumerate() {
        let g = [
            ElementGeom::Feature(FittedFeature::Plane { p: v3(r, 0), n: v3(r, 3) }),
            ElementGeom::Feature(FittedFeature::Cone { apex: v3(r, 6), d: v3(r, 9), half_angle: r[12] }),
        ];
        let FittedFeature::Ellipse { c, d, a, b, .. } =
            *run(&pcon, &g, &p_none).unwrap().output().as_feature().unwrap()
        else {
            panic!()
        };
        assert!(dist3(c, v3(r, 13)) <= 1e-12 * scale3(v3(r, 13)), "plane_cone c {i}");
        assert!(dist3(d, v3(r, 16)) <= 1e-12, "plane_cone d {i}");
        assert!((a - r[19]).abs() <= 1e-12 * r[19].max(1.0), "plane_cone a {i}");
        assert!((b - r[20]).abs() <= 1e-12 * r[20].max(1.0), "plane_cone b {i}");
    }

    // 7. circle_circle
    let cc = IntersectCircleCircle::new(id(1), id(2)).unwrap();
    let rows = parse_rows(CIRCLE_CIRCLE_TSV);
    assert_eq!(rows.len(), 500);
    for (i, r) in rows.iter().enumerate() {
        let g = [
            ElementGeom::Feature(FittedFeature::Circle3 { c: v3(r, 0), n: v3(r, 3), r: r[6] }),
            ElementGeom::Feature(FittedFeature::Circle3 { c: v3(r, 7), n: v3(r, 3), r: r[10] }),
        ];
        let FittedFeature::Point { c: c1 } = *run(&cc, &g, &p1).unwrap().output().as_feature().unwrap() else {
            panic!()
        };
        let FittedFeature::Point { c: c2 } = *run(&cc, &g, &p2).unwrap().output().as_feature().unwrap() else {
            panic!()
        };
        assert!(dist3(c1, v3(r, 11)) <= 1e-12 * scale3(v3(r, 11)), "circle_circle b1 {i}");
        assert!(dist3(c2, v3(r, 14)) <= 1e-12 * scale3(v3(r, 14)), "circle_circle b2 {i}");
    }

    // 8. sphere_sphere
    let ss = IntersectSphereSphere::new(id(1), id(2)).unwrap();
    let rows = parse_rows(SPHERE_SPHERE_TSV);
    assert_eq!(rows.len(), 500);
    for (i, r) in rows.iter().enumerate() {
        let g = [
            ElementGeom::Feature(FittedFeature::Sphere { c: v3(r, 0), r: r[3] }),
            ElementGeom::Feature(FittedFeature::Sphere { c: v3(r, 4), r: r[7] }),
        ];
        let FittedFeature::Circle3 { c, n, r: cr } = *run(&ss, &g, &p_none).unwrap().output().as_feature().unwrap()
        else {
            panic!()
        };
        assert!(dist3(c, v3(r, 8)) <= 1e-12 * scale3(v3(r, 8)), "sphere_sphere c {i}");
        assert!(dist3(n, v3(r, 11)) <= 1e-12, "sphere_sphere n {i}");
        assert!((cr - r[14]).abs() <= 1e-12 * r[14].max(1.0), "sphere_sphere r {i}");
    }
}

#[test]
fn ac2_ac3_oblique_plane_cylinder_tangency_miss_and_conic_not_supported() {
    // AC-2: Plane at 30 deg to cylinder axis (normal at 60 deg to axis) -> ellipse a = r / cos(60 deg), b = r.
    let pcyl = IntersectPlaneCylinder::new(id(1), id(2)).unwrap();
    let phi = core::f64::consts::FRAC_PI_3;
    let r = 0.015_f64;
    let g_cyl = [
        ElementGeom::Feature(FittedFeature::Plane { p: [0.0, 0.0, 0.0], n: [0.0, phi.sin(), phi.cos()] }),
        ElementGeom::Feature(FittedFeature::Cylinder { p: [0.0, 0.0, 0.0], d: [0.0, 0.0, 1.0], r }),
    ];
    let p_none = ConstructParams::new(1e-8, 1e-8).unwrap();
    let FittedFeature::Ellipse { a, b, .. } = *run(&pcyl, &g_cyl, &p_none).unwrap().output().as_feature().unwrap()
    else {
        panic!()
    };
    assert!((a - r / phi.cos()).abs() <= 1e-12 * a);
    assert!((b - r).abs() <= 1e-12 * b);

    // AC-3: Line 1e-12 m from tangent to sphere with 1e-9 m threshold -> IllConditioned(Distance).
    let ls = IntersectLineSphere::new(id(1), id(2)).unwrap();
    let p_tang = ConstructParams::new(1e-8, 1e-9)
        .unwrap()
        .with(PARAM_BRANCH, QuantityKind::Count, Branch::First.as_param())
        .unwrap();
    let g_tang = [
        ElementGeom::Feature(FittedFeature::Line3 { p: [0.0, 1.0 - 1e-12, 0.0], d: [1.0, 0.0, 0.0] }),
        ElementGeom::Feature(FittedFeature::Sphere { c: [0.0, 0.0, 0.0], r: 1.0 }),
    ];
    let err_t = run(&ls, &g_tang, &p_tang).unwrap_err();
    assert!(matches!(err_t, ConstructError::IllConditioned(cond) if cond.measure() == Measure::Distance));

    // AC-3: Line 1 mm outside sphere -> Undefined{NoIntersection} with gap 0.001 m.
    let g_miss = [
        ElementGeom::Feature(FittedFeature::Line3 { p: [0.0, 1.001, 0.0], d: [1.0, 0.0, 0.0] }),
        ElementGeom::Feature(FittedFeature::Sphere { c: [0.0, 0.0, 0.0], r: 1.0 }),
    ];
    let decl = ls.declare(&p_tang);
    let inp = Inputs::bind(&decl, &g_miss).unwrap();
    let err_m = ls.evaluate_with_diagnostics(&inp, &p_tang).unwrap_err();
    assert_eq!(err_m.error, ConstructError::Undefined { reason: UndefinedReason::NoIntersection });
    assert!((err_m.gap.unwrap() - 0.001).abs() <= 1e-15);

    // REQ-004: Hyperbolic plane-cone section -> Undefined{NotSupported}.
    let pcon = IntersectPlaneCone::new(id(1), id(2)).unwrap();
    let g_hyp = [
        ElementGeom::Feature(FittedFeature::Plane { p: [0.5, 0.0, 1.0], n: [1.0, 0.0, 0.0] }),
        ElementGeom::Feature(FittedFeature::Cone { apex: [0.0, 0.0, 0.0], d: [0.0, 0.0, 1.0], half_angle: 0.4 }),
    ];
    let err_h = run(&pcon, &g_hyp, &p_none).unwrap_err();
    assert_eq!(err_h, ConstructError::Undefined { reason: UndefinedReason::NotSupported });
}

#[test]
fn ac4_recompute_equivalence_harness_passes_representative_quadrics() {
    let p1 = params_br(Branch::First);
    let p_none = ConstructParams::new(1e-8, 1e-8).unwrap();

    harness::check(
        || IntersectLineSphere::new(id(1), id(2)).unwrap(),
        &p1,
        &[
            ElementGeom::Feature(FittedFeature::Line3 { p: [0.0, 0.2, 0.0], d: [1.0, 0.0, 0.0] }),
            ElementGeom::Feature(FittedFeature::Sphere { c: [0.0, 0.0, 0.0], r: 1.0 }),
        ],
        &[
            ElementGeom::Feature(FittedFeature::Line3 { p: [0.0, 0.3, 0.1], d: [1.0, 0.1, 0.0] }),
            ElementGeom::Feature(FittedFeature::Sphere { c: [0.1, 0.0, 0.0], r: 1.2 }),
        ],
    )
    .unwrap()
    .unwrap();

    harness::check(
        || IntersectPlaneCylinder::new(id(1), id(2)).unwrap(),
        &p_none,
        &[
            ElementGeom::Feature(FittedFeature::Plane { p: [0.0, 0.0, 0.0], n: [0.0, 0.5, 0.8660254] }),
            ElementGeom::Feature(FittedFeature::Cylinder { p: [0.0, 0.0, 0.0], d: [0.0, 0.0, 1.0], r: 0.5 }),
        ],
        &[
            ElementGeom::Feature(FittedFeature::Plane { p: [0.0, 0.0, 1.0], n: [0.1, 0.4, 0.91] }),
            ElementGeom::Feature(FittedFeature::Cylinder { p: [0.2, 0.0, 0.0], d: [0.0, 0.0, 1.0], r: 0.8 }),
        ],
    )
    .unwrap()
    .unwrap();

    harness::check(
        || IntersectSphereSphere::new(id(1), id(2)).unwrap(),
        &p_none,
        &[
            ElementGeom::Feature(FittedFeature::Sphere { c: [0.0, 0.0, 0.0], r: 1.0 }),
            ElementGeom::Feature(FittedFeature::Sphere { c: [1.0, 0.0, 0.0], r: 1.0 }),
        ],
        &[
            ElementGeom::Feature(FittedFeature::Sphere { c: [0.0, 0.1, 0.0], r: 1.1 }),
            ElementGeom::Feature(FittedFeature::Sphere { c: [1.2, 0.0, 0.0], r: 0.9 }),
        ],
    )
    .unwrap()
    .unwrap();
}
