//! Acceptance tests AC-1..AC-4 for `KRN.CONS.OPS.OFFS` (`fid_construct::ops_offs`).
//! Uses `include_str!` so every test runs identically on x86-64, ARM64 and `wasm32-wasip1`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_construct::contract::{
    harness, run, ConstructError, ConstructParams, Construction, ElementGeom, ElementId, Measure, QuantityKind,
    UndefinedReason,
};
use fid_construct::ops_offs::{
    unit_direction, OffsetCircleAxial, OffsetCircleRadial, OffsetLineDir, OffsetPlaneNormal, OffsetPointDir,
    PARAM_DISTANCE,
};
use fid_fit::contract::FittedFeature;

const PLANE_NORMAL_TSV: &str = include_str!("../../../fixtures/cons/offs-v1/plane_normal.tsv");
const POINT_DIR_TSV: &str = include_str!("../../../fixtures/cons/offs-v1/point_dir.tsv");
const LINE_DIR_TSV: &str = include_str!("../../../fixtures/cons/offs-v1/line_dir.tsv");
const CIRCLE_AXIAL_TSV: &str = include_str!("../../../fixtures/cons/offs-v1/circle_axial.tsv");
const CIRCLE_RADIAL_TSV: &str = include_str!("../../../fixtures/cons/offs-v1/circle_radial.tsv");

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

fn dot3(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn scale3(a: [f64; 3]) -> f64 {
    (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt().max(1.0)
}

fn params_dist(d: f64) -> ConstructParams {
    ConstructParams::new(1e-8, 1e-8).unwrap().with(PARAM_DISTANCE, QuantityKind::Length, d).unwrap()
}

#[test]
fn ac1_all_five_variants_match_1000_seeded_closed_form_cases() {
    // 1. OffsetPlaneNormal
    let pn = OffsetPlaneNormal::new(id(1)).unwrap();
    let rows = parse_rows(PLANE_NORMAL_TSV);
    assert_eq!(rows.len(), 1000);
    for (i, r) in rows.iter().enumerate() {
        let g = [ElementGeom::Feature(FittedFeature::Plane { p: v3(r, 0), n: v3(r, 3) })];
        let ev = run(&pn, &g, &params_dist(r[6])).unwrap();
        let FittedFeature::Plane { p, n } = *ev.output().as_feature().unwrap() else { panic!() };
        let exp_p = v3(r, 7);
        let exp_n = v3(r, 10);
        // Check point lies on expected plane and normal matches.
        assert!(dist3(n, exp_n) <= 1e-12, "plane_normal n {i}");
        let plane_err = (dot3(p, exp_n) - dot3(exp_p, exp_n)).abs();
        assert!(plane_err <= 1e-12 * scale3(exp_p), "plane_normal offset {i}");
    }

    // 2. OffsetPointDir
    let pd = OffsetPointDir::new(id(1), id(2)).unwrap();
    let rows = parse_rows(POINT_DIR_TSV);
    assert_eq!(rows.len(), 1000);
    for (i, r) in rows.iter().enumerate() {
        let g = [
            ElementGeom::Feature(FittedFeature::Point { c: v3(r, 0) }),
            ElementGeom::Feature(FittedFeature::Line3 { p: [0.0, 0.0, 0.0], d: v3(r, 3) }),
        ];
        let ev = run(&pd, &g, &params_dist(r[6])).unwrap();
        let FittedFeature::Point { c } = *ev.output().as_feature().unwrap() else { panic!() };
        let exp_c = v3(r, 7);
        assert!(dist3(c, exp_c) <= 1e-12 * scale3(exp_c), "point_dir {i}");
    }

    // 3. OffsetLineDir
    let ld = OffsetLineDir::new(id(1), id(2)).unwrap();
    let rows = parse_rows(LINE_DIR_TSV);
    assert_eq!(rows.len(), 1000);
    for (i, r) in rows.iter().enumerate() {
        let g = [
            ElementGeom::Feature(FittedFeature::Line3 { p: v3(r, 0), d: v3(r, 3) }),
            ElementGeom::Feature(FittedFeature::Plane { p: [0.0, 0.0, 0.0], n: v3(r, 6) }),
        ];
        let ev = run(&ld, &g, &params_dist(r[9])).unwrap();
        let FittedFeature::Line3 { p, d } = *ev.output().as_feature().unwrap() else { panic!() };
        let exp_p = v3(r, 10);
        let exp_d = v3(r, 13);
        assert!(dist3(d, exp_d) <= 1e-12, "line_dir d {i}");
        // Distance from p to expected line must be <= 1e-12 * scale.
        let diff = [p[0] - exp_p[0], p[1] - exp_p[1], p[2] - exp_p[2]];
        let along = dot3(diff, exp_d);
        let perp = [diff[0] - along * exp_d[0], diff[1] - along * exp_d[1], diff[2] - along * exp_d[2]];
        let err = (perp[0] * perp[0] + perp[1] * perp[1] + perp[2] * perp[2]).sqrt();
        assert!(err <= 1e-12 * scale3(exp_p), "line_dir p {i}");
    }

    // 4. OffsetCircleAxial
    let ca = OffsetCircleAxial::new(id(1)).unwrap();
    let rows = parse_rows(CIRCLE_AXIAL_TSV);
    assert_eq!(rows.len(), 1000);
    for (i, r) in rows.iter().enumerate() {
        let g = [ElementGeom::Feature(FittedFeature::Circle3 { c: v3(r, 0), n: v3(r, 3), r: r[6] })];
        let ev = run(&ca, &g, &params_dist(r[7])).unwrap();
        let FittedFeature::Circle3 { c, n, r: out_r } = *ev.output().as_feature().unwrap() else { panic!() };
        let exp_c = v3(r, 8);
        let exp_n = v3(r, 11);
        assert!(dist3(c, exp_c) <= 1e-12 * scale3(exp_c), "circle_axial c {i}");
        assert!(dist3(n, exp_n) <= 1e-12, "circle_axial n {i}");
        assert!((out_r - r[14]).abs() <= 1e-15, "circle_axial r {i}");
    }

    // 5. OffsetCircleRadial
    let cr = OffsetCircleRadial::new(id(1)).unwrap();
    let rows = parse_rows(CIRCLE_RADIAL_TSV);
    assert_eq!(rows.len(), 1000);
    for (i, r) in rows.iter().enumerate() {
        let g = [ElementGeom::Feature(FittedFeature::Circle3 { c: v3(r, 0), n: v3(r, 3), r: r[6] })];
        let ev = run(&cr, &g, &params_dist(r[7])).unwrap();
        let FittedFeature::Circle3 { r: out_r, .. } = *ev.output().as_feature().unwrap() else { panic!() };
        assert!((out_r - r[8]).abs() <= 1e-12 * r[8].max(1.0), "circle_radial r {i}");
    }
}

#[test]
fn ac2_ac3_negative_radius_zero_dir_and_digest_change_with_distance() {
    // AC-2: 5 mm circle with -6 mm radial offset returns Undefined (non_positive_radius / ZeroLength).
    let cr = OffsetCircleRadial::new(id(1)).unwrap();
    let g = [ElementGeom::Feature(FittedFeature::Circle3 { c: [0.0, 0.0, 0.0], n: [0.0, 0.0, 1.0], r: 0.005 })];
    let err = run(&cr, &g, &params_dist(-0.006)).unwrap_err();
    assert_eq!(err, ConstructError::Undefined { reason: UndefinedReason::ZeroLength });

    // REQ-003: zero-norm direction returns IllConditioned.
    let zerr = unit_direction([0.0, 0.0, 0.0], 1e-8).unwrap_err();
    assert!(matches!(zerr, ConstructError::IllConditioned(c) if c.measure() == Measure::Ratio));

    // AC-3: distances 1 mm and 2 mm produce different parameter digests.
    let pn = OffsetPlaneNormal::new(id(1)).unwrap();
    let d1 = pn.declare(&params_dist(0.001));
    let d2 = pn.declare(&params_dist(0.002));
    assert_ne!(d1.params_digest(), d2.params_digest());
    assert_ne!(d1.canonical_bytes(), d2.canonical_bytes());
}

#[test]
fn ac4_recompute_equivalence_harness_passes_all_five_variants() {
    let params = params_dist(0.005);

    harness::check(
        || OffsetPlaneNormal::new(id(1)).unwrap(),
        &params,
        &[ElementGeom::Feature(FittedFeature::Plane { p: [0.0, 0.0, 0.0], n: [0.0, 0.0, 1.0] })],
        &[ElementGeom::Feature(FittedFeature::Plane { p: [0.0, 0.0, 1.0], n: [0.0, 1.0, 0.0] })],
    )
    .unwrap()
    .unwrap();

    harness::check(
        || OffsetPointDir::new(id(1), id(2)).unwrap(),
        &params,
        &[
            ElementGeom::Feature(FittedFeature::Point { c: [1.0, 2.0, 3.0] }),
            ElementGeom::Feature(FittedFeature::Line3 { p: [0.0, 0.0, 0.0], d: [1.0, 0.0, 0.0] }),
        ],
        &[
            ElementGeom::Feature(FittedFeature::Point { c: [4.0, 5.0, 6.0] }),
            ElementGeom::Feature(FittedFeature::Plane { p: [0.0, 0.0, 0.0], n: [0.0, 1.0, 0.0] }),
        ],
    )
    .unwrap()
    .unwrap();

    harness::check(
        || OffsetLineDir::new(id(1), id(2)).unwrap(),
        &params,
        &[
            ElementGeom::Feature(FittedFeature::Line3 { p: [0.0, 0.0, 0.0], d: [1.0, 0.0, 0.0] }),
            ElementGeom::Feature(FittedFeature::Plane { p: [0.0, 0.0, 0.0], n: [0.0, 0.0, 1.0] }),
        ],
        &[
            ElementGeom::Feature(FittedFeature::Line3 { p: [0.0, 1.0, 0.0], d: [0.0, 1.0, 0.0] }),
            ElementGeom::Feature(FittedFeature::Line3 { p: [0.0, 0.0, 0.0], d: [1.0, 0.0, 0.0] }),
        ],
    )
    .unwrap()
    .unwrap();

    harness::check(
        || OffsetCircleAxial::new(id(1)).unwrap(),
        &params,
        &[ElementGeom::Feature(FittedFeature::Circle3 { c: [0.0, 0.0, 0.0], n: [0.0, 0.0, 1.0], r: 0.01 })],
        &[ElementGeom::Feature(FittedFeature::Circle3 { c: [1.0, 2.0, 3.0], n: [1.0, 0.0, 0.0], r: 0.02 })],
    )
    .unwrap()
    .unwrap();

    harness::check(
        || OffsetCircleRadial::new(id(1)).unwrap(),
        &params,
        &[ElementGeom::Feature(FittedFeature::Circle3 { c: [0.0, 0.0, 0.0], n: [0.0, 0.0, 1.0], r: 0.01 })],
        &[ElementGeom::Feature(FittedFeature::Circle3 { c: [1.0, 2.0, 3.0], n: [0.0, 1.0, 0.0], r: 0.02 })],
    )
    .unwrap()
    .unwrap();
}
