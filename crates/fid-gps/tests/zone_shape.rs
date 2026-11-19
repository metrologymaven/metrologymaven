//! Acceptance criteria of spec node GDT.ZONE.ZTYPE (AC-1 .. AC-5) for the zone shapes, plus
//! the error order and limits pinned in the leaf's Design. Expected values come from the
//! independent stdlib-Python reference `fixtures/gdt/zone/make_shapes_fixtures.py`:
//! `shapes_analytic.json` (exact-rational oracle) and `shapes_random_v1.json` (the pinned
//! binary64 expressions re-implemented in Python over 1 000 000 Philox points). The property
//! test checks invariants only and needs no expected values.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::CValue;
use fid_geo_frame::Rigid;
use fid_gps::zone::shape::{Pose, Usage, ZoneError, ZoneInput, ZoneShape, P3};
use fid_num::rng::{Address, Cursor};
use std::collections::BTreeMap;
use std::path::Path;

fn fixture(name: &str) -> CValue {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/gdt/zone").join(name);
    fid_canon::parse(&std::fs::read(path).unwrap()).unwrap()
}

fn obj(v: &CValue) -> &BTreeMap<String, CValue> {
    match v {
        CValue::Obj(m) => m,
        other => panic!("not an object: {other:?}"),
    }
}

fn arr(v: &CValue) -> &[CValue] {
    match v {
        CValue::Arr(a) => a,
        other => panic!("not an array: {other:?}"),
    }
}

fn st(v: &CValue) -> &str {
    match v {
        CValue::Str(s) => s,
        other => panic!("not a string: {other:?}"),
    }
}

fn int(v: &CValue) -> u64 {
    match v {
        CValue::Num(x) => {
            assert!(*x >= 0.0 && x.fract() == 0.0);
            *x as u64
        }
        other => panic!("not a number: {other:?}"),
    }
}

/// An f64 written as "0x" + 16 hex digits of its bits.
fn hf(v: &CValue) -> f64 {
    let s = st(v);
    f64::from_bits(u64::from_str_radix(s.strip_prefix("0x").unwrap(), 16).unwrap())
}

fn p3(v: &CValue) -> P3 {
    let a = arr(v);
    assert_eq!(a.len(), 3);
    [hf(&a[0]), hf(&a[1]), hf(&a[2])]
}

fn shape_of(v: &CValue) -> ZoneShape {
    let m = obj(v);
    match st(&m["kind"]) {
        "two_planes" => ZoneShape::TwoPlanes,
        "two_lines" => ZoneShape::TwoLines { plane: p3(&m["plane"]) },
        "cylinder" => ZoneShape::Cylinder,
        "circle" => ZoneShape::Circle { plane: p3(&m["plane"]) },
        "sphere" => ZoneShape::Sphere,
        "offset" => ZoneShape::Offset { disposition_shift: hf(&m["disposition_shift"]) },
        other => panic!("unknown shape {other}"),
    }
}

fn pose_of(v: &CValue) -> Pose {
    let m = obj(v);
    Pose::new(p3(&m["origin"]), p3(&m["direction"]))
}

/// Ordered distance in ulps between two finite doubles.
fn ulps(a: f64, b: f64) -> u64 {
    fn key(x: f64) -> i64 {
        let u = x.to_bits() as i64;
        if u >= 0 {
            u
        } else {
            -(u & i64::MAX)
        }
    }
    key(a).abs_diff(key(b))
}

struct Case {
    name: String,
    shape: ZoneShape,
    pose: Pose,
    points: Vec<P3>,
    deviations: Option<Vec<f64>>,
    expect: BTreeMap<String, CValue>,
}

impl Case {
    fn input(&self) -> ZoneInput<'_> {
        match &self.deviations {
            Some(d) => ZoneInput::Deviations { points: &self.points, deviations: d },
            None => ZoneInput::Points(&self.points),
        }
    }
}

fn analytic_cases() -> Vec<Case> {
    let v = fixture("shapes_analytic.json");
    assert_eq!(st(&obj(&v)["leaf"]), "GDT.ZONE.ZTYPE");
    arr(&obj(&v)["cases"])
        .iter()
        .map(|c| {
            let m = obj(c);
            Case {
                name: st(&m["name"]).to_string(),
                shape: shape_of(&m["shape"]),
                pose: pose_of(&m["pose"]),
                points: arr(&m["points"]).iter().map(p3).collect(),
                deviations: m.get("deviations").map(|d| arr(d).iter().map(hf).collect()),
                expect: obj(&m["expect"]).clone(),
            }
        })
        .collect()
}

/// AC-1 (sizes and driving indices), AC-2 (the `tie_*` cases) and AC-4 (the `err_*` cases).
#[test]
fn ac1_ac2_ac4_analytic_fixture() {
    let cases = analytic_cases();
    assert!(cases.len() >= 60);
    let (mut ok, mut err) = (0, 0);
    let mut classes = Vec::new();
    for c in &cases {
        let got = c.shape.usage(&c.pose, c.input());
        if let Some(code) = c.expect.get("error") {
            let e = got.expect_err(&c.name);
            assert_eq!(e.code(), st(code), "{}: {e}", c.name);
            err += 1;
            continue;
        }
        let u = got.unwrap_or_else(|e| panic!("{}: {e}", c.name));
        let want = hf(&c.expect["size"]);
        assert_eq!(u64::from(u.driving_index), int(&c.expect["index"]), "{}", c.name);
        // Every success case: the Design's absolute bound (plus 1 ulp of the expected size).
        let tol = hf(&c.expect["abs_tol"]);
        assert!((u.size_m - want).abs() <= tol, "{}: {} vs {want} (tol {tol:e})", c.name, u.size_m);
        // Stronger agreement only for the fixture classes that have it.
        match st(&c.expect["check"]) {
            "bits" => assert_eq!(u.size_m.to_bits(), want.to_bits(), "{}: {} vs {want}", c.name, u.size_m),
            "ulp1" => assert!(ulps(u.size_m, want) <= 1, "{}: {} vs {want}", c.name, u.size_m),
            "abs_bound" => {}
            other => panic!("{}: check class {other}", c.name),
        }
        classes.push(st(&c.expect["check"]).to_string());
        ok += 1;
    }
    assert!(ok >= 30 && err >= 30, "{ok} ok, {err} err");
    for class in ["bits", "ulp1", "abs_bound"] {
        assert!(classes.iter().any(|k| k == class), "{class}");
    }
    // The documented underflow: a point 1e-163 m off the reference reads size 0.
    let tiny = cases.iter().find(|c| c.name == "hard_sphere_1e_163").unwrap();
    assert_eq!(tiny.shape.usage(&tiny.pose, tiny.input()).unwrap().size_m, 0.0);
    // Every shape is covered by at least one successful and one empty-set case.
    for kind in ["two_planes", "two_lines", "cylinder", "circle", "sphere", "offset"] {
        assert!(cases.iter().any(|c| c.shape.name() == kind && c.expect.contains_key("size")), "{kind}");
        assert!(cases.iter().any(|c| c.shape.name() == kind && c.name.starts_with("err_empty")), "{kind}");
    }
}

/// AC-3: `contains` is true at the usage size (also the oracle's size when it is exact) and
/// false one f64 below it; error inputs give the same error as `usage`.
#[test]
fn ac3_contains_exact_boundary() {
    for c in analytic_cases() {
        let Ok(u) = c.shape.usage(&c.pose, c.input()) else {
            let e = c.shape.usage(&c.pose, c.input()).unwrap_err();
            assert_eq!(c.shape.contains(&c.pose, 1.0, c.input()), Err(e), "{}", c.name);
            continue;
        };
        assert_eq!(c.shape.contains(&c.pose, u.size_m, c.input()), Ok(true), "{}", c.name);
        assert_eq!(c.shape.contains(&c.pose, u.size_m.next_down(), c.input()), Ok(false), "{}", c.name);
        assert_eq!(c.shape.contains(&c.pose, u.size_m.next_up(), c.input()), Ok(true), "{}", c.name);
        if st(&c.expect["check"]) == "bits" {
            let want = hf(&c.expect["size"]);
            assert_eq!(c.shape.contains(&c.pose, want, c.input()), Ok(true), "{}", c.name);
            assert_eq!(c.shape.contains(&c.pose, want.next_down(), c.input()), Ok(false), "{}", c.name);
        }
    }
}

#[test]
fn contains_size_rules() {
    let pose = Pose::new([0.0; 3], [0.0, 0.0, 1.0]);
    let pts = [[0.0, 0.0, 0.25]];
    let s = ZoneShape::TwoPlanes;
    assert_eq!(s.contains(&pose, 0.5, ZoneInput::Points(&pts)), Ok(true));
    assert_eq!(s.contains(&pose, -0.5, ZoneInput::Points(&pts)), Ok(false));
    assert_eq!(s.contains(&pose, f64::NAN, ZoneInput::Points(&pts)), Err(ZoneError::NanInput));
    assert_eq!(s.contains(&pose, f64::INFINITY, ZoneInput::Points(&pts)), Err(ZoneError::NonFiniteInput));
    // Emptiness is checked before the size.
    assert_eq!(s.contains(&pose, f64::NAN, ZoneInput::Points(&[])), Err(ZoneError::EmptyPoints));
    // A zero-size zone contains points lying exactly on the datum plane, and -0.0 == 0.0.
    let on = [[3.0, -2.0, 0.0]];
    assert_eq!(s.contains(&pose, 0.0, ZoneInput::Points(&on)), Ok(true));
    assert_eq!(s.contains(&pose, -0.0, ZoneInput::Points(&on)), Ok(true));
}

/// AC-2 beyond the fixture: many identical maxima, the first wins, for every shape.
#[test]
fn ac2_first_of_many_ties() {
    let pose = Pose::new([0.0; 3], [0.0, 0.0, 1.0]);
    let pts: Vec<P3> = (0..100).map(|i| if i < 7 { [0.0, 0.0, 0.0] } else { [0.5, 0.0, 0.5] }).collect();
    let devs: Vec<f64> = (0..100).map(|i| if i < 7 { 0.0 } else { 0.5 }).collect();
    for shape in [
        ZoneShape::TwoPlanes,
        ZoneShape::TwoLines { plane: [0.0, 1.0, 0.0] },
        ZoneShape::Cylinder,
        ZoneShape::Circle { plane: [0.0, 0.0, 1.0] },
        ZoneShape::Sphere,
    ] {
        assert_eq!(shape.usage(&pose, ZoneInput::Points(&pts)).unwrap().driving_index, 7, "{}", shape.name());
    }
    let off = ZoneShape::Offset { disposition_shift: 0.0 };
    let u = off.usage(&pose, ZoneInput::Deviations { points: &pts, deviations: &devs }).unwrap();
    assert_eq!(u, Usage { size_m: 1.0, driving_index: 7 });
}

#[test]
fn error_details_and_display() {
    let pose = Pose::new([0.0; 3], [0.0, 0.0, 1.1]);
    let e = ZoneShape::TwoPlanes.usage(&pose, ZoneInput::Points(&[[0.0; 3]])).unwrap_err();
    match e {
        ZoneError::NonUnitDirection { norm } => assert!((norm - 1.1).abs() < 1e-15),
        other => panic!("{other:?}"),
    }
    assert!(e.to_string().starts_with("non_unit_direction"));
    let off = ZoneShape::Offset { disposition_shift: 0.0 };
    let e = off.usage(&pose, ZoneInput::Deviations { points: &[[0.0; 3]; 3], deviations: &[0.0; 2] }).unwrap_err();
    assert_eq!(e, ZoneError::DeviationCountMismatch { points: 3, deviations: 2 });
    assert_eq!(e.code(), "deviation_count_mismatch");
    let pose = Pose::new([0.0; 3], [0.0, 0.0, 1.0]);
    let e = ZoneShape::Sphere.usage(&pose, ZoneInput::Points(&[[0.0; 3], [1e200, 0.0, 0.0]])).unwrap_err();
    assert_eq!(e, ZoneError::Overflow { index: 1 });
    assert_eq!(e.to_string(), "overflow (point 1)");
}

#[test]
fn pose_from_rigid_copies_origin_and_local_z() {
    // A quarter turn about x: local z maps to -y.
    let r = [[1.0, 0.0, 0.0], [0.0, 0.0, -1.0], [0.0, 1.0, 0.0]];
    let t = Rigid::new(r, [0.5, -0.25, 2.0]).unwrap();
    let pose = Pose::from_rigid(&t);
    assert_eq!(pose, Pose::new([0.5, -0.25, 2.0], [0.0, -1.0, 0.0]));
    // Two planes normal to -y through (., -0.25, .): a point at y = 0.5 is 0.75 away.
    let u = ZoneShape::TwoPlanes.usage(&pose, ZoneInput::Points(&[[9.0, 0.5, -4.0]])).unwrap();
    assert_eq!(u, Usage { size_m: 1.5, driving_index: 0 });
    let pose = Pose::from_rigid(&Rigid::IDENTITY);
    assert_eq!(pose, Pose::new([0.0; 3], [0.0, 0.0, 1.0]));
}

fn uniform_pm1(c: &mut Cursor) -> f64 {
    2.0 * c.uniform_f64() - 1.0
}

fn digest_hex(bytes: &[u8]) -> String {
    let d = fid_canon::digest_bytes(bytes).to_string();
    d.strip_prefix("sha256:").unwrap().to_string()
}

/// AC-5, part 1: 1 000 000 points from seed 0x5eed give the bits the Python re-implementation
/// of the pinned expressions gives (whole set and 1 000 chunks per shape); this test runs on
/// Linux, Windows, macOS and wasm32 in CI.
#[test]
fn ac5_one_million_points_bit_identical() {
    let v = fixture("shapes_random_v1.json");
    let m = obj(&v);
    assert_eq!(st(&m["seed"]), "0x5eed");
    let count = int(&m["count"]) as usize;
    let chunk = int(&m["chunk"]) as usize;
    assert_eq!((count, chunk), (1_000_000, 1_000));
    let mut c0 = Cursor::new(Address { seed: 0x5eed, stream: 0 }, 0);
    let pts: Vec<P3> = (0..count).map(|_| [uniform_pm1(&mut c0), uniform_pm1(&mut c0), uniform_pm1(&mut c0)]).collect();
    let mut c1 = Cursor::new(Address { seed: 0x5eed, stream: 1 }, 0);
    let devs: Vec<f64> = (0..count).map(|_| uniform_pm1(&mut c1)).collect();
    // The generators agree.
    for (i, p) in arr(&m["first_points"]).iter().enumerate() {
        assert_eq!(p3(p).map(f64::to_bits), pts[i].map(f64::to_bits));
    }
    for (i, d) in arr(&m["first_deviations"]).iter().enumerate() {
        assert_eq!(hf(d).to_bits(), devs[i].to_bits());
    }
    let pose = pose_of(&m["pose"]);
    let shapes = arr(&m["shapes"]);
    assert_eq!(shapes.len(), 6);
    for s in shapes {
        let s = obj(s);
        let shape = shape_of(&s["shape"]);
        let input = |a: usize, b: usize| match shape {
            ZoneShape::Offset { .. } => ZoneInput::Deviations { points: &pts[a..b], deviations: &devs[a..b] },
            _ => ZoneInput::Points(&pts[a..b]),
        };
        let u = shape.usage(&pose, input(0, count)).unwrap();
        assert_eq!(u.size_m.to_bits(), hf(&s["size"]).to_bits(), "{}", shape.name());
        assert_eq!(u64::from(u.driving_index), int(&s["index"]), "{}", shape.name());
        assert_eq!(shape.contains(&pose, u.size_m, input(0, count)), Ok(true));
        let mut bytes = Vec::with_capacity(12 * count / chunk);
        for k in (0..count).step_by(chunk) {
            let uk = shape.usage(&pose, input(k, k + chunk)).unwrap();
            bytes.extend_from_slice(&uk.size_m.to_bits().to_le_bytes());
            bytes.extend_from_slice(&uk.driving_index.to_le_bytes());
        }
        assert_eq!(digest_hex(&bytes), st(&s["chunk_sha256"]), "{}", shape.name());
    }
}

fn unit(v: P3) -> Option<P3> {
    let n = ((v[0] * v[0] + v[1] * v[1]) + v[2] * v[2]).sqrt();
    (n > 0.125).then(|| v.map(|x| x / n))
}

fn random_unit(c: &mut Cursor) -> P3 {
    loop {
        if let Some(u) = unit([uniform_pm1(c), uniform_pm1(c), uniform_pm1(c)]) {
            return u;
        }
    }
}

fn cross(a: P3, b: P3) -> P3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

/// AC-5, part 2: 10 000 seeded cases (seed 0x5eed, stream 2): the usage size is always
/// contained, the next lower f64 never is, and the driving index is the first point whose
/// own (single-point) usage equals the size, with no point exceeding it.
#[test]
fn ac5_property_contains_own_usage() {
    let mut c = Cursor::new(Address { seed: 0x5eed, stream: 2 }, 0);
    for case in 0..10_000 {
        let n = 1 + c.below(64).unwrap() as usize;
        let scale = [1e-3, 1.0, 1e3][c.below(3).unwrap() as usize];
        let origin = [uniform_pm1(&mut c), uniform_pm1(&mut c), uniform_pm1(&mut c)].map(|x| x * scale);
        let direction = random_unit(&mut c);
        let shape = match c.below(6).unwrap() {
            0 => ZoneShape::TwoPlanes,
            1 => loop {
                if let Some(plane) = unit(cross(direction, random_unit(&mut c))) {
                    break ZoneShape::TwoLines { plane };
                }
            },
            2 => ZoneShape::Cylinder,
            3 => ZoneShape::Circle { plane: random_unit(&mut c) },
            4 => ZoneShape::Sphere,
            _ => ZoneShape::Offset { disposition_shift: uniform_pm1(&mut c) * scale },
        };
        let pose = Pose::new(origin, direction);
        let pts: Vec<P3> = (0..n)
            .map(|_| [uniform_pm1(&mut c), uniform_pm1(&mut c), uniform_pm1(&mut c)].map(|x| x * scale))
            .collect();
        let devs: Vec<f64> = (0..n).map(|_| uniform_pm1(&mut c) * scale).collect();
        let input = |a: usize, b: usize| match shape {
            ZoneShape::Offset { .. } => ZoneInput::Deviations { points: &pts[a..b], deviations: &devs[a..b] },
            _ => ZoneInput::Points(&pts[a..b]),
        };
        let u = shape.usage(&pose, input(0, n)).unwrap_or_else(|e| panic!("case {case}: {e}"));
        assert!(u.size_m >= 0.0);
        assert_eq!(shape.contains(&pose, u.size_m, input(0, n)), Ok(true), "case {case}");
        assert_eq!(shape.contains(&pose, u.size_m.next_down(), input(0, n)), Ok(false), "case {case}");
        let d = u.driving_index as usize;
        for j in 0..n {
            let s = shape.usage(&pose, input(j, j + 1)).unwrap().size_m;
            assert!(s <= u.size_m, "case {case} point {j}");
            if j < d {
                assert!(s < u.size_m, "case {case}: earlier tie {j} < {d}");
            } else if j == d {
                assert_eq!(s.to_bits(), u.size_m.to_bits(), "case {case}");
            }
        }
    }
}
