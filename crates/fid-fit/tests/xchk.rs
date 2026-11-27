//! Acceptance criteria of `KRN.ENV.XCHK`: AC-1 (Welzl against the exact brute-force and
//! incremental oracles of `fixtures/fit/make_xchk_fixtures.py`, bit-identical across seeds),
//! AC-2 (Voronoi maximum inscribed circle against the grid-refinement oracle, boundary flag),
//! AC-3 (output-bit pin, checked by CI on x86-64 Linux and Windows, ARM64 macOS and wasm32
//! under wasmtime) and AC-4 (error paths), plus the spec cards. Expected values come only from
//! the Python oracles; `OUTPUT_PIN` is a regression pin of this crate's own output, recorded
//! from a local x86-64 run (not an oracle).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::result_large_err)]

use fid_fit::contract::FitError;
use fid_fit::xchk::{
    algorithm_ids, hull_region, intersect_convex, mcc_circle2d_id, mcc_circle2d_welzl, mcc_sphere_id, mcc_sphere_welzl,
    mic_circle2d_id, mic_circle2d_vor, region_tol_floor, Enclosing, Inscribed, MCC_CIRCLE2D_ID, MCC_SPHERE_ID,
    MIC_CIRCLE2D_ID,
};
use fid_kci::{check, parse_card};
use fid_testkit::Sha256Hasher;

const MCC_TXT: &str = include_str!("../../../fixtures/fit/xchk-mcc-v1/cases.txt");
const MIC_TXT: &str = include_str!("../../../fixtures/fit/xchk-mic-v1/cases.txt");
const CARDS: [&str; 3] = [
    include_str!("../cards/fit.circle2d.mcc_welzl@1.0.0.toml"),
    include_str!("../cards/fit.sphere.mcc_welzl@1.0.0.toml"),
    include_str!("../cards/fit.circle2d.mic_vor@1.0.0.toml"),
];

/// One coordinate unit of xchk-mcc-v1: `2^-24` m.
const UNIT: f64 = 1.0 / (1u64 << 24) as f64;
/// AC-1 radius tolerance (metres).
const MCC_TOL: f64 = 1e-12;
/// AC-2 radius and centre tolerance (metres).
const MIC_TOL: f64 = 1e-9;

fn hex(s: &str) -> f64 {
    f64::from_bits(u64::from_str_radix(s, 16).expect("hex f64"))
}

struct SplitMix64(u64);

impl SplitMix64 {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
}

/// The fixture's `gen` rule (documented in the fixture header).
fn gen_points(dim: usize, count: usize, seed: u64, rad: i64, centre: &[i64]) -> Vec<Vec<i64>> {
    let mut rng = SplitMix64(seed);
    let span = (2 * rad + 1) as u64;
    let mut out = Vec::with_capacity(count);
    while out.len() < count {
        let v: Vec<i64> = (0..dim).map(|_| (rng.next() % span) as i64 - rad).collect();
        let n2: i128 = v.iter().map(|&x| i128::from(x) * i128::from(x)).sum();
        if n2 <= i128::from(rad) * i128::from(rad) {
            out.push(v.iter().zip(centre).map(|(x, c)| x + c).collect());
        }
    }
    out
}

struct MccCase {
    id: String,
    dim: usize,
    oracle: String,
    pts: Vec<Vec<f64>>,
    r: f64,
    centre: Vec<f64>,
    on: Vec<u32>,
}

fn load_mcc() -> Vec<MccCase> {
    let mut out = Vec::new();
    let mut cur: Option<MccCase> = None;
    for line in MCC_TXT.lines() {
        if line.starts_with('#') || line.trim().is_empty() {
            continue;
        }
        let t: Vec<&str> = line.split_whitespace().collect();
        match t[0] {
            "case" => {
                let dim = if t[2] == "circle" { 2 } else { 3 };
                cur = Some(MccCase {
                    id: t[1].to_string(),
                    dim,
                    oracle: t[4].to_string(),
                    pts: Vec::new(),
                    r: 0.0,
                    centre: Vec::new(),
                    on: Vec::new(),
                });
            }
            "pts" => {
                let c = cur.as_mut().unwrap();
                let n: usize = t[1].parse().unwrap();
                assert_eq!(t.len(), 2 + n * c.dim, "{}", c.id);
                for k in 0..n {
                    let p = (0..c.dim).map(|j| t[2 + k * c.dim + j].parse::<i64>().unwrap() as f64 * UNIT).collect();
                    c.pts.push(p);
                }
            }
            "gen" => {
                let c = cur.as_mut().unwrap();
                let count: usize = t[1].parse().unwrap();
                let seed: u64 = t[2].parse().unwrap();
                let rad: i64 = t[3].parse().unwrap();
                let centre: Vec<i64> = t[4..].iter().map(|s| s.parse().unwrap()).collect();
                assert_eq!(centre.len(), c.dim);
                for p in gen_points(c.dim, count, seed, rad, &centre) {
                    c.pts.push(p.iter().map(|&x| x as f64 * UNIT).collect());
                }
            }
            "expect" => {
                let c = cur.as_mut().unwrap();
                c.r = hex(t[3]);
                c.centre = (0..c.dim).map(|j| hex(t[4 + j])).collect();
                assert_eq!(t[4 + c.dim], "on");
                let k: usize = t[5 + c.dim].parse().unwrap();
                c.on = t[6 + c.dim..].iter().map(|s| s.parse().unwrap()).collect();
                assert_eq!(c.on.len(), k, "{}", c.id);
            }
            "end" => out.push(cur.take().unwrap()),
            other => panic!("unknown line {other}"),
        }
    }
    assert_eq!(out.len(), 1000, "AC-1 requires 1 000 seeded point sets");
    out
}

fn run_mcc(c: &MccCase, seed: u64) -> Result<Enclosing, FitError> {
    if c.dim == 2 {
        let xy: Vec<[f64; 2]> = c.pts.iter().map(|p| [p[0], p[1]]).collect();
        mcc_circle2d_welzl(&xy, seed)
    } else {
        let xyz: Vec<[f64; 3]> = c.pts.iter().map(|p| [p[0], p[1], p[2]]).collect();
        mcc_sphere_welzl(&xyz, seed)
    }
}

fn same_bits(a: &Enclosing, b: &Enclosing) -> bool {
    a.radius.to_bits() == b.radius.to_bits()
        && a.centre.iter().zip(&b.centre).all(|(x, y)| x.to_bits() == y.to_bits())
        && a.support == b.support
}

#[test]
fn ac1_welzl_matches_exact_oracles_and_is_seed_independent() {
    let cases = load_mcc();
    let mut sizes = (usize::MAX, 0usize);
    let mut brute = 0usize;
    let mut cocircular = 0usize;
    for c in &cases {
        sizes = (sizes.0.min(c.pts.len()), sizes.1.max(c.pts.len()));
        if c.oracle == "brute" {
            assert!(c.pts.len() <= 30, "{}", c.id);
            brute += 1;
        }
        if c.on.len() > c.dim + 1 {
            cocircular += 1;
        }
        let r1 = run_mcc(c, 1).unwrap_or_else(|e| panic!("{}: {e}", c.id));
        let r2 = run_mcc(c, 2).unwrap();
        let r3 = run_mcc(c, 3).unwrap();
        assert!(same_bits(&r1, &r2) && same_bits(&r1, &r3), "{}: seeds 1, 2, 3 disagree", c.id);
        assert_eq!(r1.algorithm, if c.dim == 2 { mcc_circle2d_id() } else { mcc_sphere_id() });
        assert!((r1.radius - c.r).abs() <= MCC_TOL, "{}: radius {} vs oracle {}", c.id, r1.radius, c.r);
        for (j, (x, y)) in r1.centre.iter().zip(&c.centre).enumerate() {
            assert!((x - y).abs() <= 1e-10, "{}: centre[{j}] {x} vs oracle {y}", c.id);
        }
        if c.dim == 2 {
            assert_eq!(r1.centre[2].to_bits(), 0, "{}: circle z", c.id);
        }
        // REQ-002: 2 to dim + 1 support points, every one exactly on the oracle's ball.
        assert!((2..=c.dim + 1).contains(&r1.support.len()), "{}: support {:?}", c.id, r1.support);
        for s in &r1.support {
            assert!(c.on.contains(s), "{}: support point {s} is not on the oracle's ball", c.id);
        }
        // Enclosure (independent of the oracle): every point within the radius.
        for p in &c.pts {
            let d2: f64 = p.iter().zip(&r1.centre).map(|(a, b)| (a - b) * (a - b)).sum();
            assert!(d2.sqrt() <= r1.radius + MCC_TOL, "{}: a point lies outside", c.id);
        }
    }
    assert!(sizes.0 == 3 && sizes.1 <= 5000 && sizes.1 > 4000, "sizes {sizes:?}");
    assert!(brute >= 700, "brute-force cases {brute}");
    assert!(cocircular >= 100, "cocircular and cospherical cases {cocircular}");
}

struct MicCase {
    id: String,
    coverage: u32,
    pts: Vec<[f64; 2]>,
    clip: Option<Vec<[f64; 2]>>,
    r: f64,
    centre: [f64; 2],
    on_boundary: bool,
}

fn hex_pairs(t: &[&str]) -> Vec<[f64; 2]> {
    let n: usize = t[0].parse().unwrap();
    assert_eq!(t.len(), 1 + 2 * n);
    (0..n).map(|k| [hex(t[1 + 2 * k]), hex(t[2 + 2 * k])]).collect()
}

fn load_mic() -> Vec<MicCase> {
    let mut out = Vec::new();
    let mut cur: Option<MicCase> = None;
    for line in MIC_TXT.lines() {
        if line.starts_with('#') || line.trim().is_empty() {
            continue;
        }
        let t: Vec<&str> = line.split_whitespace().collect();
        match t[0] {
            "case" => {
                cur = Some(MicCase {
                    id: t[1].to_string(),
                    coverage: t[2].parse().unwrap(),
                    pts: Vec::new(),
                    clip: None,
                    r: 0.0,
                    centre: [0.0; 2],
                    on_boundary: false,
                });
                assert!(t[3] == "hull" || t[3] == "hull_clip");
            }
            "pts" => cur.as_mut().unwrap().pts = hex_pairs(&t[1..]),
            "clip" => cur.as_mut().unwrap().clip = Some(hex_pairs(&t[1..])),
            "expect" => {
                let c = cur.as_mut().unwrap();
                c.r = hex(t[1]);
                c.centre = [hex(t[2]), hex(t[3])];
                c.on_boundary = t[4] == "1";
            }
            "end" => out.push(cur.take().unwrap()),
            other => panic!("unknown line {other}"),
        }
    }
    assert_eq!(out.len(), 24);
    out
}

fn run_mic(c: &MicCase) -> Result<Inscribed, FitError> {
    let hull = hull_region(&c.pts)?;
    let region = match &c.clip {
        Some(sq) => intersect_convex(&hull, sq)?,
        None => hull,
    };
    mic_circle2d_vor(&c.pts, &region, 1e-12)
}

#[test]
fn ac2_voronoi_mic_matches_grid_oracle_and_flags_the_boundary() {
    let cases = load_mic();
    let mut partial = 0;
    for c in &cases {
        let r = run_mic(c).unwrap_or_else(|e| panic!("{}: {e}", c.id));
        assert_eq!(r.algorithm, mic_circle2d_id());
        assert!((r.radius - c.r).abs() <= MIC_TOL, "{}: radius {} vs oracle {}", c.id, r.radius, c.r);
        assert!(r.radius >= c.r - 1e-15, "{}: below a feasible oracle sample", c.id);
        assert_eq!(r.on_region_boundary, c.on_boundary, "{}: boundary flag ({} m)", c.id, r.boundary_distance_m);
        if c.coverage == 360 && c.clip.is_none() {
            assert!(!r.on_region_boundary, "{}", c.id);
            for (j, (x, y)) in r.centre.iter().zip(&c.centre).enumerate() {
                assert!((x - y).abs() <= MIC_TOL, "{}: centre[{j}]", c.id);
            }
        }
        if c.coverage == 90 {
            partial += 1;
            assert!(c.on_boundary && r.on_region_boundary, "{}: a 90-degree arc must report on_region_boundary", c.id);
        }
        assert!(!r.contacts.is_empty(), "{}", c.id);
        for &i in &r.contacts {
            let p = c.pts[i as usize];
            let d = ((p[0] - r.centre[0]).powi(2) + (p[1] - r.centre[1]).powi(2)).sqrt();
            assert!((d - r.radius).abs() <= 1e-12, "{}: contact {i}", c.id);
        }
    }
    assert_eq!(partial, 7);
}

/// AC-3: output-bit pin of every AC-1 (seed 1) and AC-2 result, recorded from a local x86-64
/// run and checked by CI on x86-64 Linux and Windows, ARM64 macOS and wasm32 under wasmtime.
const OUTPUT_PIN: &str = "sha256:8743b5732fef912dead4a9022af3af9e1d6b55cab25f73da93884833a656c784";

#[test]
fn ac3_output_bits_pinned() {
    let mut h = Sha256Hasher::new();
    for c in load_mcc() {
        let r = run_mcc(&c, 1).unwrap();
        h.update(c.id.as_bytes());
        for v in r.centre.iter().chain([&r.radius]) {
            h.update(&v.to_bits().to_le_bytes());
        }
        for s in &r.support {
            h.update(&s.to_le_bytes());
        }
    }
    for c in load_mic() {
        let r = run_mic(&c).unwrap();
        h.update(c.id.as_bytes());
        for v in [r.centre[0], r.centre[1], r.radius, r.boundary_distance_m] {
            h.update(&v.to_bits().to_le_bytes());
        }
        h.update(&[u8::from(r.on_region_boundary)]);
        for s in &r.contacts {
            h.update(&s.to_le_bytes());
        }
    }
    assert_eq!(h.finish(), OUTPUT_PIN, "output bits changed on {}", std::env::consts::ARCH);
}

#[test]
fn ac4_error_paths() {
    // AC-4: two points for a sphere.
    assert_eq!(mcc_sphere_welzl(&[[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]], 1), Err(FitError::TooFewPoints { n: 2, need: 4 }));
    assert_eq!(mcc_circle2d_welzl(&[[0.0, 0.0], [1.0, 0.0]], 1), Err(FitError::TooFewPoints { n: 2, need: 3 }));
    // REQ-005: collinear circle, coplanar sphere, non-finite input.
    let line: Vec<[f64; 2]> = (0..6).map(|i| [0.25 * i as f64, 0.5 * i as f64]).collect();
    assert!(matches!(mcc_circle2d_welzl(&line, 1), Err(FitError::Collinear { n: 6, .. })));
    let flat = [[0.0, 0.0, 1.0], [1.0, 0.0, 1.0], [0.0, 1.0, 1.0], [1.0, 1.0, 1.0], [0.5, 0.25, 1.0]];
    assert!(matches!(mcc_sphere_welzl(&flat, 1), Err(FitError::Coplanar { n: 5, rank: 2, .. })));
    let mut nan = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    nan[2][1] = f64::NAN;
    assert_eq!(mcc_sphere_welzl(&nan, 1), Err(FitError::NonFinite { index: 2 }));

    // Voronoi MIC: too few points, collinear points, bad regions, tolerance below the floor.
    let sq = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
    assert!(matches!(mic_circle2d_vor(&[[0.1, 0.1], [0.9, 0.9]], &sq, 1e-12), Err(FitError::TooFewPoints { .. })));
    let diag: Vec<[f64; 2]> = (0..5).map(|i| [0.2 * i as f64, 0.2 * i as f64]).collect();
    assert!(matches!(mic_circle2d_vor(&diag, &sq, 1e-12), Err(FitError::Collinear { .. })));
    let pts = [[0.1, 0.1], [0.9, 0.2], [0.5, 0.8], [0.4, 0.4]];
    let region_err = Err(FitError::InvalidParams { field: "region" });
    let cw = [[0.0, 0.0], [0.0, 1.0], [1.0, 1.0], [1.0, 0.0]];
    assert_eq!(mic_circle2d_vor(&pts, &cw, 1e-12), region_err);
    let star: Vec<[f64; 2]> = [0usize, 2, 4, 1, 3]
        .iter()
        .map(|&k| {
            let a = core::f64::consts::TAU * k as f64 / 5.0;
            [0.5 + 0.4 * a.cos(), 0.5 + 0.4 * a.sin()]
        })
        .collect();
    assert_eq!(mic_circle2d_vor(&pts, &star, 1e-12), region_err);
    assert_eq!(mic_circle2d_vor(&pts, &sq[..2], 1e-12), region_err);
    let floor = region_tol_floor(&sq).unwrap();
    assert_eq!(floor, 16.0 * f64::EPSILON);
    for tol in [0.0, -0.0, floor / 2.0, f64::NAN, f64::INFINITY] {
        assert_eq!(mic_circle2d_vor(&pts, &sq, tol), Err(FitError::InvalidParams { field: "boundary_tol" }), "{tol}");
    }
    assert!(mic_circle2d_vor(&pts, &sq, floor).is_ok());
    let far = [[5.0, 5.0], [6.0, 5.0], [6.0, 6.0]];
    assert_eq!(intersect_convex(&sq, &far), Err(FitError::InvalidParams { field: "region_intersection" }));
}

#[test]
fn hand_labelled_envelopes() {
    // Square corners: every corner is on the circle about the centre; the support is canonical.
    let sq = [[0.0, 0.0], [2.0, 0.0], [2.0, 2.0], [0.0, 2.0]];
    let e = mcc_circle2d_welzl(&sq, 7).unwrap();
    assert_eq!(e.centre, [1.0, 1.0, 0.0]);
    assert_eq!(e.radius, core::f64::consts::SQRT_2);
    for seed in 0..16 {
        assert_eq!(mcc_circle2d_welzl(&sq, seed).unwrap(), e);
    }
    // Obtuse triangle: the long side is the diameter, two support points.
    let tri = [[0.0, 0.0], [4.0, 0.0], [2.0, 0.5]];
    let e = mcc_circle2d_welzl(&tri, 1).unwrap();
    assert_eq!(e.support, vec![0, 1]);
    assert_eq!(e.radius, 2.0);
    // Regular tetrahedron corners of a cube: four support points, radius sqrt(3).
    let tet = [[1.0, 1.0, 1.0], [1.0, -1.0, -1.0], [-1.0, 1.0, -1.0], [-1.0, -1.0, 1.0], [0.0, 0.0, 0.1]];
    let e = mcc_sphere_welzl(&tet, 1).unwrap();
    assert_eq!(e.support, vec![0, 1, 2, 3]);
    assert!((e.radius - 3.0_f64.sqrt()).abs() <= 4.0 * f64::EPSILON);

    // MIC in the unit square with four corner points and the centre point: the four edge
    // midpoints are each 0.5 from two corners and the centre, nothing is farther; the
    // total-order tie-break picks (0, 0.5), on the boundary, touching points 0, 3 and 4.
    let pts = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0], [0.5, 0.5]];
    let sq = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
    let r = mic_circle2d_vor(&pts, &sq, 1e-12).unwrap();
    assert_eq!(r.radius, 0.5);
    assert_eq!(r.centre, [0.0, 0.5]);
    assert!(r.on_region_boundary);
    assert_eq!(r.boundary_distance_m, 0.0);
    assert_eq!(r.contacts, vec![0, 3, 4]);
    // Without the centre point the optimum is the square's centre, interior, 0.5 from the
    // boundary, touching all four corners.
    let r = mic_circle2d_vor(&pts[..4], &sq, 1e-12).unwrap();
    assert_eq!(r.centre, [0.5, 0.5]);
    assert_eq!(r.radius, core::f64::consts::FRAC_1_SQRT_2);
    assert!(!r.on_region_boundary);
    assert_eq!(r.boundary_distance_m, 0.5);
    assert_eq!(r.contacts, vec![0, 1, 2, 3]);
}

#[test]
fn spec_cards_pass_the_card_check() {
    let mut ids = Vec::new();
    for text in CARDS {
        ids.push(parse_card(text).expect("card parses").id);
    }
    assert_eq!(ids, algorithm_ids().to_vec());
    assert!(check(&algorithm_ids(), &ids).is_clean());
    let strs: Vec<String> = algorithm_ids().iter().map(|a| a.to_string()).collect();
    assert_eq!(strs, [MCC_CIRCLE2D_ID, MCC_SPHERE_ID, MIC_CIRCLE2D_ID]);
}
