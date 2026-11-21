//! Acceptance criteria of spec node KRN.LSQ.INIT (AC-1 to AC-4).
//!
//! Expected values come from `fixtures/fit/make_init_fixtures.py`: the planted geometry of
//! seeded synthetic cylinders, cones and tori (60-digit Decimal, no libm, independent of this
//! crate), including axes 0.3 m and 1 m from the origin, a near-planar ring and exact error
//! cases. `OUTPUT_PIN` is a regression pin of this crate's own output (not an oracle); CI
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
    assert_eq!(n, 28);
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
const OUTPUT_PIN: &str = "sha256:4bfb6703fd1aae02ac1ef60a109dc0a952e943ac1b492978dc607bae1365f553";

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
