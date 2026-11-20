//! Acceptance criteria of spec node KRN.LSQ.TRAIT (AC-1 .. AC-6) and the error paths.
//!
//! Expected values come from `fixtures/fit/make_trait_fixtures.py`: an exact rational / 80-digit
//! oracle for the degeneracy checks (AC-1), a bit-exact Python model of the pinned `red` and
//! centring rules (AC-2, AC-3) and hand-built canonical bytes and data rows (AC-3, AC-6). CI
//! runs these tests on x86-64 Linux and Windows, ARM64 macOS and wasm32 under wasmtime against
//! the same expected bytes (AC-3).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_fit::contract::{
    algorithm_id, check, check_points, command_row, encode, normalize_direction, normalize_direction2, unit_direction,
    AlgorithmId, Centred, Certificate, CommandRow, Constraint, Field, Fit, FitDiagnostics, FitError, FitParams,
    FitResult, FittedFeature, Method, Normalisation, NotConvergedReason, PointsView, Primitive, ResidualStats,
    RowError, StopReason, Thresholds, ValueType, Weights, DEFAULT_MIN_COVERAGE,
};
use fid_num::NumError;
use fid_solv::nlls::{Diagnostics, NllsConfig, SolverError};
use fid_types::{Determinism, SideEffect};

const DEGENERATE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/fit/trait-degenerate-v1/cases.txt");
const FAR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/fit/trait-far-origin-v1/points.txt");
const ENCODE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/fit/trait-encode-v1/result.txt");
const ROW: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/fit/trait-rows-v1/fit.ls_plane.json");

fn hex(s: &str) -> f64 {
    f64::from_bits(u64::from_str_radix(s, 16).unwrap())
}

fn hexes(s: &str) -> Vec<f64> {
    s.split_whitespace().map(hex).collect()
}

fn read(path: &str) -> String {
    std::fs::read_to_string(path).unwrap().replace("\r\n", "\n")
}

fn lines(text: &str) -> impl Iterator<Item = (&str, &str)> {
    text.lines().filter(|l| !l.starts_with('#') && !l.trim().is_empty()).map(|l| l.split_once(' ').unwrap_or((l, "")))
}

fn rel_close(a: f64, b: f64, tol: f64) -> bool {
    (a - b).abs() <= tol * b.abs()
}

struct Case {
    name: String,
    primitive: Primitive,
    result: String,
    xyz: Vec<f64>,
    vals: Vec<(String, String)>,
}

impl Case {
    fn get(&self, k: &str) -> Option<&str> {
        self.vals.iter().find(|(key, _)| key == k).map(|(_, v)| v.as_str())
    }
    fn usize(&self, k: &str) -> usize {
        self.get(k).unwrap().parse().unwrap()
    }
    fn f(&self, k: &str) -> f64 {
        self.get(k).unwrap().parse().unwrap()
    }
    fn check_cond(&self, cond: f64) {
        match self.get("cond").unwrap() {
            "inf" => assert!(cond > 1.0 / Thresholds::default().rank_tol, "{}: cond {cond:e}", self.name),
            "1" => assert_eq!(cond.to_bits(), 1.0f64.to_bits(), "{}", self.name),
            _ => assert!(rel_close(cond, self.f("cond"), 1e-9), "{}: cond {cond:e} vs {}", self.name, self.f("cond")),
        }
    }
}

fn cases() -> Vec<Case> {
    let mut out: Vec<Case> = Vec::new();
    for (k, v) in lines(&read(DEGENERATE)) {
        match k {
            "case" => {
                let t: Vec<&str> = v.split_whitespace().collect();
                assert_eq!((t[1], t[3]), ("primitive", "result"));
                out.push(Case {
                    name: t[0].to_string(),
                    primitive: Primitive::parse(t[2]).unwrap(),
                    result: t[4].to_string(),
                    xyz: Vec::new(),
                    vals: Vec::new(),
                });
            }
            "xyz" => out.last_mut().unwrap().xyz = hexes(v),
            _ => out.last_mut().unwrap().vals.push((k.to_string(), v.to_string())),
        }
    }
    out
}

#[test]
fn ac1_degeneracy_checks_with_default_thresholds() {
    let cases = cases();
    assert_eq!(cases.len(), 12);
    assert_eq!(cases.iter().filter(|c| c.result == "ok").count(), 8);
    let t = Thresholds::default();
    for c in &cases {
        let view = PointsView::new(&c.xyz, None).unwrap();
        let n = c.usize("n");
        assert_eq!(view.len(), n, "{}", c.name);
        let got = check_points(c.primitive, &view, &t);
        match (c.result.as_str(), got) {
            ("too_few_points", Err(FitError::TooFewPoints { n: gn, need })) => {
                assert_eq!((gn, need), (n, c.usize("need")), "{}", c.name);
            }
            ("collinear", Err(FitError::Collinear { n: gn, rank, cond }))
            | ("coplanar", Err(FitError::Coplanar { n: gn, rank, cond })) => {
                assert_eq!((gn, rank), (n, c.usize("rank")), "{}", c.name);
                c.check_cond(cond);
            }
            ("insufficient_coverage", Err(FitError::InsufficientCoverage { n: gn, coverage, need })) => {
                assert_eq!(gn, n, "{}", c.name);
                assert!((coverage - c.f("coverage")).abs() <= 1e-9, "{}: {coverage}", c.name);
                assert_eq!(need.to_bits(), DEFAULT_MIN_COVERAGE.to_bits());
            }
            ("ok", Ok((centred, d))) => {
                assert_eq!((d.n, d.rank), (n, c.usize("rank")), "{}", c.name);
                c.check_cond(d.cond);
                match c.get("coverage") {
                    Some(_) => assert!((d.coverage.unwrap() - c.f("coverage")).abs() <= 1e-9, "{}", c.name),
                    None => assert_eq!(d.coverage, None, "{}", c.name),
                }
                // The shape is the same when checked again on the centred data (deterministic).
                assert_eq!(check(c.primitive, &centred, &t).unwrap(), d, "{}", c.name);
                let s = d.shape.singular_values;
                assert!(s[0] >= s[1] && s[1] >= s[2] && s[2] >= 0.0, "{}", c.name);
                for a in d.shape.axes {
                    assert_eq!(normalize_direction(a).unwrap(), a, "{}: axes are canonical", c.name);
                }
            }
            (want, got) => panic!("{}: expected {want}, got {got:?}", c.name),
        }
    }
}

#[test]
fn ac1_thresholds_are_taken_from_params() {
    let c = cases().into_iter().find(|c| c.name == "arc10_circle3").unwrap();
    let view = PointsView::new(&c.xyz, None).unwrap();
    // With a 5-degree threshold the 10-degree arc passes.
    let t = Thresholds { min_coverage: 5.0 * core::f64::consts::PI / 180.0, ..Thresholds::default() };
    let (_, d) = check_points(Primitive::Circle3, &view, &t).unwrap();
    assert!((d.coverage.unwrap() - c.f("coverage")).abs() <= 1e-9);
    // A rank tolerance above the arc's singular-value ratio makes it collinear.
    let t = Thresholds { rank_tol: 0.5, ..Thresholds::default() };
    assert!(matches!(check_points(Primitive::Circle3, &view, &t), Err(FitError::Collinear { rank: 1, .. })));
    for bad in [-1e-9, 1.0, f64::NAN, f64::INFINITY] {
        let t = Thresholds { rank_tol: bad, ..Thresholds::default() };
        assert_eq!(
            check_points(Primitive::Circle3, &view, &t).unwrap_err(),
            FitError::InvalidParams { field: "thresholds.rank_tol" }
        );
    }
    for bad in [-0.1, 7.0, f64::NAN] {
        let t = Thresholds { min_coverage: bad, ..Thresholds::default() };
        assert_eq!(
            check_points(Primitive::Circle3, &view, &t).unwrap_err(),
            FitError::InvalidParams { field: "thresholds.min_coverage" }
        );
    }
}

fn ulp(x: f64) -> f64 {
    let a = x.abs();
    f64::from_bits(a.to_bits() + 1) - a
}

#[test]
fn ac2_far_origin_round_trip() {
    let text = read(FAR);
    let get = |key: &str| lines(&text).find(|(k, _)| *k == key).unwrap().1.to_string();
    let xyz = hexes(&get("xyz"));
    assert_eq!(xyz.len(), 3000);
    let view = PointsView::new(&xyz, None).unwrap();
    let c = Centred::new(&view).unwrap();
    let centroid = hexes(&get("centroid"));
    assert_eq!(c.map().centroid().map(f64::to_bits).to_vec(), centroid.iter().map(|x| x.to_bits()).collect::<Vec<_>>());
    assert_eq!(c.map().scale_exp(), get("scale_exp").parse::<i32>().unwrap());
    let scaled = hexes(&get("scaled"));
    assert_eq!(c.xyz().len(), scaled.len());
    for (i, (a, b)) in c.xyz().iter().zip(&scaled).enumerate() {
        assert_eq!(a.to_bits(), b.to_bits(), "scaled {i}");
    }
    let map = c.map();
    for (i, (q, p)) in c.xyz().chunks_exact(3).zip(xyz.chunks_exact(3)).enumerate() {
        let back = map.inverse([q[0], q[1], q[2]]).unwrap();
        let fwd = map.forward([p[0], p[1], p[2]]).unwrap();
        for (k, ((b, x), (f, y))) in back.iter().zip(p).zip(fwd.iter().zip(q)).enumerate() {
            assert!((b - x).abs() <= ulp(*x), "point {i} coordinate {k}");
            assert_eq!(f.to_bits(), y.to_bits(), "point {i} coordinate {k}");
        }
    }
    // Lengths scale exactly by the power of two.
    let r = 0.0123;
    assert_eq!(map.inverse_length(map.forward_length(r).unwrap()).unwrap().to_bits(), r.to_bits());
}

#[test]
fn scaling_is_a_power_of_two_and_exact_at_the_extremes() {
    // Tiny normal coordinates: the scale goes below 2^-1000 and the round trip stays exact.
    let t = f64::from_bits(0x0030_0000_0000_0000); // 2^-1020
    let xyz = [0.0, 0.0, 0.0, t, 0.0, 0.0, 0.0, t, 0.0, 0.0, 0.0, t];
    let c = Centred::new(&PointsView::new(&xyz, None).unwrap()).unwrap();
    let s = c.map().scale();
    assert_eq!(s.to_bits(), f64::from_bits(((c.map().scale_exp() + 1023) as u64) << 52).to_bits());
    for (q, p) in c.xyz().chunks_exact(3).zip(xyz.chunks_exact(3)) {
        assert_eq!(c.map().inverse([q[0], q[1], q[2]]).unwrap(), [p[0], p[1], p[2]]);
    }
    // Coincident points: RMS distance zero, scale 1, data all zero.
    let xyz = [5.0, 6.0, 7.0, 5.0, 6.0, 7.0];
    let c = Centred::new(&PointsView::new(&xyz, None).unwrap()).unwrap();
    assert_eq!((c.map().scale_exp(), c.map().centroid()), (0, [5.0, 6.0, 7.0]));
    assert!(c.xyz().iter().all(|x| x.to_bits() == 0));
    // Coincident points are collinear (rank 0) for a line and fine for a point.
    let v = PointsView::new(&xyz, None).unwrap();
    assert_eq!(
        check_points(Primitive::Line3, &v, &Thresholds::default()).unwrap_err(),
        FitError::Collinear { n: 2, rank: 0, cond: f64::INFINITY }
    );
    assert_eq!(check_points(Primitive::Point, &v, &Thresholds::default()).unwrap().1.rank, 0);
    // Overflowing spread is a typed error, not an infinity.
    let xyz = [f64::MAX, 0.0, 0.0, -f64::MAX, 0.0, 0.0];
    assert!(matches!(Centred::new(&PointsView::new(&xyz, None).unwrap()), Err(FitError::Num(_))));
    assert_eq!(
        Centred::new(&PointsView::new(&[], None).unwrap()).unwrap_err(),
        FitError::TooFewPoints { n: 0, need: 1 }
    );
}

#[test]
fn inverse_feature_maps_positions_and_lengths_only() {
    let xyz = [1.0, 2.0, 3.0, 3.0, 2.0, 3.0, 1.0, 6.0, 3.0, 3.0, 6.0, 7.0];
    let c = Centred::new(&PointsView::new(&xyz, None).unwrap()).unwrap();
    let m = c.map();
    let f = FittedFeature::Cone { apex: [0.5, -0.25, 0.0], d: [0.0, 0.6, 0.8], half_angle: 0.3 };
    let g = m.inverse_feature(&f).unwrap();
    let FittedFeature::Cone { apex, d, half_angle } = g else { panic!() };
    assert_eq!(apex, m.inverse([0.5, -0.25, 0.0]).unwrap());
    assert_eq!((d, half_angle), ([0.0, 0.6, 0.8], 0.3));
    let f = FittedFeature::Circle2 { c: [1.0, -1.0], r: 0.5 };
    let FittedFeature::Circle2 { c: cc, r } = m.inverse_feature(&f).unwrap() else { panic!() };
    let p = m.inverse([1.0, -1.0, 0.0]).unwrap();
    assert_eq!((cc, r), ([p[0], p[1]], 0.5 * m.scale()));
    let f = FittedFeature::Spline { degree: 3, knots: vec![0.0, 0.5, 1.0], control: vec![[0.0; 3], [1.0; 3]] };
    let FittedFeature::Spline { degree, knots, control } = m.inverse_feature(&f).unwrap() else { panic!() };
    assert_eq!((degree, knots), (3, vec![0.0, 0.5, 1.0]));
    assert_eq!(control, vec![m.centroid(), m.inverse([1.0; 3]).unwrap()]);
    assert_eq!(Normalisation::IDENTITY.inverse([1.5, 2.5, -3.5]).unwrap(), [1.5, 2.5, -3.5]);
}

fn sample_result() -> FitResult {
    let feature = FittedFeature::Plane { p: [0.1, -2.5, 1000.0], n: [0.0, 0.0, -1.0] };
    let diag = FitDiagnostics {
        grad_norm: 3.25e-12,
        rank: 3,
        cond: f64::INFINITY,
        iterations: 12,
        stop: StopReason::Gradient,
    };
    let cert = Certificate { support: vec![0, 4, 17], gap: 0.0 };
    let alg = AlgorithmId::parse("fit.plane.ls@1.0.0").unwrap();
    FitResult::assemble(&feature, vec![1e-6, -2e-6, 0.0, -0.0, 5e-7], diag, Some(cert), alg).unwrap()
}

#[test]
fn ac3_canonical_encoding_is_fixed_bytes() {
    let text = read(ENCODE);
    let want = lines(&text).find(|(k, _)| *k == "encoded").unwrap().1.as_bytes().to_vec();
    let r = sample_result();
    let a = encode(&r).unwrap();
    let b = encode(&r).unwrap();
    assert_eq!(a, b);
    assert_eq!(String::from_utf8(a.clone()).unwrap(), String::from_utf8(want).unwrap());
    // Every f64 is a 16-digit lowercase hex string: count the quoted 16-hex-digit tokens.
    let s = String::from_utf8(a).unwrap();
    let hex16 = s
        .split('"')
        .filter(|t| t.len() == 16 && t.bytes().all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c)))
        .count();
    // p (3) + n (3) + residuals (5) + stats (5 floats) + grad_norm + cond + gap.
    assert_eq!(hex16, 3 + 3 + 5 + 5 + 3);
    assert!(s.is_ascii() && !s.contains(' '));
}

#[test]
fn encoding_covers_every_feature_kind() {
    use FittedFeature as F;
    let v = [1.0, 2.0, 2.0];
    let feats = [
        F::Point { c: v },
        F::Line2 { p: [1.0, 2.0], d: [0.0, -1.0] },
        F::Line3 { p: v, d: v },
        F::Circle2 { c: [0.0, 0.0], r: 1.0 },
        F::Circle3 { c: v, n: v, r: 1.0 },
        F::Sphere { c: v, r: 1.0 },
        F::Cylinder { p: v, d: v, r: 1.0 },
        F::Cone { apex: v, d: [0.0, 0.0, -2.0], half_angle: 0.5 },
        F::Torus { c: v, n: v, major_r: 2.0, minor_r: 0.5 },
        F::Slot { c: v, n: [0.0, 0.0, 1.0], d: [1.0, 0.0, 0.0], length: 3.0, width: 1.0 },
        F::Rectangle { c: v, n: [0.0, 0.0, 1.0], d: [1.0, 0.0, 0.0], length: 3.0, width: 1.0 },
        F::Ellipse { c: v, n: [0.0, 0.0, 1.0], d: [1.0, 0.0, 0.0], a: 3.0, b: 1.0 },
        F::Paraboloid { apex: v, d: [0.0, -1.0, 0.0], focal: 0.25 },
        F::Polyline { vertices: vec![v, [0.0; 3]] },
        F::Spline { degree: 1, knots: vec![0.0, 0.0, 1.0, 1.0], control: vec![v, [0.0; 3]] },
    ];
    for f in feats {
        let p = f.primitive();
        let alg = algorithm_id(p, Method::LeastSquares, (1, 2, 3)).unwrap();
        let diag = FitDiagnostics { grad_norm: 0.0, rank: 1, cond: 1.0, iterations: 0, stop: StopReason::ClosedForm };
        let r = FitResult::assemble(&f, vec![0.5], diag, None, alg).unwrap();
        let s = String::from_utf8(encode(&r).unwrap()).unwrap();
        let head = format!("{{\"algorithm\":\"fit.{}.ls@1.2.3\",\"feature\":{{\"kind\":\"{}\"", p.slug(), p.slug());
        assert!(s.starts_with(&head), "{s}");
        assert!(s.ends_with("\"stop\":\"closed_form\"},\"certificate\":null}"), "{s}");
    }
    // Oriented axes keep their sign; sign-free ones are normalised.
    let f = F::Cone { apex: v, d: [0.0, 0.0, -2.0], half_angle: 0.5 }.canonical().unwrap();
    assert_eq!(f, F::Cone { apex: v, d: [0.0, 0.0, -1.0], half_angle: 0.5 });
    let f = F::Line2 { p: [1.0, 2.0], d: [0.0, -3.0] }.canonical().unwrap();
    let F::Line2 { d, .. } = f else { panic!() };
    assert_eq!(d.map(f64::to_bits), [0.0f64.to_bits(), 1.0f64.to_bits()]);
    assert_eq!(F::Plane { p: v, n: [0.0, 0.0, 0.0] }.canonical().unwrap_err(), FitError::InvalidDirection);
    assert!(matches!(F::Sphere { c: v, r: f64::NAN }.canonical(), Err(FitError::Num(NumError::NotANumber { .. }))));
}

#[test]
fn ac4_nan_at_point_7_is_refused() {
    let mut xyz = vec![0.25; 30];
    xyz[22] = f64::NAN; // point 7, y
    assert_eq!(PointsView::new(&xyz, None).unwrap_err(), FitError::NonFinite { index: 7 });
    xyz[22] = 0.25;
    xyz[21] = f64::NEG_INFINITY;
    assert_eq!(PointsView::new(&xyz, None).unwrap_err(), FitError::NonFinite { index: 7 });
    xyz[21] = 0.25;
    xyz[5] = f64::from_bits(1); // subnormal in point 1
    assert_eq!(PointsView::new(&xyz, None).unwrap_err(), FitError::Subnormal { index: 1 });
    xyz[5] = -0.0; // signed zero is fine
    assert_eq!(PointsView::new(&xyz, None).unwrap().len(), 10);
    assert_eq!(PointsView::new(&xyz[..29], None).unwrap_err(), FitError::BadLength { len: 29 });
    let normals = vec![0.0; 27];
    assert_eq!(
        PointsView::new(&xyz, Some(normals.as_slice())).unwrap_err(),
        FitError::LengthMismatch { points: 30, normals: 27 }
    );
    let mut normals = [0.0, 0.0, 1.0].repeat(10);
    let v = PointsView::new(&xyz, Some(normals.as_slice())).unwrap();
    assert_eq!(v.normal(9), Some([0.0, 0.0, 1.0]));
    assert_eq!((v.point(10), v.normal(10)), (None, None));
    normals[13] = f64::NAN;
    assert_eq!(PointsView::new(&xyz, Some(normals.as_slice())).unwrap_err(), FitError::NonFiniteNormal { index: 4 });
    normals[13] = 0.0;
    normals[29] = 1.01;
    assert_eq!(PointsView::new(&xyz, Some(normals.as_slice())).unwrap_err(), FitError::NonUnitNormal { index: 9 });
    normals[29] = 1.0 + 5e-7;
    assert!(PointsView::new(&xyz, Some(normals.as_slice())).is_ok());
}

#[test]
fn ac5_direction_sign_normalisation() {
    let a = normalize_direction([0.0, 0.0, -1.0]).unwrap();
    let b = normalize_direction([0.0, 0.0, 1.0]).unwrap();
    let want = [0.0f64.to_bits(), 0.0f64.to_bits(), 1.0f64.to_bits()];
    assert_eq!(a.map(f64::to_bits), want);
    assert_eq!(b.map(f64::to_bits), want);
    assert_eq!(normalize_direction([-0.0, -3.0, 4.0]).unwrap(), [0.0, 0.6, -0.8]);
    assert_eq!(normalize_direction([-0.0, -3.0, 4.0]).unwrap()[0].to_bits(), 0);
    assert_eq!(normalize_direction2([-4.0, 3.0]).unwrap(), [0.8, -0.6]);
    assert_eq!(unit_direction([0.0, -2.0, 0.0]).unwrap(), [0.0, -1.0, 0.0]);
    assert_eq!(normalize_direction([0.0; 3]).unwrap_err(), FitError::InvalidDirection);
    assert_eq!(normalize_direction([1e-310, 0.0, 0.0]).unwrap_err(), FitError::InvalidDirection);
    assert!(matches!(normalize_direction([f64::NAN, 0.0, 1.0]), Err(FitError::Num(_))));
    // Equal geometry, equal parameters: a plane with either normal gives the same bits.
    let p = [1.0, 2.0, 3.0];
    let f1 = FittedFeature::Plane { p, n: [0.0, 0.0, -2.0] }.canonical().unwrap();
    let f2 = FittedFeature::Plane { p, n: [0.0, 0.0, 5.0] }.canonical().unwrap();
    assert_eq!(f1, f2);
}

#[test]
fn foot_of_perpendicular_from_centroid() {
    use fid_fit::contract::foot_of_perpendicular;
    let f = foot_of_perpendicular([1.0, 1.0, 5.0], [0.0, 0.0, 0.0], [0.0, 0.0, -2.0]).unwrap();
    assert_eq!(f, [0.0, 0.0, 5.0]);
}

#[test]
fn ac6_command_rows_declare_quantity_and_unit() {
    let row = command_row(Primitive::Plane, Method::LeastSquares, (1, 0, 0), Determinism::D0).unwrap();
    row.validate().unwrap();
    let want = read(ROW);
    assert_eq!(String::from_utf8(row.to_json().unwrap()).unwrap() + "\n", want);
    for f in row.params.iter().chain(&row.result) {
        assert_eq!(f.ty.is_numeric(), f.quantity.is_some(), "{}", f.name);
    }
    // Every defined (primitive, method) pair has a valid row.
    let mut count = 0;
    for &p in Primitive::ALL {
        for &m in Method::ALL {
            match command_row(p, m, (1, 0, 0), Determinism::D0) {
                Ok(r) => {
                    r.validate().unwrap();
                    assert_eq!(r.command.as_str(), format!("fit.{}_{}@1", m.slug(), p.slug()));
                    count += 1;
                }
                Err(e) => assert_eq!(e, FitError::MethodUndefined { primitive: p, method: m }),
            }
        }
    }
    assert_eq!(count, 16 + 15 + 4 + 4 + 3);
}

fn tamper(f: impl Fn(&mut CommandRow)) -> Result<(), RowError> {
    let mut row = command_row(Primitive::Cone, Method::LeastSquares, (1, 0, 0), Determinism::D0).unwrap();
    f(&mut row);
    row.validate()
}

#[test]
fn ac6_row_validation_errors() {
    assert_eq!(tamper(|r| r.params[0].quantity = None), Err(RowError::MissingQuantity { field: "points" }));
    assert_eq!(
        tamper(|r| r.result[3].quantity = Some(("angle", "deg"))),
        Err(RowError::UnknownQuantity { field: "feature.half_angle" })
    );
    assert_eq!(
        tamper(|r| r.params[11].quantity = Some(("count", "1"))),
        Err(RowError::UnexpectedQuantity { field: "nlls.method" })
    );
    let dup = Field { name: "points", ty: ValueType::Boolean, quantity: None, required: false };
    assert_eq!(tamper(|r| r.params.push(dup)), Err(RowError::DuplicateField { field: "points" }));
    assert_eq!(tamper(|r| r.side_effect = SideEffect::W1), Err(RowError::SideEffect));
    assert_eq!(tamper(|r| r.dry_run = SideEffect::R0), Err(RowError::SideEffect));
    assert_eq!(tamper(|r| r.determinism = Determinism::N), Err(RowError::Determinism));
    assert_eq!(
        tamper(|r| r.algorithm = AlgorithmId::parse("fit.cylinder.ls@1.0.0").unwrap()),
        Err(RowError::CommandMismatch)
    );
    assert_eq!(tamper(|_| {}), Ok(()));
}

#[test]
fn algorithm_ids_and_undefined_methods() {
    let id = algorithm_id(Primitive::Circle3, Method::MinimumZone, (1, 0, 2)).unwrap();
    assert_eq!(id.to_string(), "fit.circle3.mz@1.0.2");
    assert_eq!(AlgorithmId::parse(&id.to_string()).unwrap(), id);
    assert_eq!(
        algorithm_id(Primitive::Plane, Method::MaximumInscribed, (1, 0, 0)).unwrap_err(),
        FitError::MethodUndefined { primitive: Primitive::Plane, method: Method::MaximumInscribed }
    );
    for &p in Primitive::ALL {
        assert_eq!(Primitive::parse(p.slug()), Some(p));
        assert!(p.requirement().min_points >= 1 && p.requirement().rank <= 3);
    }
}

#[test]
fn residual_statistics() {
    let s = ResidualStats::of(&[3.0, -1.0, -0.0, 0.0]).unwrap();
    assert_eq!((s.n, s.max, s.min), (4, 3.0, -1.0));
    assert_eq!(s.mean, 0.5);
    assert_eq!(s.rms, (10.0f64 / 4.0).sqrt());
    assert_eq!(s.std_dev, 1.5);
    let s = ResidualStats::of(&[-0.0, 0.0]).unwrap();
    assert_eq!((s.max.to_bits(), s.min.to_bits()), (0.0f64.to_bits(), (-0.0f64).to_bits()));
    assert!(matches!(ResidualStats::of(&[]), Err(FitError::Num(NumError::EmptyInput { .. }))));
    assert!(matches!(ResidualStats::of(&[1.0, f64::NAN]), Err(FitError::Num(NumError::NotANumber { .. }))));
}

#[test]
fn params_validation() {
    let xyz = [0.0; 9];
    let v = PointsView::new(&xyz, None).unwrap();
    let ok = FitParams::new(Method::LeastSquares);
    ok.validate(&v).unwrap();
    let field = |p: FitParams| match p.validate(&v) {
        Err(FitError::InvalidParams { field }) => field,
        other => panic!("{other:?}"),
    };
    let with = |f: &dyn Fn(&mut FitParams)| {
        let mut p = FitParams::new(Method::LeastSquares);
        f(&mut p);
        p
    };
    assert_eq!(
        field(with(&|p| p.constraints = vec![Constraint::FixedRadius(1.0), Constraint::FixedRadius(2.0)])),
        "constraints"
    );
    assert_eq!(field(with(&|p| p.constraints = vec![Constraint::FixedRadius(0.0)])), "constraints.fixed_radius");
    assert_eq!(field(with(&|p| p.constraints = vec![Constraint::FixedRadius(1e-310)])), "constraints.fixed_radius");
    assert_eq!(
        field(with(&|p| p.constraints = vec![Constraint::FixedAxisDirection([0.0; 3])])),
        "constraints.fixed_axis_direction"
    );
    assert_eq!(
        field(with(&|p| p.constraints = vec![Constraint::ProjectionPlane { p: [f64::NAN; 3], n: [0.0, 0.0, 1.0] }])),
        "constraints.projection_plane.p"
    );
    assert_eq!(field(with(&|p| p.weights = Weights::PerPoint(vec![1.0, 1.0]))), "weights");
    assert_eq!(field(with(&|p| p.weights = Weights::PerPoint(vec![1.0, 0.0, 1.0]))), "weights");
    assert_eq!(field(with(&|p| p.weights = Weights::PerPoint(vec![1.0, f64::INFINITY, 1.0]))), "weights");
    assert_eq!(field(with(&|p| p.nlls.max_iter = usize::MAX)), "nlls.max_iter");
    assert_eq!(field(with(&|p| p.nlls.tol_grad = -1.0)), "nlls.tol_grad");
    assert_eq!(field(with(&|p| p.nlls.cond_limit = 0.5)), "nlls.cond_limit");
    assert_eq!(field(with(&|p| p.nlls.rank_tol = 1.0)), "nlls.rank_tol");
    assert_eq!(field(with(&|p| p.nlls.lm_tau = 0.0)), "nlls.lm_tau");
    assert_eq!(field(with(&|p| p.thresholds.rank_tol = f64::NAN)), "thresholds.rank_tol");
    let p = with(&|p| {
        p.constraints = vec![
            Constraint::FixedRadius(0.01),
            Constraint::FixedAxisDirection([0.0, 0.0, 1.0]),
            Constraint::ProjectionPlane { p: [0.0; 3], n: [1.0, 0.0, 0.0] },
        ];
        p.weights = Weights::PerPoint(vec![1.0, 2.0, 0.5]);
    });
    p.validate(&v).unwrap();
    assert_eq!(p.fixed_radius(), Some(0.01));
    assert_eq!(p.fixed_axis_direction(), Some([0.0, 0.0, 1.0]));
    assert_eq!(p.projection_plane(), Some(([0.0; 3], [1.0, 0.0, 0.0])));
}

#[test]
fn solver_errors_map_to_fit_errors() {
    let last = Diagnostics { iterations: 7, ..Diagnostics::default() };
    let cfg = NllsConfig { max_iter: 7, ..NllsConfig::default() };
    let e = SolverError::IterationLimit { last };
    assert_eq!(
        FitError::from_solver(e, &cfg),
        FitError::NotConverged { iterations: 7, reason: NotConvergedReason::MaxIter, last: Some(last) }
    );
    let cfg = NllsConfig { max_iter: 200, ..NllsConfig::default() };
    assert_eq!(
        FitError::from_solver(e, &cfg),
        FitError::NotConverged { iterations: 7, reason: NotConvergedReason::TrialsExhausted, last: Some(last) }
    );
    // Without the configuration the driver error is kept whole, diagnostics included.
    assert_eq!(FitError::from(e), FitError::Solver(e));
    assert_eq!(
        FitError::from_solver(SolverError::IllConditioned { cond: None, last }, &cfg),
        FitError::IllConditioned { cond: None }
    );
    assert_eq!(
        FitError::from(SolverError::IllConditioned { cond: Some(1e13), last }),
        FitError::IllConditioned { cond: Some(1e13) }
    );
    let e = SolverError::RankDeficient { rank: 2, cond: None, last };
    assert_eq!(FitError::from(e), FitError::Solver(e));
    assert_eq!(FitError::from(e).code(), "solver");
    assert_eq!(StopReason::from(fid_solv::nlls::StopReason::Cost), StopReason::Cost);
}

/// A minimal method through the trait: the centroid as a fitted point.
struct CentroidFit;

impl Fit for CentroidFit {
    fn algorithm_id(&self) -> AlgorithmId {
        algorithm_id(Primitive::Point, Method::LeastSquares, (0, 1, 0)).unwrap()
    }

    fn fit(&self, pts: &PointsView<'_>, params: &FitParams, _seed: Option<u64>) -> Result<FitResult, FitError> {
        params.validate(pts)?;
        let (c, d) = check_points(Primitive::Point, pts, &params.thresholds)?;
        let feature = c.map().inverse_feature(&FittedFeature::Point { c: [0.0; 3] })?;
        let mut residuals = Vec::new();
        for q in c.xyz().chunks_exact(3) {
            residuals.push(c.map().inverse_length(fid_num::red::norm2(q)?)?);
        }
        let diag =
            FitDiagnostics { grad_norm: 0.0, rank: d.rank, cond: d.cond, iterations: 0, stop: StopReason::ClosedForm };
        FitResult::assemble(&feature, residuals, diag, None, self.algorithm_id())
    }
}

#[test]
fn a_method_through_the_trait() {
    let xyz = [1.0, 2.0, 3.0, 3.0, 2.0, 3.0];
    let v = PointsView::new(&xyz, None).unwrap();
    let r = CentroidFit.fit(&v, &FitParams::new(Method::LeastSquares), None).unwrap();
    assert_eq!(r.feature, FittedFeature::Point { c: [2.0, 2.0, 3.0] });
    assert_eq!(r.residuals, vec![1.0, 1.0]);
    assert_eq!((r.stats.rms, r.stats.std_dev), (1.0, 0.0));
    assert_eq!(r.algorithm.to_string(), "fit.point.ls@0.1.0");
    assert_eq!(
        CentroidFit.fit(&PointsView::new(&[], None).unwrap(), &FitParams::new(Method::LeastSquares), None).unwrap_err(),
        FitError::TooFewPoints { n: 0, need: 1 }
    );
    let bad = AlgorithmId::parse("fit.plane.ls@1.0.0").unwrap();
    let diag = FitDiagnostics { grad_norm: 0.0, rank: 0, cond: 1.0, iterations: 0, stop: StopReason::ClosedForm };
    assert_eq!(
        FitResult::assemble(&r.feature, vec![0.0], diag, None, bad).unwrap_err(),
        FitError::InvalidParams { field: "algorithm" }
    );
}

#[test]
fn degeneracy_on_large_inputs_spans_several_blocks() {
    // 10 000 points on a tilted plane (dyadic, exactly coplanar): more than two QR blocks.
    let mut xyz = Vec::new();
    for i in 0..10_000u32 {
        let x = f64::from(i % 100) / 64.0;
        let y = f64::from(i / 100) / 64.0;
        xyz.extend_from_slice(&[x, y, x / 2.0 - y / 4.0]);
    }
    let v = PointsView::new(&xyz, None).unwrap();
    let (_, d) = check_points(Primitive::Plane, &v, &Thresholds::default()).unwrap();
    assert_eq!(d.rank, 2);
    // The plane normal is the third axis: (-1/2, 1/4, 1) normalised and sign-normalised.
    let n = normalize_direction([-0.5, 0.25, 1.0]).unwrap();
    for (a, b) in d.shape.axes[2].iter().zip(n) {
        assert!((a - b).abs() < 1e-12, "{:?} vs {n:?}", d.shape.axes[2]);
    }
    assert!(matches!(
        check_points(Primitive::Sphere, &v, &Thresholds::default()),
        Err(FitError::Coplanar { n: 10_000, rank: 2, .. })
    ));
}

#[test]
fn tiny_third_spread_is_coplanar_not_a_stall() {
    // Regression for the Jacobi stall: a 20 x 20 grid of 1 m with a pseudo-random z spread of
    // `eps` (`z = eps * w / 50`, `w` in [-50, 50]). For eps <= ~1e-160 the squared third column
    // norm underflows; the old tolerance `sqrt(a) * sqrt(b)` became 0 and the sweep loop
    // returned NotConverged. Every case must be rank 2: Coplanar for a sphere.
    let mut cases = 0;
    for a in 1..=20i64 {
        for k in (160..=300).step_by(10) {
            let eps: f64 = format!("1e-{k}").parse().unwrap();
            let mut xyz = Vec::new();
            for i in 0..400i64 {
                let x = (i % 20) as f64 / 20.0;
                let y = (i / 20) as f64 / 20.0;
                let w = ((i * a * 37 + i / 7) % 101) - 50;
                xyz.extend_from_slice(&[x, y, eps * (w as f64 / 50.0)]);
            }
            let v = PointsView::new(&xyz, None).unwrap();
            match check_points(Primitive::Sphere, &v, &Thresholds::default()) {
                Err(FitError::Coplanar { n: 400, rank: 2, .. }) => {}
                other => panic!("a {a}, eps 1e-{k}: {other:?}"),
            }
            let (_, d) = check_points(Primitive::Plane, &v, &Thresholds::default())
                .unwrap_or_else(|e| panic!("a {a}, eps 1e-{k}: {e:?}"));
            assert_eq!(d.rank, 2, "a {a}, eps 1e-{k}");
            cases += 1;
        }
    }
    assert_eq!(cases, 20 * 15);
}

#[test]
fn canonical_refuses_out_of_range_values() {
    use FittedFeature as F;
    let v = [1.0, 2.0, 3.0];
    let z = [0.0, 0.0, 1.0];
    let x = [1.0, 0.0, 0.0];
    let bad = |f: F, field: &'static str| {
        assert_eq!(f.canonical(), Err(FitError::InvalidParams { field }), "{f:?}");
    };
    for r in [0.0, -0.0, -1.0] {
        bad(F::Circle2 { c: [0.0, 0.0], r }, "feature.r");
        bad(F::Circle3 { c: v, n: z, r }, "feature.r");
        bad(F::Sphere { c: v, r }, "feature.r");
        bad(F::Cylinder { p: v, d: z, r }, "feature.r");
    }
    bad(F::Torus { c: v, n: z, major_r: -1.0, minor_r: 0.5 }, "feature.major_r");
    bad(F::Torus { c: v, n: z, major_r: 1.0, minor_r: -0.5 }, "feature.minor_r");
    bad(F::Slot { c: v, n: z, d: x, length: -1.0, width: 1.0 }, "feature.length");
    bad(F::Slot { c: v, n: z, d: x, length: 1.0, width: -1.0 }, "feature.width");
    bad(F::Rectangle { c: v, n: z, d: x, length: -1.0, width: 1.0 }, "feature.length");
    bad(F::Rectangle { c: v, n: z, d: x, length: 1.0, width: -1.0 }, "feature.width");
    bad(F::Paraboloid { apex: v, d: z, focal: -0.25 }, "feature.focal");
    bad(F::Paraboloid { apex: v, d: z, focal: 0.0 }, "feature.focal");
    bad(F::Torus { c: v, n: z, major_r: 0.0, minor_r: 0.5 }, "feature.major_r");
    bad(F::Torus { c: v, n: z, major_r: 1.0, minor_r: 0.0 }, "feature.minor_r");
    bad(F::Ellipse { c: v, n: z, d: x, a: 1.0, b: 0.0 }, "feature.b");
    bad(F::Rectangle { c: v, n: z, d: x, length: 1.0, width: 0.0 }, "feature.width");
    bad(F::Rectangle { c: v, n: z, d: x, length: 1.0, width: 2.0 }, "feature.length");
    bad(F::Ellipse { c: v, n: z, d: x, a: 1.0, b: 2.0 }, "feature.a");
    bad(F::Ellipse { c: v, n: z, d: x, a: 1.0, b: -0.5 }, "feature.b");
    for h in [0.0, -0.1, core::f64::consts::FRAC_PI_2, 2.0] {
        bad(F::Cone { apex: v, d: z, half_angle: h }, "feature.half_angle");
    }
    bad(F::Spline { degree: 0, knots: vec![0.0, 1.0], control: vec![v] }, "feature.degree");
    bad(F::Spline { degree: 3, knots: vec![0.0; 6], control: vec![v; 2] }, "feature.control");
    bad(F::Spline { degree: 3, knots: vec![0.0; 7], control: vec![v; 4] }, "feature.knots");
    bad(F::Spline { degree: 3, knots: vec![0.0; 9], control: vec![v; 4] }, "feature.knots");
    // Boundary values that are in range.
    assert!(F::Spline { degree: 3, knots: vec![0.0; 8], control: vec![v; 4] }.canonical().is_ok());
    assert!(F::Ellipse { c: v, n: z, d: x, a: 1.0, b: 1.0 }.canonical().is_ok());
    assert!(F::Torus { c: v, n: z, major_r: 1e-3, minor_r: 1e-3 }.canonical().is_ok());
    assert!(F::Rectangle { c: v, n: z, d: x, length: 1.0, width: 1.0 }.canonical().is_ok());
    // A slot's -0.0 length becomes +0.0, so both zeros encode identically.
    let F::Slot { length, width, .. } = F::Slot { c: v, n: z, d: x, length: -0.0, width: 1.0 }.canonical().unwrap()
    else {
        panic!()
    };
    assert_eq!((length.to_bits(), width), (0, 1.0));
    // A degree near u32::MAX must not overflow the count arithmetic.
    for degree in [u32::MAX, u32::MAX - 1] {
        let got = F::Spline { degree, knots: vec![0.0; 4], control: vec![v; 2] }.canonical().unwrap_err();
        assert!(
            got == FitError::InvalidParams { field: "feature.degree" }
                || got == FitError::InvalidParams { field: "feature.control" },
            "{got:?}"
        );
    }
    assert!(F::Cone { apex: v, d: z, half_angle: 1.5 }.canonical().is_ok());
}
