//! Acceptance tests AC-1..AC-4 for `KRN.CONS.OPS.PROJ` (`fid_construct::ops_proj`).
//! Uses `include_str!` so every test runs identically on x86-64, ARM64 and `wasm32-wasip1`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_construct::contract::{
    harness, run, ConstructError, ConstructParams, Construction, ElementGeom, ElementId, Inputs, Measure,
    UndefinedReason,
};
use fid_construct::ops_proj::{ProjectCirclePlane, ProjectLinePlane, ProjectPointLine, ProjectPointPlane};
use fid_fit::contract::FittedFeature;

const POINT_PLANE_TSV: &str = include_str!("../../../fixtures/cons/proj-v1/point_plane.tsv");
const POINT_LINE_TSV: &str = include_str!("../../../fixtures/cons/proj-v1/point_line.tsv");
const LINE_PLANE_TSV: &str = include_str!("../../../fixtures/cons/proj-v1/line_plane.tsv");
const CIRCLE_PLANE_TSV: &str = include_str!("../../../fixtures/cons/proj-v1/circle_plane.tsv");

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

#[test]
fn ac1_all_four_variants_match_1000_seeded_closed_form_cases() {
    let params = ConstructParams::new(1e-8, 1e-8).unwrap();

    // 1. ProjectPointPlane
    let pp = ProjectPointPlane::new(id(1), id(2)).unwrap();
    let rows = parse_rows(POINT_PLANE_TSV);
    assert_eq!(rows.len(), 1000);
    for (i, r) in rows.iter().enumerate() {
        let g = [
            ElementGeom::Feature(FittedFeature::Point { c: v3(r, 0) }),
            ElementGeom::Feature(FittedFeature::Plane { p: v3(r, 3), n: v3(r, 6) }),
        ];
        let decl = pp.declare(&params);
        let inp = Inputs::bind(&decl, &g).unwrap();
        let (el, diag) = pp.evaluate_with_diagnostics(&inp, &params).unwrap();
        let FittedFeature::Point { c } = *el.as_feature().unwrap() else { panic!() };
        let exp = v3(r, 9);
        assert!(dist3(c, exp) <= 1e-12 * scale3(exp), "point_plane {i}");
        assert!((diag.signed_distance - r[12]).abs() <= 1e-12 * scale3(exp), "point_plane dist {i}");
    }

    // 2. ProjectPointLine
    let pl = ProjectPointLine::new(id(1), id(2)).unwrap();
    let rows = parse_rows(POINT_LINE_TSV);
    assert_eq!(rows.len(), 1000);
    for (i, r) in rows.iter().enumerate() {
        let g = [
            ElementGeom::Feature(FittedFeature::Point { c: v3(r, 0) }),
            ElementGeom::Feature(FittedFeature::Line3 { p: v3(r, 3), d: v3(r, 6) }),
        ];
        let decl = pl.declare(&params);
        let inp = Inputs::bind(&decl, &g).unwrap();
        let (el, diag) = pl.evaluate_with_diagnostics(&inp, &params).unwrap();
        let FittedFeature::Point { c } = *el.as_feature().unwrap() else { panic!() };
        let exp = v3(r, 9);
        assert!(dist3(c, exp) <= 1e-12 * scale3(exp), "point_line {i}");
        assert!((diag.signed_distance - r[12]).abs() <= 1e-12 * scale3(exp), "point_line dist {i}");
    }

    // 3. ProjectLinePlane
    let lp = ProjectLinePlane::new(id(1), id(2)).unwrap();
    let rows = parse_rows(LINE_PLANE_TSV);
    assert_eq!(rows.len(), 1000);
    for (i, r) in rows.iter().enumerate() {
        let g = [
            ElementGeom::Feature(FittedFeature::Line3 { p: v3(r, 0), d: v3(r, 3) }),
            ElementGeom::Feature(FittedFeature::Plane { p: v3(r, 6), n: v3(r, 9) }),
        ];
        let decl = lp.declare(&params);
        let inp = Inputs::bind(&decl, &g).unwrap();
        let (el, diag) = lp.evaluate_with_diagnostics(&inp, &params).unwrap();
        let FittedFeature::Line3 { p, d } = *el.as_feature().unwrap() else { panic!() };
        let exp_p = v3(r, 12);
        let exp_d = v3(r, 15);
        assert!(dist3(p, exp_p) <= 1e-12 * scale3(exp_p), "line_plane p {i}");
        assert!(dist3(d, exp_d) <= 1e-12, "line_plane d {i}");
        assert!((diag.signed_distance - r[18]).abs() <= 1e-12 * scale3(exp_p), "line_plane dist {i}");
    }

    // 4. ProjectCirclePlane
    let cp = ProjectCirclePlane::new(id(1), id(2)).unwrap();
    let rows = parse_rows(CIRCLE_PLANE_TSV);
    assert_eq!(rows.len(), 1000);
    for (i, r) in rows.iter().enumerate() {
        let g = [
            ElementGeom::Feature(FittedFeature::Circle3 { c: v3(r, 0), n: v3(r, 3), r: r[6] }),
            ElementGeom::Feature(FittedFeature::Plane { p: v3(r, 7), n: v3(r, 10) }),
        ];
        let decl = cp.declare(&params);
        let inp = Inputs::bind(&decl, &g).unwrap();
        let (el, diag) = cp.evaluate_with_diagnostics(&inp, &params).unwrap();
        let FittedFeature::Ellipse { c, d, a, b, .. } = *el.as_feature().unwrap() else { panic!() };
        let exp_c = v3(r, 13);
        let exp_d = v3(r, 16);
        assert!(dist3(c, exp_c) <= 1e-12 * scale3(exp_c), "circle_plane c {i}");
        assert!(dist3(d, exp_d) <= 1e-12, "circle_plane d {i}");
        assert!((a - r[19]).abs() <= 1e-12 * r[19].max(1.0), "circle_plane a {i}");
        assert!((b - r[20]).abs() <= 1e-12 * r[20].max(1.0), "circle_plane b {i}");
        assert!((diag.signed_distance - r[21]).abs() <= 1e-12 * scale3(exp_c), "circle_plane dist {i}");
    }
}

#[test]
fn ac2_tilted_60deg_circle_and_parallel_and_edge_on_circle() {
    let cp = ProjectCirclePlane::new(id(1), id(2)).unwrap();
    let params = ConstructParams::new(1e-8, 1e-8).unwrap();

    // AC-2: 10 mm circle tilted 60 degrees -> ellipse with semi-axes 10 mm and 5 mm.
    let theta = core::f64::consts::FRAC_PI_3;
    let g_tilt = [
        ElementGeom::Feature(FittedFeature::Circle3 {
            c: [0.0, 0.0, 0.02],
            n: [0.0, theta.sin(), theta.cos()],
            r: 0.010,
        }),
        ElementGeom::Feature(FittedFeature::Plane { p: [0.0, 0.0, 0.0], n: [0.0, 0.0, 1.0] }),
    ];
    let ev = run(&cp, &g_tilt, &params).unwrap();
    let FittedFeature::Ellipse { c, a, b, .. } = *ev.output().as_feature().unwrap() else { panic!() };
    assert!(dist3(c, [0.0, 0.0, 0.0]) <= 1e-15);
    assert!((a - 0.010).abs() <= 1e-15);
    assert!((b - 0.005).abs() <= 1e-15);

    // Parallel circle within threshold -> Circle3.
    let g_par = [
        ElementGeom::Feature(FittedFeature::Circle3 { c: [1.0, 2.0, 3.0], n: [0.0, 0.0, 1.0], r: 0.010 }),
        ElementGeom::Feature(FittedFeature::Plane { p: [0.0, 0.0, 0.0], n: [0.0, 0.0, 1.0] }),
    ];
    let ev_par = run(&cp, &g_par, &params).unwrap();
    let FittedFeature::Circle3 { c, r, .. } = *ev_par.output().as_feature().unwrap() else { panic!() };
    assert!(dist3(c, [1.0, 2.0, 0.0]) <= 1e-15);
    assert!((r - 0.010).abs() <= 1e-15);

    // Edge-on circle -> IllConditioned(Angle).
    let g_edge = [
        ElementGeom::Feature(FittedFeature::Circle3 { c: [0.0, 0.0, 1.0], n: [1.0, 0.0, 0.0], r: 0.010 }),
        ElementGeom::Feature(FittedFeature::Plane { p: [0.0, 0.0, 0.0], n: [0.0, 0.0, 1.0] }),
    ];
    let err = run(&cp, &g_edge, &params).unwrap_err();
    assert!(matches!(err, ConstructError::IllConditioned(cond) if cond.measure() == Measure::Angle));
}

#[test]
fn ac3_perpendicular_line_projects_to_point_undefined() {
    let lp = ProjectLinePlane::new(id(1), id(2)).unwrap();
    let eps = 1e-10_f64;
    let g = [
        ElementGeom::Feature(FittedFeature::Line3 { p: [1.0, 2.0, 3.0], d: [eps.sin(), 0.0, eps.cos()] }),
        ElementGeom::Feature(FittedFeature::Plane { p: [0.0, 0.0, 0.0], n: [0.0, 0.0, 1.0] }),
    ];
    let params = ConstructParams::new(1e-8, 1e-8).unwrap();
    let err = run(&lp, &g, &params).unwrap_err();
    assert_eq!(err, ConstructError::Undefined { reason: UndefinedReason::NotSupported });
}

#[test]
fn ac4_recompute_equivalence_harness_passes_all_four_variants() {
    let params = ConstructParams::new(1e-8, 1e-8).unwrap();

    harness::check(
        || ProjectPointPlane::new(id(1), id(2)).unwrap(),
        &params,
        &[
            ElementGeom::Feature(FittedFeature::Point { c: [1.0, 2.0, 3.0] }),
            ElementGeom::Feature(FittedFeature::Plane { p: [0.0, 0.0, 1.0], n: [0.0, 0.0, 1.0] }),
        ],
        &[
            ElementGeom::Feature(FittedFeature::Point { c: [4.0, 5.0, 6.0] }),
            ElementGeom::Feature(FittedFeature::Plane { p: [0.0, 0.0, 2.0], n: [0.1, 0.0, 1.0] }),
        ],
    )
    .unwrap()
    .unwrap();

    harness::check(
        || ProjectPointLine::new(id(1), id(2)).unwrap(),
        &params,
        &[
            ElementGeom::Feature(FittedFeature::Point { c: [1.0, 2.0, 3.0] }),
            ElementGeom::Feature(FittedFeature::Line3 { p: [0.0, 0.0, 0.0], d: [1.0, 0.0, 0.0] }),
        ],
        &[
            ElementGeom::Feature(FittedFeature::Point { c: [4.0, 5.0, 6.0] }),
            ElementGeom::Feature(FittedFeature::Line3 { p: [0.0, 1.0, 0.0], d: [1.0, 0.2, 0.0] }),
        ],
    )
    .unwrap()
    .unwrap();

    harness::check(
        || ProjectLinePlane::new(id(1), id(2)).unwrap(),
        &params,
        &[
            ElementGeom::Feature(FittedFeature::Line3 { p: [1.0, 2.0, 3.0], d: [1.0, 0.0, 0.3] }),
            ElementGeom::Feature(FittedFeature::Plane { p: [0.0, 0.0, 0.0], n: [0.0, 0.0, 1.0] }),
        ],
        &[
            ElementGeom::Feature(FittedFeature::Line3 { p: [2.0, 1.0, 4.0], d: [0.5, 1.0, 0.2] }),
            ElementGeom::Feature(FittedFeature::Plane { p: [0.0, 0.0, 1.0], n: [0.0, 0.1, 1.0] }),
        ],
    )
    .unwrap()
    .unwrap();

    harness::check(
        || ProjectCirclePlane::new(id(1), id(2)).unwrap(),
        &params,
        &[
            ElementGeom::Feature(FittedFeature::Circle3 { c: [1.0, 2.0, 3.0], n: [0.0, 0.5, 0.8660254], r: 0.02 }),
            ElementGeom::Feature(FittedFeature::Plane { p: [0.0, 0.0, 0.0], n: [0.0, 0.0, 1.0] }),
        ],
        &[
            ElementGeom::Feature(FittedFeature::Circle3 { c: [2.0, 3.0, 4.0], n: [0.3, 0.4, 0.86], r: 0.03 }),
            ElementGeom::Feature(FittedFeature::Plane { p: [0.0, 0.0, 1.0], n: [0.0, 0.05, 1.0] }),
        ],
    )
    .unwrap()
    .unwrap();
}
