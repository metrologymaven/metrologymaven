//! Acceptance tests AC-1..AC-4 for `KRN.CONS.OPS.MID` (`fid_construct::ops_mid`).
//! Uses `include_str!` so every test runs identically on x86-64, ARM64 and `wasm32-wasip1`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_construct::contract::{
    harness, run, ConstructError, ConstructParams, ElementGeom, ElementId, Measure, QuantityKind, UndefinedReason,
};
use fid_construct::ops_mid::{
    BisectBranch, BisectLines, BisectPlanes, MidpointPoints, SymmetryCircles, SymmetryLines, SymmetryPlanes,
    PARAM_ALLOW_INTERSECTING, PARAM_BRANCH,
};
use fid_fit::contract::FittedFeature;

const MIDPOINT_TSV: &str = include_str!("../../../fixtures/cons/mid-v1/midpoint_points.tsv");
const SYM_PLANES_TSV: &str = include_str!("../../../fixtures/cons/mid-v1/symmetry_planes.tsv");
const SYM_LINES_TSV: &str = include_str!("../../../fixtures/cons/mid-v1/symmetry_lines.tsv");
const SYM_CIRCLES_TSV: &str = include_str!("../../../fixtures/cons/mid-v1/symmetry_circles.tsv");
const BISECT_LINES_TSV: &str = include_str!("../../../fixtures/cons/mid-v1/bisect_lines.tsv");
const BISECT_PLANES_TSV: &str = include_str!("../../../fixtures/cons/mid-v1/bisect_planes.tsv");

fn id1() -> ElementId {
    ElementId::parse("00000000-0000-7000-8000-000000000001").unwrap()
}

fn id2() -> ElementId {
    ElementId::parse("00000000-0000-7000-8000-000000000002").unwrap()
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

fn dot3(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn sub3(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

#[test]
fn ac1_all_six_variants_match_1000_seeded_closed_form_cases_within_1e_12() {
    let params = ConstructParams::new(1e-6, 1e-6).unwrap();

    // 1. MidpointPoints (1,000 cases)
    let mp = MidpointPoints::new(id1(), id2()).unwrap();
    let rows = parse_rows(MIDPOINT_TSV);
    assert_eq!(rows.len(), 1000);
    for (i, r) in rows.iter().enumerate() {
        let g = [
            ElementGeom::Feature(FittedFeature::Point { c: v3(r, 0) }),
            ElementGeom::Feature(FittedFeature::Point { c: v3(r, 3) }),
        ];
        let ev = run(&mp, &g, &params).unwrap();
        let FittedFeature::Point { c } = *ev.output().as_feature().unwrap() else { panic!("expected Point") };
        let exp = v3(r, 6);
        assert!(dist3(c, exp) <= 1e-12 * scale3(exp), "midpoint case {i}");
    }

    // 2. SymmetryPlanes (1,000 cases)
    let sp = SymmetryPlanes::new(id1(), id2()).unwrap();
    let rows = parse_rows(SYM_PLANES_TSV);
    assert_eq!(rows.len(), 1000);
    for (i, r) in rows.iter().enumerate() {
        let g = [
            ElementGeom::Feature(FittedFeature::Plane { p: v3(r, 0), n: v3(r, 3) }),
            ElementGeom::Feature(FittedFeature::Plane { p: v3(r, 6), n: v3(r, 9) }),
        ];
        let ev = run(&sp, &g, &params).unwrap();
        let FittedFeature::Plane { p, n } = *ev.output().as_feature().unwrap() else { panic!("expected Plane") };
        let exp_p = v3(r, 12);
        let exp_n = v3(r, 15);
        assert!(dist3(n, exp_n) <= 1e-12, "sym_planes normal case {i}");
        // Plane point agreement: perpendicular distance from exp_p to output plane <= 1e-12 * scale.
        let perp = dot3(sub3(p, exp_p), exp_n).abs();
        assert!(perp <= 1e-12 * scale3(exp_p), "sym_planes plane offset case {i}: {perp}");
    }

    // 3. SymmetryLines (1,000 cases)
    let sl = SymmetryLines::new(id1(), id2()).unwrap();
    let rows = parse_rows(SYM_LINES_TSV);
    assert_eq!(rows.len(), 1000);
    for (i, r) in rows.iter().enumerate() {
        let g = [
            ElementGeom::Feature(FittedFeature::Line3 { p: v3(r, 0), d: v3(r, 3) }),
            ElementGeom::Feature(FittedFeature::Line3 { p: v3(r, 6), d: v3(r, 9) }),
        ];
        let ev = run(&sl, &g, &params).unwrap();
        let FittedFeature::Line3 { p, d } = *ev.output().as_feature().unwrap() else { panic!("expected Line3") };
        let exp_p = v3(r, 12);
        let exp_d = v3(r, 15);
        assert!(dist3(d, exp_d) <= 1e-12, "sym_lines dir case {i}");
        assert!(dist3(p, exp_p) <= 1e-12 * scale3(exp_p), "sym_lines point case {i}");
    }

    // 4. SymmetryCircles (1,000 cases)
    let sc = SymmetryCircles::new(id1(), id2()).unwrap();
    let rows = parse_rows(SYM_CIRCLES_TSV);
    assert_eq!(rows.len(), 1000);
    for (i, r) in rows.iter().enumerate() {
        let g = [
            ElementGeom::Feature(FittedFeature::Circle3 { c: v3(r, 0), n: v3(r, 3), r: r[6] }),
            ElementGeom::Feature(FittedFeature::Circle3 { c: v3(r, 7), n: v3(r, 10), r: r[13] }),
        ];
        let ev = run(&sc, &g, &params).unwrap();
        let FittedFeature::Circle3 { c, n, r: rm } = *ev.output().as_feature().unwrap() else {
            panic!("expected Circle3")
        };
        let exp_c = v3(r, 14);
        let exp_n = v3(r, 17);
        let exp_r = r[20];
        assert!(dist3(c, exp_c) <= 1e-12 * scale3(exp_c), "sym_circles c case {i}");
        assert!(dist3(n, exp_n) <= 1e-12, "sym_circles n case {i}");
        assert!((rm - exp_r).abs() <= 1e-12 * exp_r.max(1.0), "sym_circles r case {i}");
    }

    // 5. BisectLines (1,000 cases)
    let bl = BisectLines::new(id1(), id2()).unwrap();
    let rows = parse_rows(BISECT_LINES_TSV);
    assert_eq!(rows.len(), 1000);
    for (i, r) in rows.iter().enumerate() {
        let p_branch = ConstructParams::new(1e-6, 1e-6).unwrap().with(PARAM_BRANCH, QuantityKind::Count, r[0]).unwrap();
        let g = [
            ElementGeom::Feature(FittedFeature::Line3 { p: v3(r, 1), d: v3(r, 4) }),
            ElementGeom::Feature(FittedFeature::Line3 { p: v3(r, 7), d: v3(r, 10) }),
        ];
        let ev = run(&bl, &g, &p_branch).unwrap();
        let FittedFeature::Line3 { p, d } = *ev.output().as_feature().unwrap() else { panic!("expected Line3") };
        let exp_p = v3(r, 13);
        let exp_d = v3(r, 16);
        assert!(dist3(d, exp_d) <= 1e-12, "bisect_lines dir case {i}");
        assert!(dist3(p, exp_p) <= 1e-12 * scale3(exp_p), "bisect_lines pt case {i}");
    }

    // 6. BisectPlanes (1,000 cases)
    let bp = BisectPlanes::new(id1(), id2()).unwrap();
    let rows = parse_rows(BISECT_PLANES_TSV);
    assert_eq!(rows.len(), 1000);
    for (i, r) in rows.iter().enumerate() {
        let p_branch = ConstructParams::new(1e-6, 1e-6).unwrap().with(PARAM_BRANCH, QuantityKind::Count, r[0]).unwrap();
        let g = [
            ElementGeom::Feature(FittedFeature::Plane { p: v3(r, 1), n: v3(r, 4) }),
            ElementGeom::Feature(FittedFeature::Plane { p: v3(r, 7), n: v3(r, 10) }),
        ];
        let ev = run(&bp, &g, &p_branch).unwrap();
        let FittedFeature::Plane { p, n } = *ev.output().as_feature().unwrap() else { panic!("expected Plane") };
        let exp_p = v3(r, 13);
        let exp_n = v3(r, 16);
        assert!(dist3(n, exp_n) <= 1e-12, "bisect_planes normal case {i}");
        // Plane passes through the intersection line point exp_p: |(p - exp_p) . exp_n| <= 1e-12 * scale.
        let perp = dot3(sub3(p, exp_p), exp_n).abs();
        assert!(perp <= 1e-12 * scale3(exp_p), "bisect_planes plane offset case {i}: {perp}");
    }
}

#[test]
fn ac2_opposite_normal_parallel_planes_10mm_apart_are_bit_identical_under_sign_swap() {
    let sp = SymmetryPlanes::new(id1(), id2()).unwrap();
    let params = ConstructParams::new(1e-6, 1e-6).unwrap();

    let g_opp = [
        ElementGeom::Feature(FittedFeature::Plane { p: [0.0, 0.0, 0.0], n: [0.0, 0.0, 1.0] }),
        ElementGeom::Feature(FittedFeature::Plane { p: [0.0, 0.0, 0.010], n: [0.0, 0.0, -1.0] }),
    ];
    let g_same = [
        ElementGeom::Feature(FittedFeature::Plane { p: [0.0, 0.0, 0.0], n: [0.0, 0.0, 1.0] }),
        ElementGeom::Feature(FittedFeature::Plane { p: [0.0, 0.0, 0.010], n: [0.0, 0.0, 1.0] }),
    ];
    let g_both_neg = [
        ElementGeom::Feature(FittedFeature::Plane { p: [0.0, 0.0, 0.0], n: [0.0, 0.0, -1.0] }),
        ElementGeom::Feature(FittedFeature::Plane { p: [0.0, 0.0, 0.010], n: [0.0, 0.0, -1.0] }),
    ];

    let ev_opp = run(&sp, &g_opp, &params).unwrap();
    let ev_same = run(&sp, &g_same, &params).unwrap();
    let ev_neg = run(&sp, &g_both_neg, &params).unwrap();

    // Lies 5 mm from each plane.
    let FittedFeature::Plane { p, n } = *ev_opp.output().as_feature().unwrap() else { panic!("expected Plane") };
    assert!((p[2] - 0.005).abs() <= 1e-15);
    assert_eq!(n, [0.0, 0.0, 1.0]);

    // Swapping either normal's sign gives bit-identical output bytes and digest.
    assert_eq!(ev_opp.output().canonical_bytes(), ev_same.output().canonical_bytes());
    assert_eq!(ev_opp.output().canonical_bytes(), ev_neg.output().canonical_bytes());
    assert_eq!(ev_opp.output_digest(), ev_same.output_digest());
}

#[test]
fn ac3_error_paths_5deg_planes_and_parallel_lines_and_required_branch() {
    let params = ConstructParams::new(1e-4, 1e-6).unwrap();

    // 1. Two planes 5 degrees apart without allow_intersecting -> IllConditioned(Measure::Angle).
    let deg5 = 5.0_f64.to_radians();
    let sp = SymmetryPlanes::new(id1(), id2()).unwrap();
    let g_5deg = [
        ElementGeom::Feature(FittedFeature::Plane { p: [0.0, 0.0, 0.0], n: [0.0, 0.0, 1.0] }),
        ElementGeom::Feature(FittedFeature::Plane { p: [0.0, 0.0, 0.0], n: [deg5.sin(), 0.0, deg5.cos()] }),
    ];
    let err = run(&sp, &g_5deg, &params).expect_err("5 deg planes without allow_intersecting must fail");
    match err {
        ConstructError::IllConditioned(cond) => assert_eq!(cond.measure(), Measure::Angle),
        other => panic!("expected IllConditioned(Angle), got {other:?}"),
    }

    // With allow_intersecting = 1 -> succeeds and returns the acute bisector plane.
    let params_allow = params.clone().with(PARAM_ALLOW_INTERSECTING, QuantityKind::Count, 1.0).unwrap();
    let ev = run(&sp, &g_5deg, &params_allow).expect("allow_intersecting=1 must return bisector");
    let FittedFeature::Plane { n, .. } = *ev.output().as_feature().unwrap() else { panic!("expected Plane") };
    let half = 2.5_f64.to_radians();
    assert!(dist3(n, [half.sin(), 0.0, half.cos()]) <= 1e-12);

    // 2. Two parallel lines -> bisect.lines returns Undefined { reason: Parallel }.
    let bl = BisectLines::new(id1(), id2()).unwrap();
    let p_acute = params.clone().with(PARAM_BRANCH, QuantityKind::Count, BisectBranch::Acute.as_param()).unwrap();
    let g_par = [
        ElementGeom::Feature(FittedFeature::Line3 { p: [0.0, 0.0, 0.0], d: [1.0, 0.0, 0.0] }),
        ElementGeom::Feature(FittedFeature::Line3 { p: [0.0, 1.0, 0.0], d: [-1.0, 0.0, 0.0] }),
    ];
    let err_par = run(&bl, &g_par, &p_acute).expect_err("parallel lines must be Undefined(Parallel)");
    assert_eq!(err_par, ConstructError::Undefined { reason: UndefinedReason::Parallel });

    // 3. Omitting required `branch` parameter -> MissingParam { name: "branch" }.
    let g_cross = [
        ElementGeom::Feature(FittedFeature::Line3 { p: [0.0, 0.0, 0.0], d: [1.0, 0.0, 0.0] }),
        ElementGeom::Feature(FittedFeature::Line3 { p: [0.0, 0.0, 0.0], d: [0.0, 1.0, 0.0] }),
    ];
    let err_missing = run(&bl, &g_cross, &params).expect_err("missing branch must fail");
    assert_eq!(err_missing, ConstructError::MissingParam { name: PARAM_BRANCH });
}

#[test]
fn ac4_recompute_equivalence_harness_passes_for_every_variant() {
    let params = ConstructParams::new(1e-6, 1e-6).unwrap();
    let p_acute = params.clone().with(PARAM_BRANCH, QuantityKind::Count, BisectBranch::Acute.as_param()).unwrap();

    // MidpointPoints (also test feature centres: Sphere + Circle3!)
    let res = harness::check(
        || MidpointPoints::new(id1(), id2()).unwrap(),
        &params,
        &[
            ElementGeom::Feature(FittedFeature::Sphere { c: [1.0, 2.0, 3.0], r: 0.5 }),
            ElementGeom::Feature(FittedFeature::Circle3 { c: [3.0, 4.0, 5.0], n: [0.0, 0.0, 1.0], r: 0.2 }),
        ],
        &[
            ElementGeom::Feature(FittedFeature::Point { c: [-1.0, 0.0, 1.0] }),
            ElementGeom::Feature(FittedFeature::Point { c: [5.0, 6.0, 7.0] }),
        ],
    )
    .expect("MidpointPoints harness");
    assert!(res.is_ok());

    // SymmetryPlanes
    let res = harness::check(
        || SymmetryPlanes::new(id1(), id2()).unwrap(),
        &params,
        &[
            ElementGeom::Feature(FittedFeature::Plane { p: [0.0, 0.0, 0.0], n: [0.0, 0.0, 1.0] }),
            ElementGeom::Feature(FittedFeature::Plane { p: [0.0, 0.0, 0.02], n: [0.0, 0.0, -1.0] }),
        ],
        &[
            ElementGeom::Feature(FittedFeature::Plane { p: [0.0, 0.0, -0.01], n: [0.0, 0.0, 1.0] }),
            ElementGeom::Feature(FittedFeature::Plane { p: [0.0, 0.0, 0.04], n: [0.0, 0.0, 1.0] }),
        ],
    )
    .expect("SymmetryPlanes harness");
    assert!(res.is_ok());

    // SymmetryLines
    let res = harness::check(
        || SymmetryLines::new(id1(), id2()).unwrap(),
        &params,
        &[
            ElementGeom::Feature(FittedFeature::Line3 { p: [0.0, 0.0, 0.0], d: [1.0, 0.0, 0.0] }),
            ElementGeom::Feature(FittedFeature::Line3 { p: [0.0, 2.0, 0.0], d: [-1.0, 0.0, 0.0] }),
        ],
        &[
            ElementGeom::Feature(FittedFeature::Line3 { p: [0.0, -1.0, 0.0], d: [1.0, 0.0, 0.0] }),
            ElementGeom::Feature(FittedFeature::Line3 { p: [0.0, 4.0, 0.0], d: [1.0, 0.0, 0.0] }),
        ],
    )
    .expect("SymmetryLines harness");
    assert!(res.is_ok());

    // SymmetryCircles
    let res = harness::check(
        || SymmetryCircles::new(id1(), id2()).unwrap(),
        &params,
        &[
            ElementGeom::Feature(FittedFeature::Circle3 { c: [0.0, 0.0, 0.0], n: [0.0, 0.0, 1.0], r: 0.1 }),
            ElementGeom::Feature(FittedFeature::Circle3 { c: [0.0, 0.0, 0.2], n: [0.0, 0.0, -1.0], r: 0.3 }),
        ],
        &[
            ElementGeom::Feature(FittedFeature::Circle3 { c: [0.0, 0.0, -0.1], n: [0.0, 0.0, 1.0], r: 0.2 }),
            ElementGeom::Feature(FittedFeature::Circle3 { c: [0.0, 0.0, 0.5], n: [0.0, 0.0, 1.0], r: 0.4 }),
        ],
    )
    .expect("SymmetryCircles harness");
    assert!(res.is_ok());

    // BisectLines
    let res = harness::check(
        || BisectLines::new(id1(), id2()).unwrap(),
        &p_acute,
        &[
            ElementGeom::Feature(FittedFeature::Line3 { p: [0.0, 0.0, 0.0], d: [1.0, 0.0, 0.0] }),
            ElementGeom::Feature(FittedFeature::Line3 { p: [0.0, 0.0, 0.0], d: [0.0, 1.0, 0.0] }),
        ],
        &[
            ElementGeom::Feature(FittedFeature::Line3 { p: [1.0, 0.0, 0.0], d: [1.0, 0.1, 0.0] }),
            ElementGeom::Feature(FittedFeature::Line3 { p: [0.0, 1.0, 0.0], d: [0.1, 1.0, 0.0] }),
        ],
    )
    .expect("BisectLines harness");
    assert!(res.is_ok());

    // BisectPlanes
    let res = harness::check(
        || BisectPlanes::new(id1(), id2()).unwrap(),
        &p_acute,
        &[
            ElementGeom::Feature(FittedFeature::Plane { p: [0.0, 0.0, 0.0], n: [1.0, 0.0, 0.0] }),
            ElementGeom::Feature(FittedFeature::Plane { p: [0.0, 0.0, 0.0], n: [0.0, 1.0, 0.0] }),
        ],
        &[
            ElementGeom::Feature(FittedFeature::Plane { p: [1.0, 0.0, 0.0], n: [1.0, 0.2, 0.0] }),
            ElementGeom::Feature(FittedFeature::Plane { p: [0.0, 1.0, 0.0], n: [0.2, 1.0, 0.0] }),
        ],
    )
    .expect("BisectPlanes harness");
    assert!(res.is_ok());
}
