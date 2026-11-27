//! Acceptance tests AC-1..AC-5 for `KRN.CONS.AUTO.PAIR` (`fid_construct::auto_pair`).
//!
//! The fixture `fixtures/cons/auto-pair-block-v1` is regenerated here from the generator
//! documented in its `make_cases.py` (bit-identical points: only +, -, *, / and sqrt). The
//! expected counts, edge-band assignments and encoding digests come from that stdlib Python
//! script, never from this crate. The small cases below are hand-labelled from the spec's
//! rules. Uses `include_str!` so every test runs identically on x86-64, ARM64 and
//! `wasm32-wasip1`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_construct::auto_pair::{
    pair, pair_buffers, ConflictRule, NominalPrimitive, NormalSense, PairParams, PairStatus, Pairing, ANGLE_GATE_FLOOR,
    PARAM_CONFLICT_RULE, PARAM_MIN_POINTS,
};
use fid_construct::contract::{ConstructError, ConstructParams, QuantityKind};
use fid_fit::contract::PointsView;
use fid_num::NumError;

const EXPECTED: &str = include_str!("../../../fixtures/cons/auto-pair-block-v1/expected.tsv");
const EDGE_BAND: &str = include_str!("../../../fixtures/cons/auto-pair-block-v1/edge_band.tsv");

// ------------------------------------------------------------------ fixture generator

const SEED: u64 = 20261127;
const NOISE: f64 = 5e-6;
const L: f64 = 0.1;
const W: f64 = 0.06;
const H: f64 = 0.04;
const GATE: f64 = 1e-4;
const CYL1: (f64, f64, f64) = (0.03, 0.03, 0.008);
const CYL2: (f64, f64, f64) = (0.07, 0.02, 0.003);
const POCKET: (f64, f64, f64) = (0.075, 0.042, 0.01);
const COUNTS: [usize; 9] = [30000, 30000, 15000, 15000, 20000, 20000, 30000, 400, 39600];

fn gate_angle() -> f64 {
    10.0 * core::f64::consts::PI / 180.0
}

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn u(&mut self) -> f64 {
        (self.next() >> 11) as f64 * (1.0 / 9_007_199_254_740_992.0)
    }
}

fn plane_elems() -> [([f64; 3], [f64; 3]); 6] {
    [
        ([0.0, 0.0, 0.0], [0.0, 0.0, -1.0]),
        ([0.0, 0.0, H], [0.0, 0.0, 1.0]),
        ([0.0, 0.0, 0.0], [-1.0, 0.0, 0.0]),
        ([L, 0.0, 0.0], [1.0, 0.0, 0.0]),
        ([0.0, 0.0, 0.0], [0.0, -1.0, 0.0]),
        ([0.0, W, 0.0], [0.0, 1.0, 0.0]),
    ]
}

fn elements() -> Vec<NominalPrimitive> {
    let mut v: Vec<NominalPrimitive> =
        plane_elems().iter().map(|&(origin, normal)| NominalPrimitive::Plane { origin, normal }).collect();
    for c in [CYL1, CYL2] {
        v.push(NominalPrimitive::Cylinder {
            axis_point: [c.0, c.1, 0.0],
            axis: [0.0, 0.0, 1.0],
            radius: c.2,
            sense: NormalSense::Inward,
        });
    }
    v.push(NominalPrimitive::Sphere { centre: [POCKET.0, POCKET.1, H], radius: POCKET.2, sense: NormalSense::Inward });
    v
}

fn in_disc(x: f64, y: f64, d: (f64, f64, f64)) -> bool {
    let dx = x - d.0;
    let dy = y - d.1;
    dx * dx + dy * dy <= d.2 * d.2
}

struct Block {
    xyz: Vec<f64>,
    normals: Vec<f64>,
    labels: Vec<usize>,
}

fn generate() -> Block {
    let mut rng = Rng(SEED);
    let planes = plane_elems();
    let mut b = Block { xyz: Vec::new(), normals: Vec::new(), labels: Vec::new() };
    for (e, &count) in COUNTS.iter().enumerate() {
        let mut k = 0;
        while k < count {
            let (p, n): ([f64; 3], [f64; 3]) = if e < 6 {
                let (o, n) = planes[e];
                let q = if e < 2 {
                    let x = L * rng.u();
                    let y = W * rng.u();
                    if in_disc(x, y, CYL1) || in_disc(x, y, CYL2) || (e == 1 && in_disc(x, y, POCKET)) {
                        continue;
                    }
                    [x, y, o[2]]
                } else if e < 4 {
                    let y = W * rng.u();
                    let z = H * rng.u();
                    [o[0], y, z]
                } else {
                    let x = L * rng.u();
                    let z = H * rng.u();
                    [x, o[1], z]
                };
                let eps = (2.0 * rng.u() - 1.0) * NOISE;
                ([q[0] + n[0] * eps, q[1] + n[1] * eps, q[2] + n[2] * eps], n)
            } else if e < 8 {
                let c = if e == 6 { CYL1 } else { CYL2 };
                let a = 2.0 * rng.u() - 1.0;
                let bb = 2.0 * rng.u() - 1.0;
                let s = a * a + bb * bb;
                if !(1e-4..=1.0).contains(&s) {
                    continue;
                }
                let r = s.sqrt();
                let (dx, dy) = (a / r, bb / r);
                let z = H * rng.u();
                let eps = (2.0 * rng.u() - 1.0) * NOISE;
                ([c.0 + (c.2 + eps) * dx, c.1 + (c.2 + eps) * dy, z], [-dx, -dy, 0.0])
            } else {
                let a = 2.0 * rng.u() - 1.0;
                let bb = 2.0 * rng.u() - 1.0;
                let cc = 2.0 * rng.u() - 1.0;
                let s = (a * a + bb * bb) + cc * cc;
                if !(1e-4..=1.0).contains(&s) {
                    continue;
                }
                let r = s.sqrt();
                let mut d = [a / r, bb / r, cc / r];
                if d[2] > 0.0 {
                    d[2] = -d[2];
                }
                if d[2] == 0.0 {
                    continue;
                }
                let eps = (2.0 * rng.u() - 1.0) * NOISE;
                let rr = POCKET.2 + eps;
                ([POCKET.0 + rr * d[0], POCKET.1 + rr * d[1], H + rr * d[2]], [-d[0], -d[1], -d[2]])
            };
            b.xyz.extend_from_slice(&p);
            b.normals.extend_from_slice(&n);
            b.labels.push(e);
            k += 1;
        }
    }
    b
}

fn expected(key: &str) -> &'static str {
    EXPECTED
        .lines()
        .filter(|l| !l.starts_with('#'))
        .find_map(|l| l.split_once('\t').filter(|(k, _)| *k == key).map(|(_, v)| v))
        .unwrap_or_else(|| panic!("missing key {key}"))
}

fn params(min_points: u32, rule: ConflictRule) -> PairParams {
    PairParams::new(GATE, gate_angle(), min_points, rule).unwrap()
}

/// Every point is accounted for exactly once (module documentation, "Accounting").
fn assert_partition(pr: &Pairing) {
    let mut seen = vec![0_u8; pr.points() as usize];
    let mut mark = |xs: &[u32]| {
        assert!(xs.windows(2).all(|w| w[0] < w[1]), "not ascending");
        for &i in xs {
            seen[i as usize] += 1;
        }
    };
    for e in pr.per_element() {
        mark(e.indices());
        mark(e.released());
        match e.status() {
            PairStatus::Paired => assert!(e.released().is_empty() && e.count() as usize == e.indices().len()),
            PairStatus::Unpaired { count } => {
                assert!(e.indices().is_empty() && count == e.count() && count as usize == e.released().len())
            }
        }
    }
    mark(pr.ambiguous());
    mark(pr.normal_undefined());
    let claimed = seen.iter().filter(|&&s| s == 1).count();
    assert!(seen.iter().all(|&s| s <= 1));
    assert_eq!(claimed + pr.ungated() as usize, pr.points() as usize);
}

// ------------------------------------------------------------------ AC-1 .. AC-4

#[test]
fn ac1_block_every_point_to_its_generating_element() {
    let b = generate();
    assert_eq!(b.labels.len().to_string(), expected("points"));
    assert_eq!(format!("{:016x}", GATE.to_bits()), expected("gate_m"));
    assert_eq!(format!("{:016x}", gate_angle().to_bits()), expected("gate_rad"));
    let view = PointsView::new(&b.xyz, Some(&b.normals)).unwrap();
    let p = params(100, ConflictRule::Nearest);
    assert_eq!(p.digest().as_str(), expected("ac1.params_digest"));
    let pr = pair(&view, true, &elements(), None, &p).unwrap();
    assert_partition(&pr);
    for (e, el) in pr.per_element().iter().enumerate() {
        assert_eq!(el.status(), PairStatus::Paired);
        assert_eq!(el.count().to_string(), expected(&format!("ac1.count.{e}")));
        let want: Vec<u32> = (0..b.labels.len()).filter(|&i| b.labels[i] == e).map(|i| i as u32).collect();
        assert_eq!(el.indices(), want.as_slice(), "element {e}");
    }
    assert!(pr.ambiguous().is_empty() && pr.normal_undefined().is_empty());
    assert_eq!(pr.ungated(), 0);
    assert_eq!(pr.digest().as_str(), expected("ac1.digest"));

    let ex = pair(&view, true, &elements(), None, &params(100, ConflictRule::ExcludeAmbiguous)).unwrap();
    assert_eq!(ex.ambiguous().len().to_string(), expected("ac1_exclude.ambiguous"));
    assert_eq!(ex.digest().as_str(), expected("ac1_exclude.digest"));
}

fn edge_band() -> Vec<(usize, usize)> {
    EDGE_BAND
        .lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
        .map(|l| {
            let (a, b) = l.split_once('\t').unwrap();
            (a.parse().unwrap(), b.parse().unwrap())
        })
        .collect()
}

#[test]
fn ac2_edge_band_nearest_and_exclude_ambiguous() {
    let b = generate();
    let band = edge_band();
    assert_eq!(band.len().to_string(), expected("edge.points"));
    let mut xyz = Vec::new();
    for &(i, _) in &band {
        xyz.extend_from_slice(&b.xyz[3 * i..3 * i + 3]);
        // Each listed point really is within 0.05 mm of two planes.
        let near = plane_elems()
            .iter()
            .filter(|(o, n)| {
                let v = [b.xyz[3 * i] - o[0], b.xyz[3 * i + 1] - o[1], b.xyz[3 * i + 2] - o[2]];
                (v[0] * n[0] + v[1] * n[1] + v[2] * n[2]).abs() <= 5e-5
            })
            .count();
        assert!(near >= 2, "point {i}");
    }
    let view = PointsView::new(&xyz, None).unwrap();

    let near = pair(&view, false, &elements(), None, &params(1, ConflictRule::Nearest)).unwrap();
    assert_partition(&near);
    for (j, &(_, want)) in band.iter().enumerate() {
        let got = near
            .per_element()
            .iter()
            .position(|e| e.indices().contains(&(j as u32)) || e.released().contains(&(j as u32)))
            .unwrap();
        assert_eq!(got, want, "edge point {j}");
    }
    assert_eq!(near.digest().as_str(), expected("edge.nearest.digest"));

    let excl = pair(&view, false, &elements(), None, &params(1, ConflictRule::ExcludeAmbiguous)).unwrap();
    assert_partition(&excl);
    assert_eq!(excl.ambiguous().len(), band.len());
    assert!(excl.per_element().iter().all(|e| e.count() == 0 && e.status() == PairStatus::Unpaired { count: 0 }));
    assert_eq!(excl.digest().as_str(), expected("edge.exclude.digest"));
}

#[test]
fn ac3_min_points_leaves_small_hole_unpaired() {
    let b = generate();
    let view = PointsView::new(&b.xyz, Some(&b.normals)).unwrap();
    let pr = pair(&view, true, &elements(), None, &params(1000, ConflictRule::Nearest)).unwrap();
    assert_partition(&pr);
    let cyl2 = &pr.per_element()[7];
    assert_eq!(cyl2.status(), PairStatus::Unpaired { count: 400 });
    assert_eq!(cyl2.count().to_string(), expected("ac3.unpaired.7"));
    assert!(cyl2.indices().is_empty());
    let want: Vec<u32> = (0..b.labels.len()).filter(|&i| b.labels[i] == 7).map(|i| i as u32).collect();
    assert_eq!(cyl2.released(), want.as_slice());
    // Released to no other element.
    for (e, el) in pr.per_element().iter().enumerate() {
        if e != 7 {
            assert_eq!(el.status(), PairStatus::Paired);
            assert!(el.indices().iter().all(|&i| b.labels[i as usize] == e));
        }
    }
    assert_eq!(pr.digest().as_str(), expected("ac3.digest"));
}

/// Per element: every point inside an axis-aligned box that holds every point passing its
/// gates (the planes are unbounded, so their box spans the block in the free directions).
fn candidate_lists(xyz: &[f64]) -> Vec<Vec<u32>> {
    let m = 1e-3;
    let all = (-m, L + m, -m, W + m, -m, H + m);
    let boxes = [
        (all.0, all.1, all.2, all.3, -m, m),
        (all.0, all.1, all.2, all.3, H - m, H + m),
        (-m, m, all.2, all.3, all.4, all.5),
        (L - m, L + m, all.2, all.3, all.4, all.5),
        (all.0, all.1, -m, m, all.4, all.5),
        (all.0, all.1, W - m, W + m, all.4, all.5),
        (CYL1.0 - CYL1.2 - m, CYL1.0 + CYL1.2 + m, CYL1.1 - CYL1.2 - m, CYL1.1 + CYL1.2 + m, all.4, all.5),
        (CYL2.0 - CYL2.2 - m, CYL2.0 + CYL2.2 + m, CYL2.1 - CYL2.2 - m, CYL2.1 + CYL2.2 + m, all.4, all.5),
        (
            POCKET.0 - POCKET.2 - m,
            POCKET.0 + POCKET.2 + m,
            POCKET.1 - POCKET.2 - m,
            POCKET.1 + POCKET.2 + m,
            H - POCKET.2 - m,
            H + POCKET.2 + m,
        ),
    ];
    boxes
        .iter()
        .map(|bx| {
            (0..xyz.len() / 3)
                .filter(|&i| {
                    let p = &xyz[3 * i..3 * i + 3];
                    (bx.0..=bx.1).contains(&p[0]) && (bx.2..=bx.3).contains(&p[1]) && (bx.4..=bx.5).contains(&p[2])
                })
                .map(|i| i as u32)
                .collect()
        })
        .collect()
}

#[test]
fn ac4_candidate_lists_do_not_change_the_encoding() {
    let b = generate();
    let view = PointsView::new(&b.xyz, Some(&b.normals)).unwrap();
    let cands = candidate_lists(&b.xyz);
    assert!(cands.iter().map(Vec::len).sum::<usize>() < 3 * b.labels.len());
    for (min, rule, key) in [
        (100, ConflictRule::Nearest, "ac1.digest"),
        (100, ConflictRule::ExcludeAmbiguous, "ac1_exclude.digest"),
        (1000, ConflictRule::Nearest, "ac3.digest"),
    ] {
        let p = params(min, rule);
        let full = pair(&view, true, &elements(), None, &p).unwrap();
        let bounded = pair(&view, true, &elements(), Some(&cands), &p).unwrap();
        assert_eq!(full.canonical_bytes(), bounded.canonical_bytes(), "{key}");
        // The committed digest pins the bytes on every platform CI runs.
        assert_eq!(bounded.digest().as_str(), expected(key), "{key}");
    }
}

// ------------------------------------------------------------------ AC-5 error paths

fn one_plane() -> Vec<NominalPrimitive> {
    vec![NominalPrimitive::Plane { origin: [0.0; 3], normal: [0.0, 0.0, 1.0] }]
}

#[test]
fn ac5_invalid_params_and_nan() {
    for g in [-1e-4, -0.0, 0.0, f64::NAN, f64::INFINITY] {
        assert_eq!(
            PairParams::new(g, 0.1, 1, ConflictRule::Nearest),
            Err(ConstructError::InvalidParams { field: "distance_gate_m" })
        );
    }
    for a in [-0.1, 0.0, ANGLE_GATE_FLOOR / 2.0, 3.2, f64::NAN] {
        assert_eq!(
            PairParams::new(1e-4, a, 1, ConflictRule::Nearest),
            Err(ConstructError::InvalidParams { field: "normal_angle_gate_rad" })
        );
    }
    assert_eq!(
        PairParams::new(1e-4, 0.1, 0, ConflictRule::Nearest),
        Err(ConstructError::InvalidParams { field: "min_points" })
    );

    let p = PairParams::new(1e-3, 0.2, 1, ConflictRule::Nearest).unwrap();
    // A NaN coordinate: a numeric error naming the point, and no pairing.
    let xyz = [0.0, 0.0, 0.0, 1.0, f64::NAN, 0.0];
    assert_eq!(
        pair_buffers(&xyz, None, false, &one_plane(), None, &p),
        Err(ConstructError::Num(NumError::NotANumber { op: "construct.auto_pair", index: Some(1) }))
    );
    let xyz = [0.0, f64::NEG_INFINITY, 0.0];
    assert_eq!(
        pair_buffers(&xyz, None, false, &one_plane(), None, &p),
        Err(ConstructError::Num(NumError::Infinite { op: "construct.auto_pair", index: Some(0) }))
    );
    let normals = [0.0, f64::NAN, 1.0];
    assert_eq!(
        pair_buffers(&[0.0; 3], Some(&normals), false, &one_plane(), None, &p),
        Err(ConstructError::Num(NumError::NotANumber { op: "construct.auto_pair.normals", index: Some(0) }))
    );
    assert_eq!(
        pair_buffers(&[0.0; 3], Some(&[0.0, 0.0, 2.0]), false, &one_plane(), None, &p),
        Err(ConstructError::InvalidParams { field: "normals" })
    );
    assert_eq!(
        pair_buffers(&[0.0; 4], None, false, &one_plane(), None, &p),
        Err(ConstructError::InvalidParams { field: "points" })
    );
    // A NaN in a nominal element names the element.
    let elems = vec![
        one_plane()[0],
        NominalPrimitive::Sphere { centre: [0.0, f64::NAN, 0.0], radius: 1.0, sense: NormalSense::Outward },
    ];
    assert_eq!(
        pair_buffers(&[0.0; 3], None, false, &elems, None, &p),
        Err(ConstructError::Num(NumError::NotANumber { op: "construct.auto_pair", index: Some(1) }))
    );
}

#[test]
fn malformed_elements_candidates_and_floor() {
    let p = PairParams::new(1e-3, 0.2, 1, ConflictRule::Nearest).unwrap();
    let pts = [0.0, 0.0, 0.0, 1.0, 0.0, 0.0];
    let view = PointsView::new(&pts, None).unwrap();
    let bad_dir = |e: usize| Err(ConstructError::InvalidFeature { index: Some(e), code: "invalid_direction" });
    let bad_par = |e: usize| Err(ConstructError::InvalidFeature { index: Some(e), code: "invalid_params" });
    let cases: Vec<(NominalPrimitive, Result<Pairing, ConstructError>)> = vec![
        (NominalPrimitive::Plane { origin: [0.0; 3], normal: [0.0, 0.0, 1.001] }, bad_dir(0)),
        (
            NominalPrimitive::Cylinder {
                axis_point: [0.0; 3],
                axis: [0.0, 0.0, 1.0],
                radius: 0.0,
                sense: NormalSense::Outward,
            },
            bad_par(0),
        ),
        (
            NominalPrimitive::Cone {
                apex: [0.0; 3],
                axis: [0.0, 0.0, 1.0],
                half_angle_rad: core::f64::consts::FRAC_PI_2,
                sense: NormalSense::Outward,
            },
            bad_par(0),
        ),
        (
            NominalPrimitive::Torus {
                centre: [0.0; 3],
                axis: [0.0, 0.0, 1.0],
                major_radius: 1.0,
                minor_radius: 1.0,
                sense: NormalSense::Outward,
            },
            bad_par(0),
        ),
        (
            NominalPrimitive::SlotInPlane {
                centre: [0.0; 3],
                plane_normal: [0.0, 0.0, 1.0],
                slot_axis: [0.0, 0.6, 0.8],
                half_length: 1.0,
                radius: 0.5,
                sense: NormalSense::Outward,
            },
            bad_dir(0),
        ),
        (
            NominalPrimitive::SlotInPlane {
                centre: [0.0; 3],
                plane_normal: [0.0, 0.0, 1.0],
                slot_axis: [1.0, 0.0, 0.0],
                half_length: -0.0,
                radius: 0.5,
                sense: NormalSense::Outward,
            },
            bad_par(0),
        ),
    ];
    for (el, want) in cases {
        assert_eq!(pair(&view, false, &[el], None, &p), want, "{el:?}");
    }
    assert_eq!(pair(&view, false, &[], None, &p), Err(ConstructError::InvalidParams { field: "elems" }));
    assert_eq!(pair(&view, true, &one_plane(), None, &p), Err(ConstructError::InvalidParams { field: "normals" }));
    for c in [vec![], vec![vec![0, 2]], vec![vec![1, 0]], vec![vec![0, 0]], vec![vec![0], vec![1]]] {
        assert_eq!(
            pair(&view, false, &one_plane(), Some(&c), &p),
            Err(ConstructError::InvalidParams { field: "candidates" }),
            "{c:?}"
        );
    }
    // A gate below the rounding resolution of the coordinates is refused.
    let far = [1e12, 0.0, 0.0];
    let view = PointsView::new(&far, None).unwrap();
    assert_eq!(
        pair(&view, false, &one_plane(), None, &p),
        Err(ConstructError::InvalidParams { field: "distance_gate_m" })
    );
}

#[test]
fn params_round_trip_through_construct_params() {
    let p = PairParams::new(1e-4, 0.25, 1234, ConflictRule::ExcludeAmbiguous).unwrap();
    let q = PairParams::from_construct_params(p.construct_params()).unwrap();
    assert_eq!(p, q);
    assert_eq!(p.digest(), q.digest());
    assert_ne!(p.digest(), PairParams::new(1e-4, 0.25, 1234, ConflictRule::Nearest).unwrap().digest());
    assert_ne!(p.digest(), PairParams::new(1e-4, 0.25, 1235, ConflictRule::ExcludeAmbiguous).unwrap().digest());

    let base = ConstructParams::new(0.25, 1e-4).unwrap();
    assert_eq!(
        PairParams::from_construct_params(&base.clone().with(PARAM_CONFLICT_RULE, QuantityKind::Count, 0.0).unwrap()),
        Err(ConstructError::MissingParam { name: PARAM_MIN_POINTS })
    );
    assert_eq!(
        PairParams::from_construct_params(&base.clone().with(PARAM_MIN_POINTS, QuantityKind::Count, 5.0).unwrap()),
        Err(ConstructError::MissingParam { name: PARAM_CONFLICT_RULE })
    );
    let full = base.with(PARAM_MIN_POINTS, QuantityKind::Count, 5.0).unwrap();
    let rule2 = full.clone().with(PARAM_CONFLICT_RULE, QuantityKind::Count, 2.0).unwrap();
    assert_eq!(
        PairParams::from_construct_params(&rule2),
        Err(ConstructError::InvalidParams { field: PARAM_CONFLICT_RULE })
    );
    let extra = full
        .clone()
        .with(PARAM_CONFLICT_RULE, QuantityKind::Count, 0.0)
        .unwrap()
        .with("unused", QuantityKind::Length, 1.0)
        .unwrap();
    assert_eq!(PairParams::from_construct_params(&extra), Err(ConstructError::InvalidParams { field: "values" }));
    let zero = ConstructParams::new(0.25, 1e-4)
        .unwrap()
        .with(PARAM_MIN_POINTS, QuantityKind::Count, 0.0)
        .unwrap()
        .with(PARAM_CONFLICT_RULE, QuantityKind::Count, 0.0)
        .unwrap();
    assert_eq!(
        PairParams::from_construct_params(&zero),
        Err(ConstructError::InvalidParams { field: PARAM_MIN_POINTS })
    );
}

// ------------------------------------------------------------------ hand-labelled rules

fn run(
    pts: &[f64],
    normals: Option<&[f64]>,
    elems: &[NominalPrimitive],
    gate: f64,
    angle: f64,
    rule: ConflictRule,
) -> Pairing {
    let p = PairParams::new(gate, angle, 1, rule).unwrap();
    let pr = pair_buffers(pts, normals, false, elems, None, &p).unwrap();
    assert_partition(&pr);
    pr
}

fn owner(pr: &Pairing, i: u32) -> Option<usize> {
    pr.per_element().iter().position(|e| e.indices().contains(&i) || e.released().contains(&i))
}

#[test]
fn ties_resolve_by_angle_then_element_index() {
    // Two planes through the origin: B (x = 0, normal +x) listed first, A (z = 0, normal +z).
    let b = NominalPrimitive::Plane { origin: [0.0; 3], normal: [1.0, 0.0, 0.0] };
    let a = NominalPrimitive::Plane { origin: [0.0; 3], normal: [0.0, 0.0, 1.0] };
    // Point 0 is 1 mm from both planes; its normal (0.6, 0, 0.8) is 36.87 deg from A and
    // 53.13 deg from B, both inside a 60 deg gate: the smaller angle wins over the index.
    // Point 1 lies on both planes with its normal along the bisector: equal distance and
    // equal angle (45 deg), so the lower element index (B) wins.
    let pts = [1e-3, 0.0, 1e-3, 0.0, 5.0, 0.0];
    let s = core::f64::consts::FRAC_1_SQRT_2;
    let normals = [0.6, 0.0, 0.8, s, 0.0, s];
    let pr = run(&pts, Some(&normals), &[b, a], 2e-3, 60f64.to_radians(), ConflictRule::Nearest);
    assert_eq!(owner(&pr, 0), Some(1));
    assert_eq!(owner(&pr, 1), Some(0));
    // Without normals the angle is 0 for both: point 0 then goes to the lower index (B).
    let pr = run(&pts, None, &[b, a], 2e-3, 60f64.to_radians(), ConflictRule::Nearest);
    assert_eq!(owner(&pr, 0), Some(0));
    // Smaller distance beats a smaller angle.
    let pts = [0.5e-3, 0.0, 1e-3];
    let pr = run(&pts, Some(&[0.6, 0.0, 0.8]), &[b, a], 2e-3, 60f64.to_radians(), ConflictRule::Nearest);
    assert_eq!(owner(&pr, 0), Some(0));
    // exclude_ambiguous assigns none of them.
    let pr = run(&pts, None, &[b, a], 2e-3, 60f64.to_radians(), ConflictRule::ExcludeAmbiguous);
    assert_eq!(pr.ambiguous(), &[0]);
}

#[test]
fn gates_are_inclusive_and_oriented() {
    let plane = NominalPrimitive::Plane { origin: [0.0; 3], normal: [0.0, 0.0, 1.0] };
    // Distances 0.5, 1.0 (on the gate, kept) and 1.5 mm (ungated); exact binary values.
    let pts = [0.0, 0.0, 0.5, 0.0, 0.0, 1.0, 0.0, 0.0, 1.5];
    let pr = run(&pts, None, &[plane], 1.0, 0.1, ConflictRule::Nearest);
    assert_eq!(pr.per_element()[0].indices(), &[0, 1]);
    assert_eq!(pr.ungated(), 1);
    // A point normal opposite to the nominal normal is at pi: refused by a 10 deg gate,
    // accepted only by a gate of pi.
    let pts = [0.0, 0.0, 0.0];
    let flipped = [0.0, 0.0, -1.0];
    let pr = run(&pts, Some(&flipped), &[plane], 1.0, 10f64.to_radians(), ConflictRule::Nearest);
    assert_eq!(pr.ungated(), 1);
    let pr = run(&pts, Some(&flipped), &[plane], 1.0, core::f64::consts::PI, ConflictRule::Nearest);
    assert_eq!(pr.per_element()[0].count(), 1);
}

#[test]
fn primitives_distance_and_normal_sense() {
    let gate = 0.01;
    let ang = 5f64.to_radians();
    let z = [0.0, 0.0, 1.0];
    // For each primitive: a point 0.005 off the surface whose normal is the outward normal
    // (claimed under Outward, refused under Inward), and a point 0.02 off it (ungated).
    type Case = (fn(NormalSense) -> NominalPrimitive, [f64; 3], [f64; 3], [f64; 3]);
    let cases: [Case; 6] = [
        (
            |s| NominalPrimitive::Cylinder { axis_point: [0.0; 3], axis: [0.0, 0.0, 1.0], radius: 1.0, sense: s },
            [1.005, 0.0, 7.0],
            [1.0, 0.0, 0.0],
            [0.0, 0.98, -3.0],
        ),
        (
            |s| NominalPrimitive::Sphere { centre: [1.0, 2.0, 3.0], radius: 2.0, sense: s },
            [1.0, 2.0, 4.995],
            [0.0, 0.0, 1.0],
            [1.0, 4.02, 3.0],
        ),
        (
            // 45 deg cone from the origin along +z: the surface point (1, 0, 1) has outward
            // normal (1, 0, -1)/sqrt2; step 0.005 along it.
            |s| NominalPrimitive::Cone {
                apex: [0.0; 3],
                axis: [0.0, 0.0, 1.0],
                half_angle_rad: core::f64::consts::FRAC_PI_4,
                sense: s,
            },
            [1.0 + 0.005 * core::f64::consts::FRAC_1_SQRT_2, 0.0, 1.0 - 0.005 * core::f64::consts::FRAC_1_SQRT_2],
            [core::f64::consts::FRAC_1_SQRT_2, 0.0, -core::f64::consts::FRAC_1_SQRT_2],
            [0.0, 1.0 + 0.02 * core::f64::consts::FRAC_1_SQRT_2, 1.0 - 0.02 * core::f64::consts::FRAC_1_SQRT_2],
        ),
        (
            // Torus R = 3, r = 1 around +z: the outer equator point (4, 0, 0) has normal +x.
            |s| NominalPrimitive::Torus {
                centre: [0.0; 3],
                axis: [0.0, 0.0, 1.0],
                major_radius: 3.0,
                minor_radius: 1.0,
                sense: s,
            },
            [4.005, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [0.0, 3.0, 1.02],
        ),
        (
            // Circle r = 2 in z = 0; in-plane distance 0.005, out-of-plane offset 0.001.
            |s| NominalPrimitive::CircleInPlane {
                centre: [0.0; 3],
                plane_normal: [0.0, 0.0, 1.0],
                radius: 2.0,
                sense: s,
            },
            [0.0, 2.005, 0.001],
            [0.0, 1.0, 0.0],
            [2.0, 0.0, 0.02],
        ),
        (
            // Slot along x, centre segment x in [-3, 3], radius 1, in z = 0: (0, 1.005, 0) is
            // beside the straight side; (4.02, 0, 0) is 0.02 past the end arc.
            |s| NominalPrimitive::SlotInPlane {
                centre: [0.0; 3],
                plane_normal: [0.0, 0.0, 1.0],
                slot_axis: [1.0, 0.0, 0.0],
                half_length: 3.0,
                radius: 1.0,
                sense: s,
            },
            [0.0, 1.005, 0.0],
            [0.0, 1.0, 0.0],
            [4.02, 0.0, 0.0],
        ),
    ];
    for (make, near, normal, far) in cases {
        let mut pts = near.to_vec();
        pts.extend_from_slice(&far);
        let mut nm = normal.to_vec();
        nm.extend_from_slice(&z);
        for (sense, claimed) in [(NormalSense::Outward, true), (NormalSense::Inward, false)] {
            let el = make(sense);
            let pr = run(&pts, Some(&nm), &[el], gate, ang, ConflictRule::Nearest);
            let got: &[u32] = if claimed { &[0] } else { &[] };
            assert_eq!(pr.per_element()[0].indices(), got, "{el:?}");
            assert_eq!(pr.ungated(), if claimed { 1 } else { 2 }, "{el:?}");
            // Without normals only the distance gate applies.
            let pr = run(&pts, None, &[el], gate, ang, ConflictRule::Nearest);
            assert_eq!(pr.per_element()[0].indices(), &[0], "{el:?}");
        }
    }
    // Slot end arc and interior: (3.5, 0.5, 0) is 1 - |(0.5, 0.5)| = 0.2929 inside the end
    // arc, not 0.5 from the straight side.
    let slot = NominalPrimitive::SlotInPlane {
        centre: [0.0; 3],
        plane_normal: [0.0, 0.0, 1.0],
        slot_axis: [1.0, 0.0, 0.0],
        half_length: 3.0,
        radius: 1.0,
        sense: NormalSense::Outward,
    };
    let pr = run(&[3.5, 0.5, 0.0, 3.5, 0.75, 0.0], None, &[slot], 0.29, 0.1, ConflictRule::Nearest);
    assert_eq!(pr.per_element()[0].indices(), &[1]);
    let pr = run(&[3.5, 0.5, 0.0], None, &[slot], 0.293, 0.1, ConflictRule::Nearest);
    assert_eq!(pr.per_element()[0].indices(), &[0]);
    // The circle's out-of-plane gate reuses the distance gate.
    let circle = NominalPrimitive::CircleInPlane {
        centre: [0.0; 3],
        plane_normal: [0.0, 0.0, 1.0],
        radius: 2.0,
        sense: NormalSense::Outward,
    };
    let pr = run(&[2.0, 0.0, 0.011], None, &[circle], gate, ang, ConflictRule::Nearest);
    assert_eq!(pr.ungated(), 1);
}

#[test]
fn undefined_nominal_normal_assigns_nothing() {
    let gate = 0.01;
    let ang = 5f64.to_radians();
    let up = [0.0, 0.0, 1.0];
    let cases = [
        // On the axis of a thin cylinder, at a small sphere's centre, on a thin torus's circle
        // of centres, near a cone apex from behind, on a slot's centre segment.
        (
            NominalPrimitive::Cylinder {
                axis_point: [0.0; 3],
                axis: [0.0, 0.0, 1.0],
                radius: 0.005,
                sense: NormalSense::Outward,
            },
            [0.0, 0.0, 3.0],
        ),
        (
            NominalPrimitive::Sphere { centre: [1.0, 1.0, 1.0], radius: 0.005, sense: NormalSense::Outward },
            [1.0, 1.0, 1.0],
        ),
        (
            NominalPrimitive::Torus {
                centre: [0.0; 3],
                axis: [0.0, 0.0, 1.0],
                major_radius: 2.0,
                minor_radius: 0.005,
                sense: NormalSense::Outward,
            },
            [2.0, 0.0, 0.0],
        ),
        (
            NominalPrimitive::Cone {
                apex: [0.0; 3],
                axis: [0.0, 0.0, 1.0],
                half_angle_rad: 0.5,
                sense: NormalSense::Outward,
            },
            [0.0, 0.0, -0.002],
        ),
        (
            NominalPrimitive::SlotInPlane {
                centre: [0.0; 3],
                plane_normal: [0.0, 0.0, 1.0],
                slot_axis: [1.0, 0.0, 0.0],
                half_length: 1.0,
                radius: 0.005,
                sense: NormalSense::Outward,
            },
            [0.5, 0.0, 0.0],
        ),
    ];
    for (el, pt) in cases {
        // A plane that would otherwise take the point (distance 0, normal +z, angle 0).
        let plane = NominalPrimitive::Plane { origin: pt, normal: up };
        let pr = run(&pt, Some(&up), &[plane, el], gate, ang, ConflictRule::Nearest);
        assert_eq!(pr.normal_undefined(), &[0], "{el:?}");
        assert!(pr.per_element().iter().all(|e| e.count() == 0), "{el:?}");
        // Without a point normal the distance gate alone decides: the plane (distance 0) wins.
        let pr = run(&pt, None, &[plane, el], gate, ang, ConflictRule::Nearest);
        assert!(pr.normal_undefined().is_empty());
        assert_eq!(owner(&pr, 0), Some(0), "{el:?}");
    }
}

#[test]
fn encoding_shape() {
    let plane = NominalPrimitive::Plane { origin: [0.0; 3], normal: [0.0, 0.0, 1.0] };
    let pts = [0.0, 0.0, 0.5, 0.0, 0.0, 3.0];
    let p = PairParams::new(1.0, 0.5, 2, ConflictRule::Nearest).unwrap();
    let pr = pair_buffers(&pts, None, false, &[plane], None, &p).unwrap();
    let want = format!(
        "{{\"ambiguous\":[],\"elements\":[{{\"count\":\"1\",\"indices\":[],\"released\":[\"0\"],\"status\":\"unpaired\"}}],\
         \"format\":\"construct.auto_pair.pairing@1\",\"normal_undefined\":[],\"params_digest\":\"{}\",\"points\":\"2\",\
         \"ungated\":\"1\"}}",
        p.digest().as_str()
    );
    assert_eq!(String::from_utf8(pr.canonical_bytes()).unwrap(), want);
}
