//! Acceptance criteria of spec node GEO.FRAME.XFORM (AC-1 to AC-5) plus requirement checks.
//! Expected bits and canonical angles come from `fixtures/geo/frame/xform_cases.json` and
//! `euler_cases.json`, written by the independent Python reference
//! `tools/fixtures/geo_xform_cases.py` (stdlib; bit-exact float model of the pinned
//! expressions plus an exact `fractions` check); none come from this crate. The AC-3 and AC-5
//! inputs below are hand-labelled from the spec.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_geo_frame::rotation::{AxisAngle, Euler, EulerKind, EulerSeq, Quaternion};
use fid_geo_frame::xform::{check_rotation, Matrix3, Rigid, Similarity, Transform, XformError, IDENTITY};
use fid_num::rng::{Address, Cursor};

const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/geo/frame/");

fn cases(file: &str) -> Vec<String> {
    let text = std::fs::read_to_string(format!("{ROOT}{file}")).unwrap();
    text.lines().map(str::trim).filter(|l| l.starts_with("{\"")).map(str::to_owned).collect()
}

/// The string value of `"key":"..."` (values never contain quotes).
fn field<'a>(line: &'a str, key: &str) -> &'a str {
    let pat = format!("\"{key}\":");
    let start = line.find(&pat).unwrap_or_else(|| panic!("{key} missing")) + pat.len();
    let rest = &line[start..];
    if let Some(r) = rest.strip_prefix('"') {
        &r[..r.find('"').unwrap()]
    } else {
        &rest[..rest.find([',', '}']).unwrap()]
    }
}

fn hexes(s: &str) -> Vec<f64> {
    s.split(' ').map(|h| f64::from_bits(u64::from_str_radix(h, 16).unwrap())).collect()
}

fn bits(xs: impl IntoIterator<Item = f64>) -> Vec<u64> {
    xs.into_iter().map(f64::to_bits).collect()
}

fn matrix(v: &[f64]) -> Matrix3 {
    [[v[0], v[1], v[2]], [v[3], v[4], v[5]], [v[6], v[7], v[8]]]
}

fn transform(line: &str, prefix: &str) -> Transform {
    let r = matrix(&hexes(field(line, &format!("{prefix}r"))));
    let t = hexes(field(line, &format!("{prefix}t")));
    let rigid = Rigid::new(r, [t[0], t[1], t[2]]).unwrap();
    match field(line, &format!("{prefix}kind")) {
        "rigid" => Transform::Rigid(rigid),
        "similarity" => {
            Transform::Similarity(Similarity::new(rigid, hexes(field(line, &format!("{prefix}s")))[0]).unwrap())
        }
        k => panic!("kind {k}"),
    }
}

fn flat(m: &Matrix3) -> Vec<f64> {
    m.iter().flatten().copied().collect()
}

/// Bit-level comparison of a result with the fixture's `<prefix>kind/r/t/s`.
fn assert_bits(name: &str, what: &str, got: &Transform, line: &str, prefix: &str) {
    let kind = if got.is_rigid() { "rigid" } else { "similarity" };
    assert_eq!(kind, field(line, &format!("{prefix}kind")), "{name} {what} kind");
    assert_eq!(bits(flat(&got.rotation())), bits(hexes(field(line, &format!("{prefix}r")))), "{name} {what} r");
    assert_eq!(bits(got.translation()), bits(hexes(field(line, &format!("{prefix}t")))), "{name} {what} t");
    assert_eq!(bits([got.scale()]), bits(hexes(field(line, &format!("{prefix}s")))), "{name} {what} s");
}

fn transforms() -> Vec<(String, Transform, String)> {
    cases("xform_cases.json").into_iter().map(|l| (field(&l, "name").to_owned(), transform(&l, ""), l)).collect()
}

/// AC-1: composed and inverted matrices bit for bit (run natively on x86-64 and ARM64 by the
/// Linux/Windows/macOS jobs and on wasm32-wasip1 by the wasm32 job).
#[test]
fn ac1_compose_and_inverse_bit_exact() {
    let ts = transforms();
    assert_eq!(ts.len(), 200);
    for (i, (name, a, line)) in ts.iter().enumerate() {
        let b = &ts[(i + 1) % ts.len()].1;
        assert_bits(name, "compose", &a.compose(b).unwrap(), line, "c_");
        assert_bits(name, "inverse", &a.inverse().unwrap(), line, "i_");
    }
}

/// AC-2: 10^6 seeded points in the 100 m cube, each mapped forward and back through one of
/// the 200 transforms (point k uses transform k mod 200), stay within 1e-12 m.
#[test]
fn ac2_forward_and_back_within_1e12() {
    let ts: Vec<(Transform, Transform)> = transforms().into_iter().map(|(_, t, _)| (t, t.inverse().unwrap())).collect();
    let mut cur = Cursor::new(Address { seed: 11, stream: 2 }, 0);
    let mut worst2 = 0.0f64;
    for k in 0..1_000_000usize {
        let (fwd, back) = &ts[k % ts.len()];
        let x = [0, 1, 2].map(|_| cur.uniform_f64() * 200.0 - 100.0);
        let y = back.apply_point(&fwd.apply_point(&x).unwrap()).unwrap();
        let d2 = (0..3).map(|i| (y[i] - x[i]) * (y[i] - x[i])).sum::<f64>();
        if d2 > worst2 {
            worst2 = d2;
        }
    }
    assert!(worst2 <= 1e-24, "largest error {:e} m", worst2.sqrt());
}

/// AC-3: one entry perturbed by 1e-9 is NotOrthonormal; a reflection is Improper.
#[test]
fn ac3_perturbed_and_reflection_rejected() {
    let mut m = IDENTITY;
    m[1][2] = 1e-9;
    match Rigid::new(m, [0.0; 3]) {
        Err(XformError::NotOrthonormal { residual }) => assert!(residual > 1e-12 && residual < 1e-8),
        other => panic!("{other:?}"),
    }
    let reflection = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, -1.0]];
    assert_eq!(Rigid::new(reflection, [0.0; 3]), Err(XformError::Improper { det: -1.0 }));
    assert_eq!(check_rotation(&[[1.0, 0.0, 0.0], [0.0, f64::NAN, 0.0], [0.0, 0.0, 1.0]]), Err(XformError::NonFinite));
    assert_eq!(Rigid::new(IDENTITY, [0.0, f64::INFINITY, 0.0]), Err(XformError::NonFinite));
}

/// REQ-002: a similarity scale must be finite and strictly positive.
#[test]
fn req2_bad_scale() {
    for s in [0.0, -0.0, -1.0, f64::NAN, f64::INFINITY] {
        assert_eq!(Similarity::new(Rigid::IDENTITY, s), Err(XformError::BadScale), "{s}");
    }
    let tiny = Transform::Similarity(Similarity::new(Rigid::IDENTITY, f64::from_bits(1)).unwrap());
    assert_eq!(tiny.inverse(), Err(XformError::BadScale), "1 / min subnormal overflows");
}

fn geodesic(a: &Matrix3, b: &Matrix3) -> f64 {
    // ||A - B||_F = 2 sqrt(2) sin(theta / 2), so theta ~ ||A - B||_F / sqrt(2) for small theta.
    let f2: f64 = (0..3).flat_map(|i| (0..3).map(move |j| (i, j))).map(|(i, j)| (a[i][j] - b[i][j]).powi(2)).sum();
    (f2 / 2.0).sqrt()
}

/// AC-4: every Euler variant round-trips within 1e-14 rad; gimbal-lock cases return the
/// canonical angles of the fixture (third angle 0).
#[test]
fn ac4_euler_round_trips_and_canonical_lock() {
    let all = cases("euler_cases.json");
    assert_eq!(all.len(), 24 * 7);
    let mut variants = std::collections::BTreeSet::new();
    let mut locks = 0;
    for line in &all {
        let name = field(line, "name");
        let seq = EulerSeq::from_name(field(line, "seq")).unwrap();
        let kind = match field(line, "kind") {
            "intrinsic" => EulerKind::Intrinsic,
            "extrinsic" => EulerKind::Extrinsic,
            k => panic!("{k}"),
        };
        variants.insert((seq, kind));
        let a = hexes(field(line, "angles"));
        let m = Euler { seq, kind, angles: [a[0], a[1], a[2]] }.to_matrix().unwrap();
        let back = Euler::from_matrix(&m, seq, kind).unwrap();
        let m2 = back.to_matrix().unwrap();
        let err = geodesic(&m, &m2);
        assert!(err <= 1e-14, "{name}: round trip {err:e} rad");
        if field(line, "check_angles") == "true" {
            let want = hexes(field(line, "expect"));
            for (g, w) in back.angles.iter().zip(&want) {
                assert!((g - w).abs() <= 1e-12, "{name}: got {:?} want {want:?}", back.angles);
            }
        }
        if field(line, "lock") == "true" {
            locks += 1;
            assert_eq!(back.angles[2].to_bits(), 0, "{name}: canonical third angle is +0");
            let want = hexes(field(line, "expect"));
            for (g, w) in back.angles.iter().zip(&want) {
                assert!((g - w).abs() <= 1e-14, "{name}: got {:?} want {want:?}", back.angles);
            }
        }
    }
    assert_eq!(variants.len(), 24);
    assert_eq!(locks, 48);
}

/// AC-5: a quaternion whose norm is off by more than 1e-12 fails with NotUnit.
///
/// The leaf's example (1, 1e-9, 0, 0) has norm sqrt(1 + 1e-18), within 5e-19 of 1, and
/// `1 + 1e-18` rounds to exactly 1 in binary64; REQ-006's tolerance accepts it. The NODE.md
/// AC-5 now uses (1, 1e-5, 0, 0), norm − 1 ≈ 5e-11 (see the PR).
#[test]
fn ac5_non_unit_quaternion_rejected() {
    let q = Quaternion { w: 1.0, x: 1e-5, y: 0.0, z: 0.0 };
    assert!(matches!(q.to_matrix(), Err(XformError::NotUnit { .. })));
    let q = Quaternion { w: 1.000_000_001, x: 0.0, y: 0.0, z: 0.0 };
    assert!(matches!(q.to_matrix(), Err(XformError::NotUnit { .. })));
    let q = Quaternion { w: 1.0, x: 1e-9, y: 0.0, z: 0.0 };
    assert!(q.to_matrix().is_ok(), "within the 1e-12 tolerance (see above)");
    let q = Quaternion { w: f64::NAN, x: 0.0, y: 0.0, z: 0.0 };
    assert_eq!(q.to_matrix(), Err(XformError::NonFinite));
}

/// REQ-005: quaternion and axis-angle round trips through the matrix on all 200 rotations.
#[test]
fn req5_quaternion_and_axis_angle_round_trips() {
    for (name, t, _) in transforms() {
        let m = t.rotation();
        let q = Quaternion::from_matrix(&m).unwrap();
        assert!(q.w >= 0.0, "{name}");
        let mq = q.to_matrix().unwrap();
        assert!(geodesic(&m, &mq) <= 1e-14, "{name}: quaternion {:e}", geodesic(&m, &mq));
        let aa = AxisAngle::from_matrix(&m).unwrap();
        assert!((0.0..=core::f64::consts::PI).contains(&aa.angle), "{name}");
        let ma = aa.to_matrix().unwrap();
        assert!(geodesic(&m, &ma) <= 1e-14, "{name}: axis-angle {:e}", geodesic(&m, &ma));
    }
    assert_eq!(AxisAngle { axis: [0.0; 3], angle: 1.0 }.to_matrix(), Err(XformError::ZeroAxis));
    let id = AxisAngle::from_matrix(&IDENTITY).unwrap();
    assert_eq!((id.axis, id.angle), ([1.0, 0.0, 0.0], 0.0));
    // A half turn about each axis picks a Shepperd branch other than the trace.
    for axis in [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0], [1.0, 1.0, 1.0]] {
        let m = AxisAngle { axis, angle: core::f64::consts::PI }.to_matrix().unwrap();
        let back = AxisAngle::from_matrix(&m).unwrap().to_matrix().unwrap();
        assert!(geodesic(&m, &back) <= 1e-14);
    }
}

/// Points and directions: rigid application, direction ignores translation and scale.
#[test]
fn apply_point_and_direction() {
    let rz90 = [[0.0, -1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]];
    let t = Transform::Similarity(Similarity::new(Rigid::new(rz90, [1.0, 2.0, 3.0]).unwrap(), 2.0).unwrap());
    assert_eq!(t.apply_point(&[1.0, 0.0, 0.0]).unwrap(), [1.0, 4.0, 3.0]);
    assert_eq!(t.apply_dir(&[1.0, 0.0, 0.0]).unwrap(), [0.0, 1.0, 0.0]);
    let unit_similarity = Transform::Similarity(Similarity::new(Rigid::IDENTITY, 1.0).unwrap());
    assert!(t.compose(&t.inverse().unwrap()).unwrap().approx_eq(&unit_similarity, 1e-15));
    assert!(!Transform::IDENTITY.approx_eq(&t, 1.0));
    let huge = Transform::Rigid(Rigid::new(IDENTITY, [f64::MAX, 0.0, 0.0]).unwrap());
    assert_eq!(huge.apply_point(&[f64::MAX, 0.0, 0.0]), Err(XformError::NonFinite));
    assert_eq!(huge.compose(&huge), Err(XformError::NonFinite));
}
