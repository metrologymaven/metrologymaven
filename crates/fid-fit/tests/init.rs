//! Acceptance criteria of spec node KRN.LSQ.INIT (AC-1 to AC-4).
//!
//! Expected values come from `fixtures/fit/make_init_fixtures.py`: the planted geometry of
//! seeded synthetic cylinders, cones and tori (60-digit Decimal, no libm, independent of this
//! crate), including axes 0.3 m and 1 m from the origin, a near-planar ring, half cylinders whose
//! principal axes mix (length near 1.066 radii), fat quarter tori (minor radius 0.45 to 0.8
//! major radii) and exact error cases. `OUTPUT_PIN` is a regression pin of this crate's own output (not an oracle); CI
//! checks it on x86-64 Linux and Windows, ARM64 macOS and wasm32.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::result_large_err)]

use fid_fit::contract::{FitError, PointsView, Primitive};
use fid_fit::init::{cone_start, cylinder_start, torus_start, AxialStart, CandidateSource, DEFAULT_COUNT};
use fid_solv::mstart::generate_starts;
use fid_testkit::Sha256Hasher;
use std::sync::OnceLock;

const FIXTURE: &str = include_str!("../../../fixtures/fit/init-axial-v1/cases.txt");
const SEED: u64 = 20_261_120;

fn hexes(s: &str) -> Vec<f64> {
    s.split_whitespace().map(|t| f64::from_bits(u64::from_str_radix(t, 16).unwrap())).collect()
}

struct Case {
    name: String,
    kind: String,
    coverage: u32,
    points: Vec<f64>,
    normals: Option<Vec<f64>>,
    axis: Vec<f64>,
    point: Vec<f64>,
    size: f64,
    /// The true half-angle in radians (cone cases).
    half_angle: Option<f64>,
    /// The true minor radius (torus cases).
    minor: Option<f64>,
    expect: String,
}

fn parse() -> Vec<Case> {
    let mut out = Vec::new();
    let mut fields: Vec<(String, String)> = Vec::new();
    let mut head = String::new();
    for l in FIXTURE.lines().filter(|l| !l.starts_with('#') && !l.trim().is_empty()) {
        if let Some(rest) = l.strip_prefix("case ") {
            head = rest.to_string();
            fields.clear();
        } else if l == "end" {
            let get = |k: &str| fields.iter().find(|(key, _)| key == k).map(|(_, v)| v.clone());
            let h: Vec<&str> = head.split_whitespace().collect();
            out.push(Case {
                name: h[0].to_string(),
                kind: h[2].to_string(),
                coverage: h[4].parse().unwrap(),
                points: hexes(&get("points").unwrap()),
                normals: get("normals").map(|s| hexes(&s)),
                axis: hexes(&get("axis").unwrap()),
                point: hexes(&get("point").unwrap()),
                size: hexes(&get("size").unwrap())[0],
                half_angle: get("half_angle").map(|s| hexes(&s)[0]),
                minor: get("minor").map(|s| hexes(&s)[0]),
                expect: get("expect").unwrap(),
            });
        } else {
            let (k, v) = l.split_once(' ').unwrap_or((l, ""));
            fields.push((k.to_string(), v.to_string()));
        }
    }
    out
}

/// Every case's outcome, computed once and shared by the tests.
fn outcomes() -> &'static [(Case, Result<AxialStart, FitError>)] {
    static ALL: OnceLock<Vec<(Case, Result<AxialStart, FitError>)>> = OnceLock::new();
    ALL.get_or_init(|| {
        parse()
            .into_iter()
            .map(|c| {
                let r = run(&c);
                (c, r)
            })
            .collect()
    })
}

fn run(c: &Case) -> Result<AxialStart, FitError> {
    let pts = PointsView::new(&c.points, c.normals.as_deref()).unwrap();
    match c.kind.as_str() {
        "cyl" => cylinder_start(&pts, SEED),
        "cone" => cone_start(&pts, SEED),
        "torus" => torus_start(&pts, SEED),
        k => panic!("{k}"),
    }
}

/// The angle in degrees between two lines with unit directions `a` and `b` (test-side std math).
fn angle_deg(a: [f64; 3], b: &[f64]) -> f64 {
    let dot = (a[0] * b[0] + a[1] * b[1] + a[2] * b[2]).abs();
    dot.clamp(0.0, 1.0).acos().to_degrees()
}

/// Distance from the true axis point to the start line (test-side std math).
fn line_dist(p: [f64; 3], d: [f64; 3], q: &[f64]) -> f64 {
    let w = [q[0] - p[0], q[1] - p[1], q[2] - p[2]];
    let t = w[0] * d[0] + w[1] * d[1] + w[2] * d[2];
    let r = [w[0] - t * d[0], w[1] - t * d[1], w[2] - t * d[2]];
    (r[0] * r[0] + r[1] * r[1] + r[2] * r[2]).sqrt()
}

fn primary_angle(s: &AxialStart, c: &Case) -> f64 {
    angle_deg(s.frame.axis, &c.axis)
}

/// AC-1: primary start within 10 degrees of the true axis for coverage >= 180 degrees; for 90
/// degrees at least one start of the `StartSpec` box within 10 degrees.
#[test]
fn ac1_starts_near_the_true_axis() {
    let mut n = 0;
    for (c, r) in outcomes().iter().filter(|(c, _)| c.expect == "start" && c.normals.is_none()) {
        let s = r.as_ref().unwrap_or_else(|e| panic!("{}: {e:?}", c.name));
        let prim = match c.kind.as_str() {
            "cyl" => Primitive::Cylinder,
            "cone" => Primitive::Cone,
            _ => Primitive::Torus,
        };
        assert_eq!(s.primitive, prim);
        assert_eq!(s.spec.count, DEFAULT_COUNT);
        assert_eq!(s.spec.seed, SEED);
        let a = primary_angle(s, c);
        eprintln!("{}: primary {a:.4} degrees", c.name);
        if c.coverage >= 180 {
            assert!(a <= 10.0, "{}: primary axis {a:.3} degrees off", c.name);
            // The anchored axis passes near the true axis (within the size of the feature).
            let dist = line_dist(s.frame.point, s.frame.axis, &c.point);
            assert!(dist <= c.size, "{}: axis {dist:e} m from the true axis", c.name);
        } else {
            let best = generate_starts(&s.spec)
                .unwrap()
                .iter()
                .map(|p| angle_deg(s.frame.line(p).unwrap().1, &c.axis))
                .fold(f64::INFINITY, f64::min);
            assert!(best <= 10.0, "{}: best start {best:.3} degrees off (primary {a:.3})", c.name);
        }
        n += 1;
    }
    assert_eq!(n, 53);
}

/// Accuracy tolerances of the primary start against the planted truth, asserted for every
/// start case of the fixture (90-degree coverage included). Each is between about 2 and 10 times
/// the worst error on the fixture (printed by `primary_start_accuracy`), so it catches a
/// regression without being tuned to the bit.
/// Axis angle, degrees (the primary's direction against the true axis; worst 0.30).
const TOL_AXIS_DEG: f64 = 1.0;
/// Cylinder radius and cone `r_ref`, relative to the true value (worst 1.9e-4 and 4.7e-4).
const TOL_RADIUS_REL: f64 = 2e-3;
/// Distance of the true axis point from the start axis line, relative to the radius (`size`;
/// worst 1.2e-3).
const TOL_POSITION_REL: f64 = 5e-3;
/// Cone half-angle, degrees (worst 0.13).
const TOL_HALF_ANGLE_DEG: f64 = 0.5;
/// Cone apex distance from the true apex, relative to the slant length from the apex to the
/// planted point (`size / sin(half_angle)`): the apex is an extrapolation of the radius line
/// (worst 2.9e-3).
const TOL_APEX_REL: f64 = 1e-2;
/// Torus major radius `R` and minor radius `r`, relative to the true values (worst 3.3e-4 and
/// 1.1e-4).
const TOL_TORUS_REL: f64 = 1e-3;
/// Distance of the start torus centre (the frame point) from the true centre, relative to the
/// major radius (worst 4.6e-3).
const TOL_CENTRE_REL: f64 = 2e-2;

fn norm3(v: [f64; 3]) -> f64 {
    (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt()
}

fn dot3(a: [f64; 3], b: &[f64]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// The relative and angular errors of one start against the truth, by name (test-side std math).
fn errors(s: &AxialStart, c: &Case) -> Vec<(&'static str, f64, f64)> {
    let mut e = vec![("axis_deg", primary_angle(s, c), TOL_AXIS_DEG)];
    let p = &s.spec.primary;
    match c.kind.as_str() {
        "cyl" => {
            e.push(("radius_rel", (p[4] - c.size).abs() / c.size, TOL_RADIUS_REL));
            let dist = line_dist(s.frame.point, s.frame.axis, &c.point);
            e.push(("position_rel", dist / c.size, TOL_POSITION_REL));
        }
        "cone" => {
            let alpha = c.half_angle.unwrap();
            // The start axis points from the apex into the cone, as the true one does.
            assert!(dot3(s.frame.axis, &c.axis) > 0.0, "{}: cone axis reversed", c.name);
            e.push(("half_angle_deg", (p[5] - alpha).abs().to_degrees(), TOL_HALF_ANGLE_DEG));
            // True radius at the frame point's height on the true axis.
            let w = [s.frame.point[0] - c.point[0], s.frame.point[1] - c.point[1], s.frame.point[2] - c.point[2]];
            let r_true = c.size + dot3(w, &c.axis) * alpha.tan();
            e.push(("r_ref_rel", (p[4] - r_true).abs() / r_true, TOL_RADIUS_REL));
            let dist = line_dist(s.frame.point, s.frame.axis, &c.point);
            e.push(("position_rel", dist / c.size, TOL_POSITION_REL));
            let back = c.size / alpha.tan();
            let apex_true =
                [c.point[0] - back * c.axis[0], c.point[1] - back * c.axis[1], c.point[2] - back * c.axis[2]];
            let a = s.apex.unwrap();
            let slant = c.size / alpha.sin();
            let da = norm3([a[0] - apex_true[0], a[1] - apex_true[1], a[2] - apex_true[2]]);
            e.push(("apex_rel", da / slant, TOL_APEX_REL));
        }
        _ => {
            let minor = c.minor.unwrap();
            e.push(("major_rel", (p[4] - c.size).abs() / c.size, TOL_TORUS_REL));
            e.push(("minor_rel", (p[5] - minor).abs() / minor, TOL_TORUS_REL));
            let w = [s.frame.point[0] - c.point[0], s.frame.point[1] - c.point[1], s.frame.point[2] - c.point[2]];
            e.push(("centre_rel", norm3(w) / c.size, TOL_CENTRE_REL));
        }
    }
    e
}

/// The accuracy claim: every start case's primary axis within [`TOL_AXIS_DEG`], and its size and
/// position parameters within the stated tolerances of the planted truth.
#[test]
fn primary_start_accuracy() {
    let mut worst: Vec<(&'static str, f64, String)> = Vec::new();
    let mut n = 0;
    for (c, r) in outcomes().iter().filter(|(c, _)| c.expect == "start") {
        let s = r.as_ref().unwrap_or_else(|e| panic!("{}: {e:?}", c.name));
        for (what, err, tol) in errors(s, c) {
            assert!(err <= tol, "{}: {what} {err:.3e} above {tol:e}", c.name);
            match worst.iter_mut().find(|w| w.0 == what) {
                Some(w) if err > w.1 => *w = (what, err, c.name.clone()),
                Some(_) => {}
                None => worst.push((what, err, c.name.clone())),
            }
        }
        n += 1;
    }
    for (what, err, name) in &worst {
        eprintln!("worst {what}: {err:.3e} ({name})");
    }
    assert_eq!(n, 56);
}

/// Regression (review of #358): half cylinders of length near 1.066 radii, where the principal
/// axes mix. Refining only the best fixed candidate converged about 90 degrees off on some of
/// them; refining the best three keeps every one within [`TOL_AXIS_DEG`].
#[test]
fn half_cylinders_with_mixed_principal_axes() {
    let mut n = 0;
    for (c, r) in outcomes().iter().filter(|(c, _)| c.name.starts_with("cyl-half-")) {
        assert_eq!(c.coverage, 180);
        let a = primary_angle(r.as_ref().unwrap(), c);
        assert!(a <= TOL_AXIS_DEG, "{}: primary axis {a:.3} degrees off", c.name);
        n += 1;
    }
    assert_eq!(n, 19);
}

/// Regression (review of #358): fat quarter tori (minor radius 0.45, 0.5 and 0.8 major radii,
/// 90 degrees of the major angle) were 14 to 89 degrees off before the mirrored torus starts.
#[test]
fn fat_quarter_tori() {
    let mut n = 0;
    for (c, r) in outcomes().iter().filter(|(c, _)| c.name.starts_with("torus-fat-")) {
        assert_eq!(c.coverage, 90);
        let s = r.as_ref().unwrap();
        assert_eq!(s.candidates.len(), 16);
        for (what, err, tol) in errors(s, c) {
            assert!(err <= tol, "{}: {what} {err:.3e} above {tol:e}", c.name);
        }
        n += 1;
    }
    assert_eq!(n, 6);
}

/// AC-3: with normals, the normal-derived direction is the first candidate and the primary axis
/// is within 1 degree.
#[test]
fn ac3_normals_lead_and_give_the_axis() {
    let cases: Vec<Case> = parse().into_iter().filter(|c| c.normals.is_some()).collect();
    assert_eq!(cases.len(), 3);
    for c in &cases {
        let s = run(c).unwrap();
        assert_eq!(s.candidates[0].source, CandidateSource::Normals, "{}", c.name);
        assert_eq!(s.candidates.len(), 17);
        let a = primary_angle(&s, c);
        assert!(a < 1.0, "{}: {a:.4} degrees", c.name);
    }
}

fn encode_err(e: &FitError) -> String {
    match e {
        FitError::TooFewPoints { n, need } => format!("too_few_points n={n} need={need}"),
        FitError::Coplanar { n, rank, .. } => format!("coplanar n={n} rank={rank}"),
        FitError::Collinear { n, rank, .. } => format!("collinear n={n} rank={rank}"),
        e => format!("{e:?}"),
    }
}

/// AC-4 and the other error paths: the typed `FitError` with the oracle's code and fields.
#[test]
fn ac4_error_paths() {
    let mut n = 0;
    for c in parse().iter().filter(|c| c.expect != "start") {
        let got = run(c).map(|_| ()).unwrap_err();
        assert_eq!(encode_err(&got), c.expect, "{}", c.name);
        n += 1;
    }
    assert_eq!(n, 3);
    // AC-4 exactly as the spec states it.
    let c = parse().into_iter().find(|c| c.name == "few-4").unwrap();
    assert_eq!(run(&c).unwrap_err(), FitError::TooFewPoints { n: 4, need: 5 });
    // Cone and torus need 6 and 7 points.
    let pts = PointsView::new(&c.points, None).unwrap();
    assert_eq!(cone_start(&pts, 1).unwrap_err(), FitError::TooFewPoints { n: 4, need: 6 });
    assert_eq!(torus_start(&pts, 1).unwrap_err(), FitError::TooFewPoints { n: 4, need: 7 });
}

/// Same input and seed: bit-identical output (D0), and the seed only changes the seed field.
#[test]
fn deterministic_and_seeded() {
    let cases = parse();
    let c = &cases[0];
    let a = run(c).unwrap();
    let b = run(c).unwrap();
    assert_eq!(a, b);
    let pts = PointsView::new(&c.points, None).unwrap();
    let other = cylinder_start(&pts, 7).unwrap();
    assert_eq!(other.spec.seed, 7);
    assert_eq!(other.frame, a.frame);
    assert_eq!(other.spec.primary, a.spec.primary);
}

fn put(h: &mut Sha256Hasher, xs: &[f64]) {
    for x in xs {
        h.update(&x.to_bits().to_le_bytes());
    }
}

/// AC-2: output-bit pin of every case (every `StartSpec`, frame, candidate list and selection,
/// and every error as code and fields), recorded from a local x86-64 run and checked by CI on
/// x86-64 Linux and Windows, ARM64 macOS and wasm32 under wasmtime.
///
/// Re-recorded for the review of #358 (the first value was
/// `sha256:4bfb6703fd1aae02ac1ef60a109dc0a952e943ac1b492978dc607bae1365f553`): the procedure
/// changed (the best three candidates are refined for every primitive and the torus score has
/// mirrored starts, so torus candidate residuals and some selections change) and the fixture
/// gained 25 cases (half cylinders near 1.066 radii and fat quarter tori) and the cone
/// `half_angle` and torus `minor` lines.
const OUTPUT_PIN: &str = "sha256:771700ca0139c6d36f32dd8499cfa7da2f07038912110fffc2098bfa322b44ce";

#[test]
fn ac2_output_bits_pinned() {
    let mut h = Sha256Hasher::new();
    for (c, r) in outcomes() {
        h.update(c.name.as_bytes());
        match r {
            Ok(s) => {
                let sp = &s.spec;
                put(&mut h, &sp.primary);
                put(&mut h, &sp.half_widths);
                put(&mut h, &sp.tolerances);
                h.update(&sp.count.to_le_bytes());
                h.update(&sp.seed.to_le_bytes());
                h.update(&sp.stream.to_le_bytes());
                put(&mut h, &s.frame.point);
                put(&mut h, &s.frame.axis);
                put(&mut h, &s.frame.e1);
                put(&mut h, &s.frame.e2);
                put(&mut h, &[s.rms_radius]);
                if let Some(a) = s.apex {
                    put(&mut h, &a);
                }
                h.update(&(s.selected as u64).to_le_bytes());
                for k in &s.candidates {
                    h.update(format!("{:?}", k.source).as_bytes());
                    put(&mut h, &k.direction);
                    put(&mut h, &[k.residual.unwrap_or(f64::NAN)]);
                }
                for st in generate_starts(sp).unwrap() {
                    put(&mut h, &st);
                }
            }
            Err(e) => h.update(encode_err(e).as_bytes()),
        }
    }
    assert_eq!(h.finish(), OUTPUT_PIN, "output bits changed on {}", std::env::consts::ARCH);
}
