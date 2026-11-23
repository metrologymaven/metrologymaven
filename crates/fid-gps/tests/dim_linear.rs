//! Kernel unit and fixture tests for `GDT.DIM.LINEAR` (`fid-gps::dim::linear`).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::CValue;
use fid_gps::dim::linear::{
    evaluate_caliper, evaluate_distance, DistanceFlag, DistanceMeasurand, LengthUnit, LinearElement, LinearError,
    NON_PARALLEL_THRESHOLD_RAD,
};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

const TOL: f64 = 1e-12;

fn fix(sub: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/gdt/dim").join(sub)
}

fn obj(v: &CValue) -> &BTreeMap<String, CValue> {
    match v {
        CValue::Obj(o) => o,
        _ => panic!("expected object"),
    }
}

fn arr(v: &CValue) -> &[CValue] {
    match v {
        CValue::Arr(a) => a,
        _ => panic!("expected array"),
    }
}

fn st(v: &CValue) -> &str {
    match v {
        CValue::Str(s) => s,
        _ => panic!("expected string"),
    }
}

fn num(v: &CValue) -> f64 {
    match v {
        CValue::Num(n) => *n,
        _ => panic!("expected number"),
    }
}

fn vec3(v: &CValue) -> [f64; 3] {
    let a = arr(v);
    [num(&a[0]), num(&a[1]), num(&a[2])]
}

fn parse_element(v: &CValue) -> LinearElement {
    let o = obj(v);
    match st(&o["kind"]) {
        "point" => LinearElement::Point {
            position: vec3(&o["position"]),
            raw_point: matches!(o.get("raw_point"), Some(CValue::Bool(true))),
        },
        "line" => LinearElement::Line { point: vec3(&o["point"]), direction: vec3(&o["direction"]) },
        "plane" => LinearElement::Plane { centroid: vec3(&o["centroid"]), normal: vec3(&o["normal"]) },
        other => panic!("unexpected element kind: {other}"),
    }
}

#[test]
fn ac1_analytic_fixtures_match_expected_within_1e12_m() {
    let cases_doc = fid_canon::parse(&std::fs::read(fix("linear_analytic.json")).unwrap()).unwrap();
    let exp_doc = fid_canon::parse(&std::fs::read(fix("expected/linear_analytic.json")).unwrap()).unwrap();
    let expected = obj(&obj(&exp_doc)["cases"]);

    for case in arr(&obj(&cases_doc)["cases"]) {
        let o = obj(case);
        let name = st(&o["name"]);
        let a = parse_element(&o["a"]);
        let b = parse_element(&o["b"]);
        let unit = LengthUnit::parse(st(&o["unit"])).unwrap();
        let res = evaluate_distance(a, b, DistanceMeasurand::ThreeD, unit).unwrap();
        let e = obj(&expected[name]);
        let exp_si = num(&e["value_si"]);
        let exp_mm = num(&e["value_mm"]);
        let exp_basis = st(&e["basis"]);

        assert!((res.value_si - exp_si).abs() <= TOL, "{name}: si {} vs {exp_si}", res.value_si);
        assert!((res.value_authored - exp_mm).abs() <= 1e-9, "{name}: mm {} vs {exp_mm}", res.value_authored);
        assert_eq!(res.reported.unit.symbol(), "mm");
        let rep_parsed: f64 = res.reported.value.parse().unwrap();
        assert!((rep_parsed - exp_mm).abs() <= 1e-6, "{name}: reported {} vs {exp_mm}", res.reported.value);
        assert_eq!(if res.from_raw_points { "POINTS" } else { "FEATURES" }, exp_basis, "{name}");
        assert!(res.flags.is_empty(), "{name} should have no flags");
    }
}

#[test]
fn ac2_component_and_projected_measurands() {
    let p1 = LinearElement::Plane { centroid: [0.0, 0.0, 0.0], normal: [1.0, 0.0, 0.0] };
    let p2 = LinearElement::Plane { centroid: [0.01, 0.0, 0.0], normal: [1.0, 0.0, 0.0] };
    let comp_x =
        evaluate_distance(p1, p2, DistanceMeasurand::Component { axis_unit: [1.0, 0.0, 0.0] }, LengthUnit::Millimetre)
            .unwrap();
    assert!((comp_x.value_si - 0.01).abs() <= TOL);
    assert_eq!(comp_x.reported.value, "10.000000");

    let comp_neg_x =
        evaluate_distance(p1, p2, DistanceMeasurand::Component { axis_unit: [-1.0, 0.0, 0.0] }, LengthUnit::Millimetre)
            .unwrap();
    assert!((comp_neg_x.value_si + 0.01).abs() <= TOL);
    assert_eq!(comp_neg_x.reported.value, "-10.000000");

    // Projected onto XY plane drops the Z component (3-4-5 -> 3-4 -> 5 mm).
    let a = LinearElement::Point { position: [0.0, 0.0, 0.0], raw_point: false };
    let b = LinearElement::Point { position: [0.003, 0.004, 0.099], raw_point: false };
    let proj =
        evaluate_distance(a, b, DistanceMeasurand::Projected { plane_normal: [0.0, 0.0, 1.0] }, LengthUnit::Millimetre)
            .unwrap();
    assert!((proj.value_si - 0.005).abs() <= TOL);
    assert_eq!(proj.reported.value, "5.000000");
}

#[test]
fn ac4_caliper_box_and_lowest_index_ties() {
    let doc = fid_canon::parse(&std::fs::read(fix("caliper_box.json")).unwrap()).unwrap();
    let o = obj(&doc);
    let dir = vec3(&o["direction"]);
    let pts: Vec<[f64; 3]> = arr(&o["points"]).iter().map(vec3).collect();
    let res = evaluate_caliper(&pts, dir, LengthUnit::Millimetre).unwrap();
    assert!((res.value_si - num(&o["expected_value_si"])).abs() <= TOL);
    assert!((res.value_authored - num(&o["expected_value_mm"])).abs() <= 1e-9);
    assert_eq!(res.reported.value, "20.000000");
    assert_eq!(res.min_index, num(&o["expected_min_index"]) as u64);
    assert_eq!(res.max_index, num(&o["expected_max_index"]) as u64);
}

#[test]
fn ac5_non_parallel_planes_flag_and_parallel_lines() {
    let alpha = 0.01_f64;
    let p1 = LinearElement::Plane { centroid: [0.0, 0.0, 0.0], normal: [0.0, 0.0, 1.0] };
    let p2 = LinearElement::Plane { centroid: [0.0, 0.0, 0.01], normal: [alpha.sin(), 0.0, alpha.cos()] };
    let res = evaluate_distance(p1, p2, DistanceMeasurand::ThreeD, LengthUnit::Millimetre).unwrap();
    assert_eq!(res.flags.len(), 1);
    match res.flags[0] {
        DistanceFlag::NonParallelPlanes { angle_rad } => {
            assert!((angle_rad - alpha).abs() <= 1e-12, "angle={angle_rad}");
            assert!(angle_rad > NON_PARALLEL_THRESHOLD_RAD);
        }
    }

    // Parallel lines fallback (2 lines along X separated by 8 mm in Y).
    let l1 = LinearElement::Line { point: [0.0, 0.0, 0.0], direction: [1.0, 0.0, 0.0] };
    let l2 = LinearElement::Line { point: [1.23, 0.008, 0.0], direction: [1.0, 0.0, 0.0] };
    let res_par = evaluate_distance(l1, l2, DistanceMeasurand::ThreeD, LengthUnit::Millimetre).unwrap();
    assert!((res_par.value_si - 0.008).abs() <= TOL);
    assert_eq!(res_par.reported.value, "8.000000");
}

#[test]
fn error_paths_and_units() {
    let l = LinearElement::Line { point: [0.0, 0.0, 0.0], direction: [1.0, 0.0, 0.0] };
    let p = LinearElement::Plane { centroid: [0.0, 0.0, 0.0], normal: [0.0, 0.0, 1.0] };
    assert_eq!(
        evaluate_distance(l, p, DistanceMeasurand::ThreeD, LengthUnit::Metre).unwrap_err().code(),
        "unsupported_pair"
    );
    let deg = LinearElement::Line { point: [0.0, 0.0, 0.0], direction: [0.0, 0.0, 0.0] };
    assert_eq!(
        evaluate_distance(deg, l, DistanceMeasurand::ThreeD, LengthUnit::Metre).unwrap_err().code(),
        "degenerate_element"
    );
    assert_eq!(evaluate_caliper(&[], [1.0, 0.0, 0.0], LengthUnit::Millimetre), Err(LinearError::EmptyPointSet));

    // Exact inch conversion: 0.0254 m == 1.000000 in.
    let a = LinearElement::Point { position: [0.0, 0.0, 0.0], raw_point: true };
    let b = LinearElement::Point { position: [0.0254, 0.0, 0.0], raw_point: true };
    let inch = evaluate_distance(a, b, DistanceMeasurand::ThreeD, LengthUnit::Inch).unwrap();
    assert_eq!(inch.reported.value, "1.000000");
    assert_eq!(inch.reported.exact_conversion.factor, "1/0.0254");
}
