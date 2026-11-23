//! Acceptance tests AC-1..AC-4 for `KRN.CONS.OPS.INTLIN` (`fid_construct::ops_intlin`).
//! Uses `include_str!` so every test runs identically on x86-64, ARM64 and `wasm32-wasip1`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_construct::contract::{
    harness, run, ConstructError, ConstructParams, Construction, ElementGeom, ElementId, Inputs, Measure, QuantityKind,
    UndefinedReason,
};
use fid_construct::ops_intlin::{
    IntersectLineLine, IntersectLinePlane, IntersectPlane3, IntersectPlanePlane, SkewRule, PARAM_RULE,
};
use fid_fit::contract::FittedFeature;

const LINE_LINE_TSV: &str = include_str!("../../../fixtures/cons/intlin-v1/line_line.tsv");
const LINE_PLANE_TSV: &str = include_str!("../../../fixtures/cons/intlin-v1/line_plane.tsv");
const PLANE_PLANE_TSV: &str = include_str!("../../../fixtures/cons/intlin-v1/plane_plane.tsv");
const PLANE3_TSV: &str = include_str!("../../../fixtures/cons/intlin-v1/plane3.tsv");

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
fn ac1_all_four_variants_match_1000_seeded_cases_including_1km_from_origin() {
    let params = ConstructParams::new(1e-8, 1e-8).unwrap();

    // 1. IntersectLineLine (1,000 cases)
    let ll = IntersectLineLine::new(id(1), id(2)).unwrap();
    let rows = parse_rows(LINE_LINE_TSV);
    assert_eq!(rows.len(), 1000);
    for (i, r) in rows.iter().enumerate() {
        let g = [
            ElementGeom::Feature(FittedFeature::Line3 { p: v3(r, 0), d: v3(r, 3) }),
            ElementGeom::Feature(FittedFeature::Line3 { p: v3(r, 6), d: v3(r, 9) }),
        ];
        let decl = ll.declare(&params);
        let inp = Inputs::bind(&decl, &g).unwrap();
        let (el, diag) = ll.evaluate_with_diagnostics(&inp, &params).unwrap();
        let FittedFeature::Point { c } = *el.as_feature().unwrap() else { panic!("expected Point") };
        let exp = v3(r, 12);
        let exp_skew = r[15];
        assert!(dist3(c, exp) <= 1e-12 * scale3(exp), "line_line case {i}: got {c:?} exp {exp:?}");
        assert!((diag.skew_distance - exp_skew).abs() <= 1e-12 * scale3(exp), "line_line skew {i}");
    }

    // 2D Line2 + Line2 check
    let g2 = [
        ElementGeom::Feature(FittedFeature::Line2 { p: [0.0, 0.0], d: [1.0, 0.0] }),
        ElementGeom::Feature(FittedFeature::Line2 { p: [3.0, -5.0], d: [0.0, 1.0] }),
    ];
    let ev2 = run(&ll, &g2, &params).unwrap();
    let FittedFeature::Point { c } = *ev2.output().as_feature().unwrap() else { panic!() };
    assert!(dist3(c, [3.0, 0.0, 0.0]) <= 1e-15);

    // 2. IntersectLinePlane (1,000 cases)
    let lp = IntersectLinePlane::new(id(1), id(2)).unwrap();
    let rows = parse_rows(LINE_PLANE_TSV);
    assert_eq!(rows.len(), 1000);
    for (i, r) in rows.iter().enumerate() {
        let g = [
            ElementGeom::Feature(FittedFeature::Line3 { p: v3(r, 0), d: v3(r, 3) }),
            ElementGeom::Feature(FittedFeature::Plane { p: v3(r, 6), n: v3(r, 9) }),
        ];
        let ev = run(&lp, &g, &params).unwrap();
        let FittedFeature::Point { c } = *ev.output().as_feature().unwrap() else { panic!("expected Point") };
        let exp = v3(r, 12);
        assert!(dist3(c, exp) <= 1e-12 * scale3(exp), "line_plane case {i}: got {c:?} exp {exp:?}");
    }

    // 3. IntersectPlanePlane (1,000 cases)
    let pp = IntersectPlanePlane::new(id(1), id(2)).unwrap();
    let rows = parse_rows(PLANE_PLANE_TSV);
    assert_eq!(rows.len(), 1000);
    for (i, r) in rows.iter().enumerate() {
        let g = [
            ElementGeom::Feature(FittedFeature::Plane { p: v3(r, 0), n: v3(r, 3) }),
            ElementGeom::Feature(FittedFeature::Plane { p: v3(r, 6), n: v3(r, 9) }),
        ];
        let ev = run(&pp, &g, &params).unwrap();
        let FittedFeature::Line3 { p, d } = *ev.output().as_feature().unwrap() else { panic!("expected Line3") };
        let exp_p = v3(r, 12);
        let exp_d = v3(r, 15);
        assert!(dist3(p, exp_p) <= 1e-12 * scale3(exp_p), "plane_plane p {i}: got {p:?} exp {exp_p:?}");
        assert!(dist3(d, exp_d) <= 1e-12, "plane_plane d {i}: got {d:?} exp {exp_d:?}");
    }

    // 4. IntersectPlane3 (1,000 cases)
    let p3 = IntersectPlane3::new(id(1), id(2), id(3)).unwrap();
    let rows = parse_rows(PLANE3_TSV);
    assert_eq!(rows.len(), 1000);
    for (i, r) in rows.iter().enumerate() {
        let g = [
            ElementGeom::Feature(FittedFeature::Plane { p: v3(r, 0), n: v3(r, 3) }),
            ElementGeom::Feature(FittedFeature::Plane { p: v3(r, 6), n: v3(r, 9) }),
            ElementGeom::Feature(FittedFeature::Plane { p: v3(r, 12), n: v3(r, 15) }),
        ];
        let ev = run(&p3, &g, &params).unwrap();
        let FittedFeature::Point { c } = *ev.output().as_feature().unwrap() else { panic!("expected Point") };
        let exp = v3(r, 18);
        assert!(dist3(c, exp) <= 1e-12 * scale3(exp), "plane3 case {i}: got {c:?} exp {exp:?}");
    }
}

#[test]
fn ac2_skew_lines_default_midpoint_and_strict_rule() {
    let ll = IntersectLineLine::new(id(1), id(2)).unwrap();
    // Two orthogonal skew lines separated by 0.3 mm along Z.
    let g = [
        ElementGeom::Feature(FittedFeature::Line3 { p: [0.0, 0.0, 0.0], d: [1.0, 0.0, 0.0] }),
        ElementGeom::Feature(FittedFeature::Line3 { p: [0.0, 0.0, 0.0003], d: [0.0, 1.0, 0.0] }),
    ];
    let p_default = ConstructParams::new(1e-8, 1e-6).unwrap();
    let decl = ll.declare(&p_default);
    let inp = Inputs::bind(&decl, &g).unwrap();
    let (el, diag) = ll.evaluate_with_diagnostics(&inp, &p_default).unwrap();
    let FittedFeature::Point { c } = *el.as_feature().unwrap() else { panic!() };
    assert!(dist3(c, [0.0, 0.0, 0.00015]) <= 1e-15);
    assert!((diag.skew_distance - 0.0003).abs() <= 1e-15);

    // Strict rule with 1 micrometre threshold returns Undefined{reason: skew}.
    let p_strict = ConstructParams::new(1e-8, 1e-6)
        .unwrap()
        .with(PARAM_RULE, QuantityKind::Count, SkewRule::Strict.as_param())
        .unwrap();
    let err = run(&ll, &g, &p_strict).unwrap_err();
    assert_eq!(err, ConstructError::Undefined { reason: UndefinedReason::Skew });
}

#[test]
fn ac3_near_parallel_planes_return_ill_conditioned_angle() {
    let pp = IntersectPlanePlane::new(id(1), id(2)).unwrap();
    let eps = 1e-10_f64;
    let g = [
        ElementGeom::Feature(FittedFeature::Plane { p: [0.0, 0.0, 0.0], n: [0.0, 0.0, 1.0] }),
        ElementGeom::Feature(FittedFeature::Plane { p: [0.0, 0.0, 1.0], n: [eps.sin(), 0.0, eps.cos()] }),
    ];
    let params = ConstructParams::new(1e-8, 1e-8).unwrap();
    let err = run(&pp, &g, &params).unwrap_err();
    match err {
        ConstructError::IllConditioned(cond) => {
            assert_eq!(cond.measure(), Measure::Angle);
            assert!((cond.value() - eps).abs() <= 1e-15);
            assert_eq!(cond.threshold(), 1e-8);
        }
        other => panic!("expected IllConditioned(Angle), got {other:?}"),
    }
}

#[test]
fn ac4_recompute_equivalence_harness_passes_all_four_variants() {
    let params = ConstructParams::new(1e-8, 1e-8).unwrap();

    let ll_in = [
        ElementGeom::Feature(FittedFeature::Line3 { p: [1000.0, 0.0, 0.0], d: [1.0, 0.0, 0.0] }),
        ElementGeom::Feature(FittedFeature::Line3 { p: [1000.0, 2.0, 0.001], d: [0.0, 1.0, 0.0] }),
    ];
    let ll_rep = [
        ElementGeom::Feature(FittedFeature::Line3 { p: [1000.0, 0.5, 0.0], d: [1.0, 0.2, 0.0] }),
        ElementGeom::Feature(FittedFeature::Line3 { p: [1001.0, 2.0, 0.002], d: [0.1, 1.0, 0.0] }),
    ];
    harness::check(|| IntersectLineLine::new(id(1), id(2)).unwrap(), &params, &ll_in, &ll_rep).unwrap().unwrap();

    let lp_in = [
        ElementGeom::Feature(FittedFeature::Line3 { p: [1000.0, 1.0, 2.0], d: [0.0, 0.0, 1.0] }),
        ElementGeom::Feature(FittedFeature::Plane { p: [1000.0, 0.0, 5.0], n: [0.0, 0.0, 1.0] }),
    ];
    let lp_rep = [
        ElementGeom::Feature(FittedFeature::Line3 { p: [1000.0, 2.0, 2.0], d: [0.1, 0.0, 1.0] }),
        ElementGeom::Feature(FittedFeature::Plane { p: [1000.0, 0.0, 7.0], n: [0.0, 0.1, 1.0] }),
    ];
    harness::check(|| IntersectLinePlane::new(id(1), id(2)).unwrap(), &params, &lp_in, &lp_rep).unwrap().unwrap();

    let pp_in = [
        ElementGeom::Feature(FittedFeature::Plane { p: [1000.0, 0.0, 0.0], n: [1.0, 0.0, 0.0] }),
        ElementGeom::Feature(FittedFeature::Plane { p: [1000.0, 1.0, 0.0], n: [0.0, 1.0, 0.0] }),
    ];
    let pp_rep = [
        ElementGeom::Feature(FittedFeature::Plane { p: [1001.0, 0.0, 0.0], n: [1.0, 0.1, 0.0] }),
        ElementGeom::Feature(FittedFeature::Plane { p: [1000.0, 2.0, 0.0], n: [0.0, 1.0, 0.1] }),
    ];
    harness::check(|| IntersectPlanePlane::new(id(1), id(2)).unwrap(), &params, &pp_in, &pp_rep).unwrap().unwrap();

    let p3_in = [
        ElementGeom::Feature(FittedFeature::Plane { p: [1000.0, 0.0, 0.0], n: [1.0, 0.0, 0.0] }),
        ElementGeom::Feature(FittedFeature::Plane { p: [0.0, 1000.0, 0.0], n: [0.0, 1.0, 0.0] }),
        ElementGeom::Feature(FittedFeature::Plane { p: [0.0, 0.0, 1000.0], n: [0.0, 0.0, 1.0] }),
    ];
    let p3_rep = [
        ElementGeom::Feature(FittedFeature::Plane { p: [1002.0, 0.0, 0.0], n: [1.0, 0.05, 0.0] }),
        ElementGeom::Feature(FittedFeature::Plane { p: [0.0, 1003.0, 0.0], n: [0.0, 1.0, 0.05] }),
        ElementGeom::Feature(FittedFeature::Plane { p: [0.0, 0.0, 1004.0], n: [0.05, 0.0, 1.0] }),
    ];
    harness::check(|| IntersectPlane3::new(id(1), id(2), id(3)).unwrap(), &params, &p3_in, &p3_rep).unwrap().unwrap();
}
