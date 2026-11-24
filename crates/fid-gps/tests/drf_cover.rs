//! Acceptance criteria of spec node GDT.DRF.COMMON.COVER (datum target coverage): AC-1 to AC-4
//! and REQ-001 to REQ-003. Expected distances and outcomes come from the independent Python
//! reference `fixtures/gdt/drf/make_targets_plate.py` (`targets_plate.json`: exact rational
//! nearest distances of the pinned samples, and the analytic largest gap); the input errors
//! are hand-labelled from the leaf's rules. Synthetic data only.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::CValue;
use fid_geo_types::PointBuffer;
use fid_gps::drf::targets::coverage::{check_coverage, CoverError, Target, TargetGeometry, MAX_SAMPLES};
use std::collections::BTreeMap;
use std::path::Path;

/// AC-1: distances within 1e-15 m of the reference.
const TOL: f64 = 1e-15;

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
    assert_eq!(a.len(), 3);
    [num(&a[0]), num(&a[1]), num(&a[2])]
}

fn count(v: &CValue) -> u64 {
    match v {
        CValue::Num(n) if n.fract() == 0.0 && *n >= 0.0 => *n as u64,
        CValue::U64(n) => *n,
        other => panic!("integer, got {other:?}"),
    }
}

struct Expect {
    sampled: f64,
    analytic: Option<f64>,
    samples: u64,
}

struct Case {
    name: String,
    threshold: f64,
    spacing: f64,
    targets: Vec<Target>,
    per_target: Vec<Expect>,
    expect: BTreeMap<String, CValue>,
}

fn doc() -> CValue {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/gdt/drf/targets_plate.json");
    fid_canon::parse(&std::fs::read(path).unwrap()).unwrap()
}

fn plate() -> PointBuffer {
    let d = doc();
    let pts: Vec<[f64; 3]> = arr(&obj(&d)["points"]).iter().map(v3).collect();
    assert_eq!(pts.len(), 121);
    PointBuffer::from_points(&pts)
}

fn target(t: &BTreeMap<String, CValue>) -> Target {
    let geometry = match st(&t["kind"]) {
        "point" => TargetGeometry::Point { at: v3(&t["at"]) },
        "line" => TargetGeometry::Line { start: v3(&t["start"]), end: v3(&t["end"]) },
        "rect" => TargetGeometry::Rect { corner: v3(&t["corner"]), edge_u: v3(&t["edge_u"]), edge_v: v3(&t["edge_v"]) },
        "disc" => {
            TargetGeometry::Disc { centre: v3(&t["centre"]), normal: v3(&t["normal"]), radius: num(&t["radius"]) }
        }
        k => panic!("kind {k}"),
    };
    Target { label: st(&t["label"]).to_owned(), geometry }
}

fn cases() -> Vec<Case> {
    let d = doc();
    arr(&obj(&d)["cases"])
        .iter()
        .map(|c| {
            let c = obj(c);
            let ts: Vec<&BTreeMap<String, CValue>> = arr(&c["targets"]).iter().map(obj).collect();
            Case {
                name: st(&c["name"]).to_owned(),
                threshold: num(&c["threshold"]),
                spacing: num(&c["spacing"]),
                targets: ts.iter().map(|t| target(t)).collect(),
                per_target: ts
                    .iter()
                    .map(|t| {
                        let e = obj(&t["expect"]);
                        Expect {
                            sampled: num(&e["sampled"]),
                            analytic: e.get("analytic").map(num),
                            samples: count(&e["samples"]),
                        }
                    })
                    .collect(),
                expect: obj(&c["expect"]).clone(),
            }
        })
        .collect()
}

fn case(name: &str) -> Case {
    cases().into_iter().find(|c| c.name == name).unwrap()
}

fn run(c: &Case) -> Result<fid_gps::drf::targets::coverage::CoverageReport, CoverError> {
    check_coverage(&c.targets, &plate(), Some(c.threshold), c.spacing)
}

/// Every case: the report (with a threshold large enough to pass) matches the reference per
/// target, and the outcome at the case's own threshold matches the reference.
#[test]
fn every_reference_case() {
    let all = cases();
    assert!(all.len() >= 14);
    for c in &all {
        let report = check_coverage(&c.targets, &plate(), Some(1.0), c.spacing).unwrap();
        assert_eq!(report.per_target.len(), c.targets.len(), "{}", c.name);
        for ((got, t), e) in report.per_target.iter().zip(&c.targets).zip(&c.per_target) {
            assert_eq!(got.label, t.label, "{}", c.name);
            assert_eq!(got.samples, e.samples, "{}: {} sample count", c.name, t.label);
            assert!(
                (got.distance - e.sampled).abs() <= TOL,
                "{}: {} distance {:e}, reference {:e}",
                c.name,
                t.label,
                got.distance,
                e.sampled
            );
            if let Some(a) = e.analytic {
                assert!(got.distance <= a + TOL, "{}: {} sampled above the analytic supremum", c.name, t.label);
            }
        }
        match run(c) {
            Ok(r) => {
                assert!(c.expect.contains_key("ok"), "{}: passed, reference {:?}", c.name, c.expect);
                assert_eq!(r, report, "{}", c.name);
            }
            Err(CoverError::TargetNotCovered { target, distance }) => {
                assert_eq!(st(&c.expect["error"]), "target_not_covered", "{}", c.name);
                assert_eq!(target, st(&c.expect["target"]), "{}", c.name);
                assert!((distance - num(&c.expect["distance"])).abs() <= TOL, "{}", c.name);
            }
            Err(e) => panic!("{}: {e}", c.name),
        }
    }
}

/// AC-1: three point targets on the plate pass at 0.8 mm, each distance the analytic nearest
/// distance within 1e-15 m.
#[test]
fn ac1_point_targets_pass() {
    let c = case("ac1_plate");
    assert_eq!(c.threshold, 0.8e-3);
    let r = run(&c).unwrap();
    let labels: Vec<&str> = r.per_target.iter().map(|t| t.label.as_str()).collect();
    assert_eq!(labels, ["A1", "A2", "A3"]);
    for (got, e) in r.per_target.iter().zip(&c.per_target) {
        assert!((got.distance - e.analytic.unwrap()).abs() <= TOL);
        assert_eq!(got.samples, 1);
    }
}

/// AC-2: A2 moved 2 mm outside the grid is `target_not_covered{A2, 2 mm}`.
#[test]
fn ac2_target_outside() {
    let e = run(&case("ac2_a2_outside")).unwrap_err();
    match &e {
        CoverError::TargetNotCovered { target, distance } => {
            assert_eq!(target, "A2");
            assert!((distance - 2e-3).abs() <= TOL);
        }
        other => panic!("{other}"),
    }
    assert_eq!(e.code(), "target_not_covered");
    assert!(e.to_string().starts_with("target_not_covered{A2,0.00"), "{e}");
}

/// REQ-002: the first failing target in definition order is named, not the worst.
#[test]
fn first_failure_in_definition_order() {
    let c = case("first_in_definition_order");
    let worst = c.per_target.iter().map(|e| e.sampled).fold(0.0, |a: f64, b| if b > a { b } else { a });
    match run(&c).unwrap_err() {
        CoverError::TargetNotCovered { target, distance } => {
            assert_eq!(target, "A3");
            assert_eq!(distance, worst);
        }
        other => panic!("{other}"),
    }
    // Reversing the definition order names A2.
    let mut rev = c.targets.clone();
    rev.reverse();
    match check_coverage(&rev, &plate(), Some(c.threshold), c.spacing).unwrap_err() {
        CoverError::TargetNotCovered { target, .. } => assert_eq!(target, "A2"),
        other => panic!("{other}"),
    }
}

/// A distance equal to the threshold passes (exceeding is strict).
#[test]
fn equal_threshold_passes() {
    let c = case("equal_threshold_passes");
    let r = run(&c).unwrap();
    assert_eq!(r.per_target[0].distance, c.threshold);
    let just_below = f64::from_bits(c.threshold.to_bits() - 1);
    assert!(matches!(
        check_coverage(&c.targets, &plate(), Some(just_below), c.spacing),
        Err(CoverError::TargetNotCovered { .. })
    ));
}

/// AC-3: a line target half outside the data reports the analytic largest gap (5 mm).
#[test]
fn ac3_line_half_outside() {
    let c = case("ac3_line_half_outside_passes");
    let r = run(&c).unwrap();
    let a = c.per_target[0].analytic.unwrap();
    assert!((a - 5e-3).abs() <= TOL);
    assert!((r.per_target[0].distance - a).abs() <= TOL);
    match run(&case("ac3_line_half_outside")).unwrap_err() {
        CoverError::TargetNotCovered { target, distance } => {
            assert_eq!(target, "C1");
            assert!((distance - a).abs() <= TOL);
        }
        other => panic!("{other}"),
    }
}

/// AC-4: no threshold from the chain is `threshold_missing`, before any other check.
#[test]
fn ac4_threshold_missing() {
    let c = case("ac1_plate");
    assert_eq!(check_coverage(&c.targets, &plate(), None, c.spacing), Err(CoverError::ThresholdMissing));
    // Even with no points, no targets and a bad spacing.
    let e = check_coverage(&[], &PointBuffer::default(), None, f64::NAN).unwrap_err();
    assert_eq!(e, CoverError::ThresholdMissing);
    assert_eq!(e.code(), "threshold_missing");
}

fn pt(label: &str, at: [f64; 3]) -> Target {
    Target { label: label.to_owned(), geometry: TargetGeometry::Point { at } }
}

/// Input errors, hand-labelled from the leaf's rules and the pinned check order.
#[test]
fn input_errors() {
    let p = plate();
    let ok = [pt("A1", [0.001, 0.001, 0.0])];
    let nan = f64::NAN;
    // NaN and infinity anywhere.
    assert_eq!(check_coverage(&ok, &p, Some(nan), 1e-3), Err(CoverError::NanInput));
    assert_eq!(check_coverage(&ok, &p, Some(f64::INFINITY), 1e-3), Err(CoverError::NanInput));
    assert_eq!(check_coverage(&ok, &p, Some(1e-3), nan), Err(CoverError::NanInput));
    assert_eq!(check_coverage(&ok, &p, Some(1e-3), f64::INFINITY), Err(CoverError::NanInput));
    assert_eq!(check_coverage(&[pt("A1", [0.0, nan, 0.0])], &p, Some(1e-3), 1e-3), Err(CoverError::NanInput));
    let mut bad = p.clone();
    bad.z[7] = nan;
    assert_eq!(check_coverage(&ok, &bad, Some(1e-3), 1e-3), Err(CoverError::NanInput));
    let disc_nan = Target {
        label: "E1".into(),
        geometry: TargetGeometry::Disc { centre: [0.0; 3], normal: [0.0, 0.0, 1.0], radius: nan },
    };
    assert_eq!(check_coverage(&[disc_nan], &p, Some(1e-3), 1e-3), Err(CoverError::NanInput));
    // NaN wins over the later checks.
    assert_eq!(check_coverage(&[], &PointBuffer::default(), Some(-1.0), nan), Err(CoverError::NanInput));
    // Threshold and spacing.
    assert_eq!(check_coverage(&ok, &p, Some(-1e-9), 1e-3), Err(CoverError::BadThreshold));
    assert_eq!(check_coverage(&ok, &p, Some(1e-3), 0.0), Err(CoverError::BadSpacing));
    assert_eq!(check_coverage(&ok, &p, Some(1e-3), -1e-3), Err(CoverError::BadSpacing));
    // Zero threshold is allowed: a target on a measured point passes.
    assert_eq!(check_coverage(&ok, &p, Some(0.0), 1e-3).unwrap().per_target[0].distance, 0.0);
    // Empty data, no targets.
    assert_eq!(check_coverage(&ok, &PointBuffer::default(), Some(1e-3), 1e-3), Err(CoverError::EmptyPoints));
    assert_eq!(check_coverage(&[], &p, Some(1e-3), 1e-3), Err(CoverError::NoTargets));
    // Degenerate discs.
    for (normal, radius) in [([0.0, 0.0, 0.0], 1e-3), ([0.0, 0.0, 1.0], -1e-3)] {
        let t = Target { label: "E9".into(), geometry: TargetGeometry::Disc { centre: [0.0; 3], normal, radius } };
        let e = check_coverage(&[pt("A1", [0.0; 3]), t], &p, Some(1e-3), 1e-3).unwrap_err();
        assert_eq!(e, CoverError::DegenerateTarget { target: "E9".into() });
        assert_eq!(e.to_string(), "degenerate_target{E9}");
    }
    // Too many samples: a 1 m line at 1e-9 m spacing, and the boundary.
    let line = |len: f64| Target {
        label: "C9".into(),
        geometry: TargetGeometry::Line { start: [0.0; 3], end: [len, 0.0, 0.0] },
    };
    let e = check_coverage(&[line(1.0)], &p, Some(10.0), 1e-9).unwrap_err();
    assert_eq!(e, CoverError::TooManySamples { target: "C9".into() });
    let n = (MAX_SAMPLES - 1) as f64;
    let r = check_coverage(&[line(n)], &p, Some(1e9), 1.0).unwrap();
    assert_eq!(r.per_target[0].samples, MAX_SAMPLES);
    assert_eq!(
        check_coverage(&[line(n + 1.0)], &p, Some(1e9), 1.0),
        Err(CoverError::TooManySamples { target: "C9".into() })
    );
}

/// Degenerate but valid geometry: a zero-length line and a zero-radius disc are their point.
#[test]
fn zero_size_targets() {
    let p = plate();
    let at = [0.0023, 0.0046, 0.0002];
    let point = check_coverage(&[pt("A1", at)], &p, Some(1.0), 1e-3).unwrap().per_target[0].distance;
    let line = Target { label: "C1".into(), geometry: TargetGeometry::Line { start: at, end: at } };
    let disc = Target {
        label: "E1".into(),
        geometry: TargetGeometry::Disc { centre: at, normal: [1.0, 2.0, 3.0], radius: 0.0 },
    };
    let rect = Target {
        label: "D1".into(),
        geometry: TargetGeometry::Rect { corner: at, edge_u: [0.0; 3], edge_v: [0.0; 3] },
    };
    let r = check_coverage(&[line, disc, rect], &p, Some(1.0), 1e-3).unwrap();
    for t in &r.per_target {
        assert_eq!(t.distance, point, "{}", t.label);
    }
    assert_eq!(r.per_target.iter().map(|t| t.samples).collect::<Vec<_>>(), [2, 5, 4]);
}

/// The result does not depend on the order of the measured points (k-d tree is exact).
#[test]
fn point_order_independent() {
    let c = case("mixed_all_pass");
    let p = plate();
    let mut pts: Vec<[f64; 3]> = (0..p.len()).map(|i| p.point(i).unwrap()).collect();
    pts.reverse();
    let a = check_coverage(&c.targets, &p, Some(c.threshold), c.spacing).unwrap();
    let b = check_coverage(&c.targets, &PointBuffer::from_points(&pts), Some(c.threshold), c.spacing).unwrap();
    assert_eq!(a, b);
}
