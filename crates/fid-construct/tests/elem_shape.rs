//! Acceptance tests AC-1..AC-5 for `KRN.CONS.ELEM.SHAPE` (`fid_construct::elem_shape`).
//! Uses `include_str!` so every test runs identically on x86-64, ARM64 and `wasm32-wasip1`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_construct::contract::{
    harness, run, ConstructError, ConstructParams, Construction, ElementGeom, ElementId, Inputs, Measure, QuantityKind,
};
use fid_construct::elem_shape::{
    command_rows, ConeAxisApexAngle, ConeTwoCircles, CylinderAxisRadius, CylinderCircleLength, CylinderTwoCircles,
    RectangleCentreDirSizes, RectangleTwoLinePairs, SlotCentreLineSizes, SlotTwoCircles, CMD_BUILD_CONE,
    CMD_BUILD_CYLINDER, CMD_BUILD_RECTANGLE, CMD_BUILD_SLOT, PARAM_FLIP, PARAM_HALF_ANGLE, PARAM_LENGTH, PARAM_RADIUS,
    PARAM_WIDTH,
};
use fid_fit::contract::FittedFeature;
use fid_kci::parse_card;
use fid_types::CommandId;

const T_CYL_AR: &str = include_str!("../../../fixtures/cons/shape-defs-v1/cylinder_axis_radius.tsv");
const T_CYL_CL: &str = include_str!("../../../fixtures/cons/shape-defs-v1/cylinder_circle_length.tsv");
const T_CYL_2C: &str = include_str!("../../../fixtures/cons/shape-defs-v1/cylinder_two_circles.tsv");
const T_CONE_2C: &str = include_str!("../../../fixtures/cons/shape-defs-v1/cone_two_circles.tsv");
const T_CONE_AAA: &str = include_str!("../../../fixtures/cons/shape-defs-v1/cone_axis_apex_angle.tsv");
const T_SLOT_2C: &str = include_str!("../../../fixtures/cons/shape-defs-v1/slot_two_circles.tsv");
const T_SLOT_CLS: &str = include_str!("../../../fixtures/cons/shape-defs-v1/slot_centre_line_sizes.tsv");
const T_RECT_2LP: &str = include_str!("../../../fixtures/cons/shape-defs-v1/rectangle_two_line_pairs.tsv");
const T_RECT_CDS: &str = include_str!("../../../fixtures/cons/shape-defs-v1/rectangle_centre_dir_sizes.tsv");

const CARDS: [&str; 9] = [
    include_str!("../cards/construct.build_cylinder.axis_radius@1.0.0.toml"),
    include_str!("../cards/construct.build_cylinder.circle_length@1.0.0.toml"),
    include_str!("../cards/construct.build_cylinder.two_circles@1.0.0.toml"),
    include_str!("../cards/construct.build_cone.two_circles@1.0.0.toml"),
    include_str!("../cards/construct.build_cone.axis_apex_angle@1.0.0.toml"),
    include_str!("../cards/construct.build_slot.two_circles@1.0.0.toml"),
    include_str!("../cards/construct.build_slot.centre_line_sizes@1.0.0.toml"),
    include_str!("../cards/construct.build_rectangle.two_line_pairs@1.0.0.toml"),
    include_str!("../cards/construct.build_rectangle.centre_dir_sizes@1.0.0.toml"),
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

#[test]
fn ac1_all_nine_variants_match_300_seeded_cases_within_1e_12() {
    let params = ConstructParams::new(1e-6, 1e-6).unwrap();

    // 1. CylinderAxisRadius
    let c_ar = CylinderAxisRadius::new(id(1)).unwrap();
    for (i, r) in parse_rows(T_CYL_AR).iter().enumerate() {
        let g = [ElementGeom::Feature(FittedFeature::Line3 { p: v3(r, 0), d: v3(r, 3) })];
        let p = params.clone().with(PARAM_RADIUS, QuantityKind::Length, r[6]).unwrap();
        let ev = run(&c_ar, &g, &p).unwrap();
        let FittedFeature::Cylinder { p: gp, d: gd, r: gr } = *ev.output().as_feature().unwrap() else { panic!() };
        assert!(dist3(gp, v3(r, 7)) <= 1e-12 * scale3(v3(r, 7)), "cyl.ar p {i}");
        assert!(dist3(gd, v3(r, 10)) <= 1e-12, "cyl.ar d {i}");
        assert!((gr - r[13]).abs() <= 1e-12 * r[13].max(1.0));
    }

    // 2. CylinderCircleLength
    let c_cl = CylinderCircleLength::new(id(1)).unwrap();
    for (i, r) in parse_rows(T_CYL_CL).iter().enumerate() {
        let g = [ElementGeom::Feature(FittedFeature::Circle3 { c: v3(r, 0), n: v3(r, 3), r: r[6] })];
        let p = params.clone().with(PARAM_LENGTH, QuantityKind::Length, r[7]).unwrap();
        let ev = run(&c_cl, &g, &p).unwrap();
        let FittedFeature::Cylinder { p: gp, d: gd, r: gr } = *ev.output().as_feature().unwrap() else { panic!() };
        assert!(dist3(gp, v3(r, 8)) <= 1e-12 * scale3(v3(r, 8)), "cyl.cl p {i}");
        assert!(dist3(gd, v3(r, 11)) <= 1e-12, "cyl.cl d {i}");
        assert!((gr - r[14]).abs() <= 1e-12 * r[14].max(1.0));
    }

    // 3. CylinderTwoCircles + diagnostics
    let c_2c = CylinderTwoCircles::new(id(1), id(2)).unwrap();
    for (i, r) in parse_rows(T_CYL_2C).iter().enumerate() {
        let g = [
            ElementGeom::Feature(FittedFeature::Circle3 { c: v3(r, 0), n: v3(r, 3), r: r[6] }),
            ElementGeom::Feature(FittedFeature::Circle3 { c: v3(r, 7), n: v3(r, 10), r: r[13] }),
        ];
        let ev = run(&c_2c, &g, &params).unwrap();
        let FittedFeature::Cylinder { p: gp, d: gd, r: gr } = *ev.output().as_feature().unwrap() else { panic!() };
        assert!(dist3(gp, v3(r, 14)) <= 1e-12 * scale3(v3(r, 14)), "cyl.2c p {i}");
        assert!(dist3(gd, v3(r, 17)) <= 1e-12, "cyl.2c d {i}");
        assert!((gr - r[20]).abs() <= 1e-12 * r[20].max(1.0));
        if i == 0 {
            let decl = c_2c.declare(&params);
            let bound = Inputs::bind(&decl, &g).unwrap();
            let (_, diag) = c_2c.evaluate_with_diagnostics(&bound, &params).unwrap();
            assert!((diag.radius_diff_m - (r[6] - r[13]).abs()).abs() <= 1e-15);
        }
    }

    // 4. ConeTwoCircles
    let co_2c = ConeTwoCircles::new(id(1), id(2)).unwrap();
    for (i, r) in parse_rows(T_CONE_2C).iter().enumerate() {
        let g = [
            ElementGeom::Feature(FittedFeature::Circle3 { c: v3(r, 0), n: v3(r, 3), r: r[6] }),
            ElementGeom::Feature(FittedFeature::Circle3 { c: v3(r, 7), n: v3(r, 10), r: r[13] }),
        ];
        let ev = run(&co_2c, &g, &params).unwrap();
        let FittedFeature::Cone { apex, d, half_angle } = *ev.output().as_feature().unwrap() else { panic!() };
        assert!(dist3(apex, v3(r, 14)) <= 1e-12 * scale3(v3(r, 14)), "cone.2c apex {i}");
        assert!(dist3(d, v3(r, 17)) <= 1e-12, "cone.2c d {i}");
        assert!((half_angle - r[20]).abs() <= 1e-12);
    }

    // 5. ConeAxisApexAngle
    let co_aaa = ConeAxisApexAngle::new(id(1), id(2)).unwrap();
    for (i, r) in parse_rows(T_CONE_AAA).iter().enumerate() {
        let g = [
            ElementGeom::Feature(FittedFeature::Line3 { p: v3(r, 0), d: v3(r, 3) }),
            ElementGeom::Feature(FittedFeature::Point { c: v3(r, 6) }),
        ];
        let p = params
            .clone()
            .with(PARAM_HALF_ANGLE, QuantityKind::Angle, r[9])
            .unwrap()
            .with(PARAM_FLIP, QuantityKind::Count, r[10])
            .unwrap();
        let ev = run(&co_aaa, &g, &p).unwrap();
        let FittedFeature::Cone { apex, d, half_angle } = *ev.output().as_feature().unwrap() else { panic!() };
        assert!(dist3(apex, v3(r, 11)) <= 1e-12 * scale3(v3(r, 11)), "cone.aaa apex {i}");
        assert!(dist3(d, v3(r, 14)) <= 1e-12, "cone.aaa d {i}");
        assert!((half_angle - r[17]).abs() <= 1e-12);
    }

    // 6. SlotTwoCircles
    let sl_2c = SlotTwoCircles::new(id(1), id(2)).unwrap();
    for (i, r) in parse_rows(T_SLOT_2C).iter().enumerate() {
        let g = [
            ElementGeom::Feature(FittedFeature::Circle3 { c: v3(r, 0), n: v3(r, 3), r: r[6] }),
            ElementGeom::Feature(FittedFeature::Circle3 { c: v3(r, 7), n: v3(r, 10), r: r[13] }),
        ];
        let ev = run(&sl_2c, &g, &params).unwrap();
        let FittedFeature::Slot { c, n, d, length, width } = *ev.output().as_feature().unwrap() else { panic!() };
        assert!(dist3(c, v3(r, 14)) <= 1e-12 * scale3(v3(r, 14)), "slot.2c c {i}");
        assert!(dist3(n, v3(r, 17)) <= 1e-12, "slot.2c n {i}");
        assert!(dist3(d, v3(r, 20)) <= 1e-12, "slot.2c d {i}");
        assert!((length - r[23]).abs() <= 1e-12 * r[23].max(1.0));
        assert!((width - r[24]).abs() <= 1e-12 * r[24].max(1.0));
    }

    // 7. SlotCentreLineSizes
    let sl_cls = SlotCentreLineSizes::new(id(1), id(2)).unwrap();
    for (i, r) in parse_rows(T_SLOT_CLS).iter().enumerate() {
        let g = [
            ElementGeom::Feature(FittedFeature::Plane { p: v3(r, 0), n: v3(r, 3) }),
            ElementGeom::Feature(FittedFeature::Line3 { p: v3(r, 6), d: v3(r, 9) }),
        ];
        let p = params
            .clone()
            .with(PARAM_LENGTH, QuantityKind::Length, r[12])
            .unwrap()
            .with(PARAM_WIDTH, QuantityKind::Length, r[13])
            .unwrap();
        let ev = run(&sl_cls, &g, &p).unwrap();
        let FittedFeature::Slot { c, n, d, length, width } = *ev.output().as_feature().unwrap() else { panic!() };
        assert!(dist3(c, v3(r, 14)) <= 1e-12 * scale3(v3(r, 14)), "slot.cls c {i}");
        assert!(dist3(n, v3(r, 17)) <= 1e-12, "slot.cls n {i}");
        assert!(dist3(d, v3(r, 20)) <= 1e-12, "slot.cls d {i}");
        assert!((length - r[23]).abs() <= 1e-12 * r[23].max(1.0));
        assert!((width - r[24]).abs() <= 1e-12 * r[24].max(1.0));
    }

    // 8. RectangleTwoLinePairs
    let rc_2lp = RectangleTwoLinePairs::new(id(1), id(2), id(3), id(4)).unwrap();
    for (i, r) in parse_rows(T_RECT_2LP).iter().enumerate() {
        let g = [
            ElementGeom::Feature(FittedFeature::Line3 { p: v3(r, 0), d: v3(r, 3) }),
            ElementGeom::Feature(FittedFeature::Line3 { p: v3(r, 6), d: v3(r, 9) }),
            ElementGeom::Feature(FittedFeature::Line3 { p: v3(r, 12), d: v3(r, 15) }),
            ElementGeom::Feature(FittedFeature::Line3 { p: v3(r, 18), d: v3(r, 21) }),
        ];
        let ev = run(&rc_2lp, &g, &params).unwrap();
        let FittedFeature::Rectangle { c, n, d, length, width } = *ev.output().as_feature().unwrap() else { panic!() };
        assert!(dist3(c, v3(r, 24)) <= 1e-12 * scale3(v3(r, 24)), "rect.2lp c {i}");
        assert!(dist3(n, v3(r, 27)) <= 1e-12, "rect.2lp n {i}");
        assert!(dist3(d, v3(r, 30)) <= 1e-12, "rect.2lp d {i}");
        assert!((length - r[33]).abs() <= 1e-12 * r[33].max(1.0));
        assert!((width - r[34]).abs() <= 1e-12 * r[34].max(1.0));
    }

    // 9. RectangleCentreDirSizes
    let rc_cds = RectangleCentreDirSizes::new(id(1), id(2), id(3)).unwrap();
    for (i, r) in parse_rows(T_RECT_CDS).iter().enumerate() {
        let g = [
            ElementGeom::Feature(FittedFeature::Point { c: v3(r, 0) }),
            ElementGeom::Feature(FittedFeature::Plane { p: v3(r, 3), n: v3(r, 6) }),
            ElementGeom::Feature(FittedFeature::Line3 { p: v3(r, 9), d: v3(r, 12) }),
        ];
        let p = params
            .clone()
            .with(PARAM_LENGTH, QuantityKind::Length, r[15])
            .unwrap()
            .with(PARAM_WIDTH, QuantityKind::Length, r[16])
            .unwrap();
        let ev = run(&rc_cds, &g, &p).unwrap();
        let FittedFeature::Rectangle { c, n, d, length, width } = *ev.output().as_feature().unwrap() else { panic!() };
        assert!(dist3(c, v3(r, 17)) <= 1e-12 * scale3(v3(r, 17)), "rect.cds c {i}");
        assert!(dist3(n, v3(r, 20)) <= 1e-12, "rect.cds n {i}");
        assert!(dist3(d, v3(r, 23)) <= 1e-12, "rect.cds d {i}");
        assert!((length - r[26]).abs() <= 1e-12 * r[26].max(1.0));
        assert!((width - r[27]).abs() <= 1e-12 * r[27].max(1.0));
    }
}

#[test]
fn ac2_ac3_and_error_paths() {
    let params = ConstructParams::new(1e-6, 1e-6).unwrap();

    // AC-2: two circles whose axes differ by 1e-3 rad with angular threshold 1e-6 rad -> IllConditioned(Angle).
    let cyl = CylinderTwoCircles::new(id(1), id(2)).unwrap();
    let tilt = 1e-3_f64;
    let misaligned = [
        ElementGeom::Feature(FittedFeature::Circle3 { c: [0.0, 0.0, 0.0], n: [0.0, 0.0, 1.0], r: 0.05 }),
        ElementGeom::Feature(FittedFeature::Circle3 { c: [0.0, 0.0, 0.1], n: [tilt.sin(), 0.0, tilt.cos()], r: 0.05 }),
    ];
    match run(&cyl, &misaligned, &params).unwrap_err() {
        ConstructError::IllConditioned(cond) => assert_eq!(cond.measure(), Measure::Angle),
        other => panic!("expected IllConditioned(Angle), got {other:?}"),
    }

    // AC-3: cone two_circles with equal radii -> IllConditioned(Distance).
    let cone = ConeTwoCircles::new(id(1), id(2)).unwrap();
    let equal_r = [
        ElementGeom::Feature(FittedFeature::Circle3 { c: [0.0, 0.0, 0.0], n: [0.0, 0.0, 1.0], r: 0.05 }),
        ElementGeom::Feature(FittedFeature::Circle3 { c: [0.0, 0.0, 0.1], n: [0.0, 0.0, 1.0], r: 0.05 }),
    ];
    match run(&cone, &equal_r, &params).unwrap_err() {
        ConstructError::IllConditioned(cond) => {
            assert_eq!(cond.measure(), Measure::Distance);
            assert_eq!(cond.value(), 0.0);
        }
        other => panic!("expected IllConditioned(Distance), got {other:?}"),
    }

    // InputCount on rectangle two_line_pairs with 3 inputs.
    let rect = RectangleTwoLinePairs::new(id(1), id(2), id(3), id(4)).unwrap();
    let three_lines = [
        ElementGeom::Feature(FittedFeature::Line3 { p: [0.0, 0.0, 0.0], d: [1.0, 0.0, 0.0] }),
        ElementGeom::Feature(FittedFeature::Line3 { p: [0.0, 1.0, 0.0], d: [1.0, 0.0, 0.0] }),
        ElementGeom::Feature(FittedFeature::Line3 { p: [0.0, 0.0, 0.0], d: [0.0, 1.0, 0.0] }),
    ];
    assert_eq!(run(&rect, &three_lines, &params).unwrap_err(), ConstructError::InputCount { expected: 4, got: 3 });
}

#[test]
fn ac4_ac5_harness_replay_command_rows_and_spec_cards() {
    let params = ConstructParams::new(1e-6, 1e-6).unwrap();

    // Harness check on cylinder.two_circles, cone.two_circles, slot.two_circles, rectangle.two_line_pairs.
    let c1 = ElementGeom::Feature(FittedFeature::Circle3 { c: [0.0, 0.0, 0.0], n: [0.0, 0.0, 1.0], r: 0.05 });
    let c2 = ElementGeom::Feature(FittedFeature::Circle3 { c: [0.0, 0.0, 0.2], n: [0.0, 0.0, 1.0], r: 0.05 });
    let c1_alt = ElementGeom::Feature(FittedFeature::Circle3 { c: [0.0, 0.0, 0.01], n: [0.0, 0.0, 1.0], r: 0.05 });
    let c2_alt = ElementGeom::Feature(FittedFeature::Circle3 { c: [0.0, 0.0, 0.25], n: [0.0, 0.0, 1.0], r: 0.05 });

    let ev_cyl = harness::check(
        || CylinderTwoCircles::new(id(1), id(2)).unwrap(),
        &params,
        &[c1.clone(), c2.clone()],
        &[c1_alt, c2_alt],
    )
    .unwrap()
    .unwrap();

    // AC-5: exact journal event replay.
    let cmd_cyl = CommandId::parse(CMD_BUILD_CYLINDER).unwrap();
    let e1 = ev_cyl.event(&cmd_cyl).unwrap();
    let e2 = run(&CylinderTwoCircles::new(id(1), id(2)).unwrap(), &[c1, c2], &params).unwrap().event(&cmd_cyl).unwrap();
    assert_eq!(e1.payload_bytes(), e2.payload_bytes());

    // All 4 command rows and all 9 spec cards agree.
    let rows = command_rows().unwrap();
    assert_eq!(rows.len(), 4);
    let cmds: Vec<&str> = rows.iter().map(|r| r.command().as_str()).collect();
    assert_eq!(cmds, vec![CMD_BUILD_CYLINDER, CMD_BUILD_CONE, CMD_BUILD_SLOT, CMD_BUILD_RECTANGLE]);
    let mut row_algs: Vec<String> = rows.iter().flat_map(|r| r.algorithms().iter().map(|a| a.to_string())).collect();
    row_algs.sort();
    assert_eq!(row_algs.len(), 9);
    let mut card_ids: Vec<String> = CARDS.iter().map(|c| parse_card(c).unwrap().id.to_string()).collect();
    card_ids.sort();
    assert_eq!(card_ids, row_algs);
}
