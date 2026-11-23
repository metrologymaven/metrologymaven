//! Acceptance tests AC-1..AC-5 for `KRN.CONS.ELEM.PRIM` (`fid_construct::elem_prim`).
//! Uses `include_str!` so every test runs identically on x86-64, ARM64 and `wasm32-wasip1`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_construct::contract::{
    harness, run, ConstructError, ConstructParams, ElementGeom, ElementId, InputKind, Measure, QuantityKind,
    UndefinedReason,
};
use fid_construct::elem_prim::{
    command_rows, CircleCentreAxisRadius, CircleThreePoints, LineAxisOf, LineNormalThrough, LinePointDirection,
    LineTwoPoints, PlaneLinePoint, PlanePointNormal, PlaneThreePoints, PlaneTwoLines, PointAlongAxis, PointApexOf,
    PointCentreOf, PointCoords, SphereCentreRadius, SphereFourPoints, CMD_BUILD_CIRCLE, CMD_BUILD_LINE,
    CMD_BUILD_PLANE, CMD_BUILD_POINT, CMD_BUILD_SPHERE, PARAM_DISTANCE, PARAM_RADIUS, PARAM_X, PARAM_Y, PARAM_Z,
};
use fid_fit::contract::{FittedFeature, Primitive};
use fid_kci::parse_card;
use fid_types::CommandId;

const T_PT_COORDS: &str = include_str!("../../../fixtures/cons/prim-defs-v1/point_coords.tsv");
const T_PT_CENTRE: &str = include_str!("../../../fixtures/cons/prim-defs-v1/point_centre_of.tsv");
const T_PT_APEX: &str = include_str!("../../../fixtures/cons/prim-defs-v1/point_apex_of.tsv");
const T_PT_ALONG: &str = include_str!("../../../fixtures/cons/prim-defs-v1/point_along_axis.tsv");
const T_LN_TWO_PTS: &str = include_str!("../../../fixtures/cons/prim-defs-v1/line_two_points.tsv");
const T_LN_AXIS: &str = include_str!("../../../fixtures/cons/prim-defs-v1/line_axis_of.tsv");
const T_LN_PT_DIR: &str = include_str!("../../../fixtures/cons/prim-defs-v1/line_point_direction.tsv");
const T_LN_NORM: &str = include_str!("../../../fixtures/cons/prim-defs-v1/line_normal_through.tsv");
const T_PL_3PTS: &str = include_str!("../../../fixtures/cons/prim-defs-v1/plane_three_points.tsv");
const T_PL_PT_N: &str = include_str!("../../../fixtures/cons/prim-defs-v1/plane_point_normal.tsv");
const T_PL_LN_PT: &str = include_str!("../../../fixtures/cons/prim-defs-v1/plane_line_point.tsv");
const T_PL_2LNS: &str = include_str!("../../../fixtures/cons/prim-defs-v1/plane_two_lines.tsv");
const T_CI_3PTS: &str = include_str!("../../../fixtures/cons/prim-defs-v1/circle_three_points.tsv");
const T_CI_CAR: &str = include_str!("../../../fixtures/cons/prim-defs-v1/circle_centre_axis_radius.tsv");
const T_SP_4PTS: &str = include_str!("../../../fixtures/cons/prim-defs-v1/sphere_four_points.tsv");
const T_SP_CR: &str = include_str!("../../../fixtures/cons/prim-defs-v1/sphere_centre_radius.tsv");

const CARDS: [&str; 16] = [
    include_str!("../cards/construct.build_point.coords@1.0.0.toml"),
    include_str!("../cards/construct.build_point.centre_of@1.0.0.toml"),
    include_str!("../cards/construct.build_point.apex_of@1.0.0.toml"),
    include_str!("../cards/construct.build_point.along_axis@1.0.0.toml"),
    include_str!("../cards/construct.build_line.two_points@1.0.0.toml"),
    include_str!("../cards/construct.build_line.axis_of@1.0.0.toml"),
    include_str!("../cards/construct.build_line.point_direction@1.0.0.toml"),
    include_str!("../cards/construct.build_line.normal_through@1.0.0.toml"),
    include_str!("../cards/construct.build_plane.three_points@1.0.0.toml"),
    include_str!("../cards/construct.build_plane.point_normal@1.0.0.toml"),
    include_str!("../cards/construct.build_plane.line_point@1.0.0.toml"),
    include_str!("../cards/construct.build_plane.two_lines@1.0.0.toml"),
    include_str!("../cards/construct.build_circle.three_points@1.0.0.toml"),
    include_str!("../cards/construct.build_circle.centre_axis_radius@1.0.0.toml"),
    include_str!("../cards/construct.build_sphere.four_points@1.0.0.toml"),
    include_str!("../cards/construct.build_sphere.centre_radius@1.0.0.toml"),
];

fn id(n: u8) -> ElementId {
    ElementId::parse(&format!("00000000-0000-7000-8000-0000000000{n:02x}")).unwrap()
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

fn assert_dir_match(got: [f64; 3], exp: [f64; 3], ctx: &str) {
    assert!(dist3(got, exp) <= 1e-12, "{ctx}: got {got:?} exp {exp:?}");
}

#[test]
fn ac1_all_sixteen_variants_match_500_seeded_cases_within_1e_12() {
    let params = ConstructParams::new(1e-6, 1e-6).unwrap();

    // 1. PointCoords
    let pc = PointCoords::new().unwrap();
    for (i, r) in parse_rows(T_PT_COORDS).iter().enumerate() {
        let p = params
            .clone()
            .with(PARAM_X, QuantityKind::Length, r[0])
            .unwrap()
            .with(PARAM_Y, QuantityKind::Length, r[1])
            .unwrap()
            .with(PARAM_Z, QuantityKind::Length, r[2])
            .unwrap();
        let ev = run(&pc, &[], &p).unwrap();
        let FittedFeature::Point { c } = *ev.output().as_feature().unwrap() else { panic!() };
        assert!(dist3(c, v3(r, 3)) <= 1e-12 * scale3(v3(r, 3)), "point.coords {i}");
    }

    // 2. PointCentreOf
    let pco = PointCentreOf::new(id(1)).unwrap();
    for (i, r) in parse_rows(T_PT_CENTRE).iter().enumerate() {
        let g = [ElementGeom::Feature(FittedFeature::Sphere { c: v3(r, 0), r: r[3] })];
        let ev = run(&pco, &g, &params).unwrap();
        let FittedFeature::Point { c } = *ev.output().as_feature().unwrap() else { panic!() };
        assert!(dist3(c, v3(r, 4)) <= 1e-12 * scale3(v3(r, 4)), "point.centre_of {i}");
    }

    // 3. PointApexOf
    let pao = PointApexOf::new(id(1)).unwrap();
    for (i, r) in parse_rows(T_PT_APEX).iter().enumerate() {
        let g = [ElementGeom::Feature(FittedFeature::Cone { apex: v3(r, 0), d: v3(r, 3), half_angle: r[6] })];
        let ev = run(&pao, &g, &params).unwrap();
        let FittedFeature::Point { c } = *ev.output().as_feature().unwrap() else { panic!() };
        assert!(dist3(c, v3(r, 7)) <= 1e-12 * scale3(v3(r, 7)), "point.apex_of {i}");
    }

    // 4. PointAlongAxis
    let paa = PointAlongAxis::new(id(1)).unwrap();
    for (i, r) in parse_rows(T_PT_ALONG).iter().enumerate() {
        let g = [ElementGeom::Feature(FittedFeature::Line3 { p: v3(r, 0), d: v3(r, 3) })];
        let p = params.clone().with(PARAM_DISTANCE, QuantityKind::Length, r[6]).unwrap();
        let ev = run(&paa, &g, &p).unwrap();
        let FittedFeature::Point { c } = *ev.output().as_feature().unwrap() else { panic!() };
        let exp = v3(r, 7);
        assert!(dist3(c, exp) <= 1e-12 * scale3(exp), "point.along_axis {i}");
    }

    // 5. LineTwoPoints
    let l2p = LineTwoPoints::new(id(1), id(2)).unwrap();
    for (i, r) in parse_rows(T_LN_TWO_PTS).iter().enumerate() {
        let g = [
            ElementGeom::Feature(FittedFeature::Point { c: v3(r, 0) }),
            ElementGeom::Feature(FittedFeature::Point { c: v3(r, 3) }),
        ];
        let ev = run(&l2p, &g, &params).unwrap();
        let FittedFeature::Line3 { p, d } = *ev.output().as_feature().unwrap() else { panic!() };
        assert!(dist3(p, v3(r, 6)) <= 1e-12 * scale3(v3(r, 6)), "line.two_points p {i}");
        assert_dir_match(d, v3(r, 9), &format!("line.two_points d {i}"));
    }

    // 6. LineAxisOf
    let lao = LineAxisOf::new(id(1)).unwrap();
    for (i, r) in parse_rows(T_LN_AXIS).iter().enumerate() {
        let g = [ElementGeom::Feature(FittedFeature::Cylinder { p: v3(r, 0), d: v3(r, 3), r: r[6] })];
        let ev = run(&lao, &g, &params).unwrap();
        let FittedFeature::Line3 { p, d } = *ev.output().as_feature().unwrap() else { panic!() };
        assert!(dist3(p, v3(r, 7)) <= 1e-12 * scale3(v3(r, 7)), "line.axis_of p {i}");
        assert_dir_match(d, v3(r, 10), &format!("line.axis_of d {i}"));
    }

    // 7. LinePointDirection
    let lpd = LinePointDirection::new(id(1), id(2)).unwrap();
    for (i, r) in parse_rows(T_LN_PT_DIR).iter().enumerate() {
        let g = [
            ElementGeom::Feature(FittedFeature::Point { c: v3(r, 0) }),
            ElementGeom::Feature(FittedFeature::Line3 { p: v3(r, 3), d: v3(r, 6) }),
        ];
        let ev = run(&lpd, &g, &params).unwrap();
        let FittedFeature::Line3 { p, d } = *ev.output().as_feature().unwrap() else { panic!() };
        assert!(dist3(p, v3(r, 9)) <= 1e-12 * scale3(v3(r, 9)), "line.point_direction p {i}");
        assert_dir_match(d, v3(r, 12), &format!("line.point_direction d {i}"));
    }

    // 8. LineNormalThrough
    let lnt = LineNormalThrough::new(id(1), id(2)).unwrap();
    for (i, r) in parse_rows(T_LN_NORM).iter().enumerate() {
        let g = [
            ElementGeom::Feature(FittedFeature::Plane { p: v3(r, 0), n: v3(r, 3) }),
            ElementGeom::Feature(FittedFeature::Point { c: v3(r, 6) }),
        ];
        let ev = run(&lnt, &g, &params).unwrap();
        let FittedFeature::Line3 { p, d } = *ev.output().as_feature().unwrap() else { panic!() };
        assert!(dist3(p, v3(r, 9)) <= 1e-12 * scale3(v3(r, 9)), "line.normal_through p {i}");
        assert_dir_match(d, v3(r, 12), &format!("line.normal_through d {i}"));
    }

    // 9. PlaneThreePoints
    let pl3 = PlaneThreePoints::new(id(1), id(2), id(3)).unwrap();
    for (i, r) in parse_rows(T_PL_3PTS).iter().enumerate() {
        let g = [
            ElementGeom::Feature(FittedFeature::Point { c: v3(r, 0) }),
            ElementGeom::Feature(FittedFeature::Point { c: v3(r, 3) }),
            ElementGeom::Feature(FittedFeature::Point { c: v3(r, 6) }),
        ];
        let ev = run(&pl3, &g, &params).unwrap();
        let FittedFeature::Plane { p, n } = *ev.output().as_feature().unwrap() else { panic!() };
        assert!(dist3(p, v3(r, 9)) <= 1e-12 * scale3(v3(r, 9)), "plane.three_points p {i}");
        assert_dir_match(n, v3(r, 12), &format!("plane.three_points n {i}"));
    }

    // 10. PlanePointNormal
    let pln = PlanePointNormal::new(id(1), id(2)).unwrap();
    for (i, r) in parse_rows(T_PL_PT_N).iter().enumerate() {
        let g = [
            ElementGeom::Feature(FittedFeature::Point { c: v3(r, 0) }),
            ElementGeom::Feature(FittedFeature::Line3 { p: v3(r, 3), d: v3(r, 6) }),
        ];
        let ev = run(&pln, &g, &params).unwrap();
        let FittedFeature::Plane { p, n } = *ev.output().as_feature().unwrap() else { panic!() };
        assert!(dist3(p, v3(r, 9)) <= 1e-12 * scale3(v3(r, 9)), "plane.point_normal p {i}");
        assert_dir_match(n, v3(r, 12), &format!("plane.point_normal n {i}"));
    }

    // 11. PlaneLinePoint
    let pllp = PlaneLinePoint::new(id(1), id(2)).unwrap();
    for (i, r) in parse_rows(T_PL_LN_PT).iter().enumerate() {
        let g = [
            ElementGeom::Feature(FittedFeature::Line3 { p: v3(r, 0), d: v3(r, 3) }),
            ElementGeom::Feature(FittedFeature::Point { c: v3(r, 6) }),
        ];
        let ev = run(&pllp, &g, &params).unwrap();
        let FittedFeature::Plane { p, n } = *ev.output().as_feature().unwrap() else { panic!() };
        assert!(dist3(p, v3(r, 9)) <= 1e-12 * scale3(v3(r, 9)), "plane.line_point p {i}");
        assert_dir_match(n, v3(r, 12), &format!("plane.line_point n {i}"));
    }

    // 12. PlaneTwoLines (250 intersecting + 250 parallel)
    let pl2l = PlaneTwoLines::new(id(1), id(2)).unwrap();
    for (i, r) in parse_rows(T_PL_2LNS).iter().enumerate() {
        let g = [
            ElementGeom::Feature(FittedFeature::Line3 { p: v3(r, 0), d: v3(r, 3) }),
            ElementGeom::Feature(FittedFeature::Line3 { p: v3(r, 6), d: v3(r, 9) }),
        ];
        let ev = run(&pl2l, &g, &params).unwrap();
        let FittedFeature::Plane { p, n } = *ev.output().as_feature().unwrap() else { panic!() };
        let exp_p = v3(r, 12);
        let exp_n = v3(r, 15);
        assert_dir_match(n, exp_n, &format!("plane.two_lines n {i}"));
        let plane_off = dot3(sub3(p, exp_p), exp_n).abs();
        assert!(plane_off <= 1e-12 * scale3(exp_p), "plane.two_lines plane offset {i}: {plane_off:e}");
    }

    // 13. CircleThreePoints
    let c3p = CircleThreePoints::new(id(1), id(2), id(3)).unwrap();
    for (i, r) in parse_rows(T_CI_3PTS).iter().enumerate() {
        let g = [
            ElementGeom::Feature(FittedFeature::Point { c: v3(r, 0) }),
            ElementGeom::Feature(FittedFeature::Point { c: v3(r, 3) }),
            ElementGeom::Feature(FittedFeature::Point { c: v3(r, 6) }),
        ];
        let ev = run(&c3p, &g, &params).unwrap();
        let FittedFeature::Circle3 { c, n, r: rad } = *ev.output().as_feature().unwrap() else { panic!() };
        let exp_c = v3(r, 9);
        let exp_n = v3(r, 12);
        let exp_r = r[15];
        assert!(dist3(c, exp_c) <= 1e-12 * scale3(exp_c), "circle.three_points c {i}");
        assert_dir_match(n, exp_n, &format!("circle.three_points n {i}"));
        assert!((rad - exp_r).abs() <= 1e-12 * exp_r.max(1.0), "circle.three_points r {i}");
    }

    // 14. CircleCentreAxisRadius
    let ccar = CircleCentreAxisRadius::new(id(1), id(2)).unwrap();
    for (i, r) in parse_rows(T_CI_CAR).iter().enumerate() {
        let g = [
            ElementGeom::Feature(FittedFeature::Point { c: v3(r, 0) }),
            ElementGeom::Feature(FittedFeature::Line3 { p: v3(r, 3), d: v3(r, 6) }),
        ];
        let p = params.clone().with(PARAM_RADIUS, QuantityKind::Length, r[9]).unwrap();
        let ev = run(&ccar, &g, &p).unwrap();
        let FittedFeature::Circle3 { c, n, r: rad } = *ev.output().as_feature().unwrap() else { panic!() };
        assert!(dist3(c, v3(r, 10)) <= 1e-12 * scale3(v3(r, 10)), "circle.car c {i}");
        assert_dir_match(n, v3(r, 13), &format!("circle.car n {i}"));
        assert!((rad - r[16]).abs() <= 1e-12 * r[16].max(1.0));
    }

    // 15. SphereFourPoints
    let s4p = SphereFourPoints::new(id(1), id(2), id(3), id(4)).unwrap();
    for (i, r) in parse_rows(T_SP_4PTS).iter().enumerate() {
        let g = [
            ElementGeom::Feature(FittedFeature::Point { c: v3(r, 0) }),
            ElementGeom::Feature(FittedFeature::Point { c: v3(r, 3) }),
            ElementGeom::Feature(FittedFeature::Point { c: v3(r, 6) }),
            ElementGeom::Feature(FittedFeature::Point { c: v3(r, 9) }),
        ];
        let ev = run(&s4p, &g, &params).unwrap();
        let FittedFeature::Sphere { c, r: rad } = *ev.output().as_feature().unwrap() else { panic!() };
        let exp_c = v3(r, 12);
        let exp_r = r[15];
        assert!(dist3(c, exp_c) <= 1e-12 * scale3(exp_c), "sphere.four_points c {i}");
        assert!((rad - exp_r).abs() <= 1e-12 * exp_r.max(1.0), "sphere.four_points r {i}");
    }

    // 16. SphereCentreRadius
    let scr = SphereCentreRadius::new(id(1)).unwrap();
    for (i, r) in parse_rows(T_SP_CR).iter().enumerate() {
        let g = [ElementGeom::Feature(FittedFeature::Point { c: v3(r, 0) })];
        let p = params.clone().with(PARAM_RADIUS, QuantityKind::Length, r[3]).unwrap();
        let ev = run(&scr, &g, &p).unwrap();
        let FittedFeature::Sphere { c, r: rad } = *ev.output().as_feature().unwrap() else { panic!() };
        assert!(dist3(c, v3(r, 4)) <= 1e-12 * scale3(v3(r, 4)), "sphere.cr c {i}");
        assert!((rad - r[7]).abs() <= 1e-12 * r[7].max(1.0));
    }
}

#[test]
fn ac2_ac3_and_error_paths() {
    // AC-2: three points 1e-13 m from collinear with threshold 1e-9 m -> IllConditioned(Distance).
    let pl3 = PlaneThreePoints::new(id(1), id(2), id(3)).unwrap();
    let params = ConstructParams::new(1e-6, 1e-9).unwrap();
    let near_col = [
        ElementGeom::Feature(FittedFeature::Point { c: [0.0, 0.0, 0.0] }),
        ElementGeom::Feature(FittedFeature::Point { c: [1.0, 0.0, 0.0] }),
        ElementGeom::Feature(FittedFeature::Point { c: [0.5, 1e-13, 0.0] }),
    ];
    match run(&pl3, &near_col, &params).unwrap_err() {
        ConstructError::IllConditioned(cond) => {
            assert_eq!(cond.measure(), Measure::Distance);
            assert!(cond.value() < 1e-9 && cond.value() > 0.0);
            assert_eq!(cond.threshold(), 1e-9);
        }
        other => panic!("expected IllConditioned, got {other:?}"),
    }

    // AC-3: sphere.four_points with 3 inputs -> InputCount { expected: 4, got: 3 }.
    let s4p = SphereFourPoints::new(id(1), id(2), id(3), id(4)).unwrap();
    let three_pts = [
        ElementGeom::Feature(FittedFeature::Point { c: [1.0, 0.0, 0.0] }),
        ElementGeom::Feature(FittedFeature::Point { c: [0.0, 1.0, 0.0] }),
        ElementGeom::Feature(FittedFeature::Point { c: [0.0, 0.0, 1.0] }),
    ];
    assert_eq!(run(&s4p, &three_pts, &params).unwrap_err(), ConstructError::InputCount { expected: 4, got: 3 });

    // Coplanar four points for sphere -> Undefined(ZeroLength); near-coplanar -> IllConditioned.
    let coplanar = [
        ElementGeom::Feature(FittedFeature::Point { c: [0.0, 0.0, 0.0] }),
        ElementGeom::Feature(FittedFeature::Point { c: [1.0, 0.0, 0.0] }),
        ElementGeom::Feature(FittedFeature::Point { c: [0.0, 1.0, 0.0] }),
        ElementGeom::Feature(FittedFeature::Point { c: [1.0, 1.0, 0.0] }),
    ];
    assert_eq!(
        run(&s4p, &coplanar, &params).unwrap_err(),
        ConstructError::Undefined { reason: UndefinedReason::ZeroLength }
    );
    let near_coplanar = [
        ElementGeom::Feature(FittedFeature::Point { c: [0.0, 0.0, 0.0] }),
        ElementGeom::Feature(FittedFeature::Point { c: [1.0, 0.0, 0.0] }),
        ElementGeom::Feature(FittedFeature::Point { c: [0.0, 1.0, 0.0] }),
        ElementGeom::Feature(FittedFeature::Point { c: [1.0, 1.0, 1e-12] }),
    ];
    assert!(matches!(run(&s4p, &near_coplanar, &params).unwrap_err(), ConstructError::IllConditioned(_)));

    // WrongInputKind naming input index.
    let pao = PointApexOf::new(id(1)).unwrap();
    let bad = [ElementGeom::Feature(FittedFeature::Point { c: [0.0, 0.0, 0.0] })];
    assert_eq!(
        run(&pao, &bad, &params).unwrap_err(),
        ConstructError::WrongInputKind {
            index: 0,
            expected: InputKind::Feature(Primitive::Cone),
            got: InputKind::Feature(Primitive::Point),
        }
    );

    // Skew lines fail PlaneTwoLines with Undefined(Skew).
    let pl2l = PlaneTwoLines::new(id(1), id(2)).unwrap();
    let skew = [
        ElementGeom::Feature(FittedFeature::Line3 { p: [0.0, 0.0, 0.0], d: [1.0, 0.0, 0.0] }),
        ElementGeom::Feature(FittedFeature::Line3 { p: [0.0, 0.0, 1.0], d: [0.0, 1.0, 0.0] }),
    ];
    assert_eq!(run(&pl2l, &skew, &params).unwrap_err(), ConstructError::Undefined { reason: UndefinedReason::Skew });
}

#[test]
fn ac4_ac5_harness_replay_command_rows_and_spec_cards() {
    let params = ConstructParams::new(1e-6, 1e-6).unwrap();

    // Recompute harness across representative multi-input primitives (AC-4).
    let p_a = ElementGeom::Feature(FittedFeature::Point { c: [0.0, 0.0, 0.0] });
    let p_b = ElementGeom::Feature(FittedFeature::Point { c: [1.0, 0.0, 0.0] });
    let p_c = ElementGeom::Feature(FittedFeature::Point { c: [0.0, 1.0, 0.0] });
    let p_d = ElementGeom::Feature(FittedFeature::Point { c: [0.0, 0.0, 1.0] });
    let r_a = ElementGeom::Feature(FittedFeature::Point { c: [0.1, 0.0, 0.0] });
    let r_b = ElementGeom::Feature(FittedFeature::Point { c: [1.2, 0.0, 0.0] });
    let r_c = ElementGeom::Feature(FittedFeature::Point { c: [0.0, 1.3, 0.0] });
    let r_d = ElementGeom::Feature(FittedFeature::Point { c: [0.0, 0.0, 1.4] });

    let ev_pl = harness::check(
        || PlaneThreePoints::new(id(1), id(2), id(3)).unwrap(),
        &params,
        &[p_a.clone(), p_b.clone(), p_c.clone()],
        &[r_a.clone(), r_b.clone(), r_c.clone()],
    )
    .unwrap()
    .unwrap();
    let ev_ci = harness::check(
        || CircleThreePoints::new(id(1), id(2), id(3)).unwrap(),
        &params,
        &[p_a.clone(), p_b.clone(), p_c.clone()],
        &[r_a.clone(), r_b.clone(), r_c.clone()],
    )
    .unwrap()
    .unwrap();
    let ev_sp = harness::check(
        || SphereFourPoints::new(id(1), id(2), id(3), id(4)).unwrap(),
        &params,
        &[p_a.clone(), p_b.clone(), p_c.clone(), p_d],
        &[r_a, r_b, r_c, r_d],
    )
    .unwrap()
    .unwrap();

    // AC-5: committed construct.* journal events replay bit-identically.
    let cmd_pl = CommandId::parse(CMD_BUILD_PLANE).unwrap();
    let cmd_ci = CommandId::parse(CMD_BUILD_CIRCLE).unwrap();
    let cmd_sp = CommandId::parse(CMD_BUILD_SPHERE).unwrap();
    let e_pl_1 = ev_pl.event(&cmd_pl).unwrap();
    let e_pl_2 =
        run(&PlaneThreePoints::new(id(1), id(2), id(3)).unwrap(), &[p_a.clone(), p_b.clone(), p_c.clone()], &params)
            .unwrap()
            .event(&cmd_pl)
            .unwrap();
    assert_eq!(e_pl_1.payload_bytes(), e_pl_2.payload_bytes());
    assert_eq!(ev_ci.event(&cmd_ci).unwrap().event_type(), CMD_BUILD_CIRCLE);
    assert_eq!(ev_sp.event(&cmd_sp).unwrap().event_type(), CMD_BUILD_SPHERE);

    // All 5 command data rows are valid canonical JSON and cover all 16 algorithm IDs.
    let rows = command_rows().unwrap();
    assert_eq!(rows.len(), 5);
    let cmds: Vec<&str> = rows.iter().map(|r| r.command().as_str()).collect();
    assert_eq!(cmds, vec![CMD_BUILD_POINT, CMD_BUILD_LINE, CMD_BUILD_PLANE, CMD_BUILD_CIRCLE, CMD_BUILD_SPHERE]);
    let mut row_algs: Vec<String> = rows.iter().flat_map(|r| r.algorithms().iter().map(|a| a.to_string())).collect();
    row_algs.sort();
    assert_eq!(row_algs.len(), 16);

    // Every one of the 16 spec cards parses cleanly under `fid_kci::parse_card` and matches the rows.
    let mut card_ids: Vec<String> = CARDS.iter().map(|c| parse_card(c).unwrap().id.to_string()).collect();
    card_ids.sort();
    assert_eq!(card_ids, row_algs);
}
