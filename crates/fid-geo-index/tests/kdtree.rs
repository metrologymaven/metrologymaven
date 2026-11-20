//! Acceptance criteria of spec node GEO.INDEX.KDTREE (AC-1 to AC-5) plus the edge cases and the
//! input-domain rules.
//!
//! Expected results come from `fixtures/geo/index/`, written by the independent stdlib-Python
//! reference `tools/fixtures/geo_index.py` (brute force over every point with a bit-exact model
//! of the pinned squared distance, plus an independent model of the pinned build for the index
//! digests); none come from this crate. The two large clouds and their query streams are
//! regenerated here from the pinned generators and checked against the fixture's `points` and
//! `queries` digests before use. The error cases are hand-labelled from the leaf's Design; the
//! randomised cross-checks at the end compare with a test-only brute force.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::BTreeMap;

use fid_geo_index::{
    dist2, leaf_depth, overhead_bytes, Neighbour, PointIndex, PointIndexError, COORD_LIMIT, MAX_POINTS,
};
use fid_geo_types::{Aabb, PointBuffer};
use fid_kci::kci::{sha256, to_hex, Sha256};

const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/geo/index/");

fn read(rel: &str) -> String {
    std::fs::read_to_string(format!("{ROOT}{rel}")).unwrap()
}

fn hexf(s: &str) -> f64 {
    f64::from_bits(u64::from_str_radix(s, 16).unwrap())
}

struct SplitMix(u64);

impl SplitMix {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    fn unit(&mut self) -> f64 {
        (self.next() >> 11) as f64 * (1.0 / 9_007_199_254_740_992.0)
    }
}

// ------------------------------------------------------------------ pinned generators

fn uniform_cloud() -> (Vec<[f64; 3]>, Vec<[f64; 3]>) {
    let mut g = SplitMix(21);
    let n = 100_000usize;
    let pts: Vec<[f64; 3]> = (0..n).map(|_| [g.unit() * 0.125, g.unit() * 0.125, g.unit() * 0.125]).collect();
    let mut qs = Vec::new();
    for i in 0..1000 {
        if i % 10 == 0 {
            qs.push(pts[(g.next() % n as u64) as usize]);
        } else {
            qs.push(std::array::from_fn(|_| g.unit() * 0.140625 - 0.0078125));
        }
    }
    (pts, qs)
}

fn lattice_cloud() -> (Vec<[f64; 3]>, Vec<[f64; 3]>) {
    let side = 50u64;
    let n = (side * side * side) as usize;
    let mut cells: Vec<u64> = (0..n as u64).collect();
    let mut g = SplitMix(22);
    for i in (1..n).rev() {
        let j = (g.next() % (i as u64 + 1)) as usize;
        cells.swap(i, j);
    }
    let pts: Vec<[f64; 3]> = cells
        .iter()
        .map(|&c| {
            let (a, rest) = (c / (side * side), c % (side * side));
            let (b, z) = (rest / side, rest % side);
            [a as f64 * 0.001, b as f64 * 0.001, z as f64 * 0.001]
        })
        .collect();
    let qs = (0..500).map(|_| pts[(g.next() % n as u64) as usize]).collect();
    (pts, qs)
}

fn cloud_digest(pts: &[[f64; 3]]) -> String {
    let mut h = Sha256::new();
    h.update(&(pts.len() as u64).to_le_bytes());
    for p in pts {
        for x in p {
            h.update(&x.to_bits().to_le_bytes());
        }
    }
    to_hex(&h.finish())
}

fn enc_nb(out: &mut Vec<u8>, res: &[Neighbour]) {
    out.extend_from_slice(&(res.len() as u64).to_le_bytes());
    for nb in res {
        out.extend_from_slice(&nb.index.to_le_bytes());
        out.extend_from_slice(&nb.dist2.to_bits().to_le_bytes());
    }
}

fn enc_idx(out: &mut Vec<u8>, res: &[u64]) {
    out.extend_from_slice(&(res.len() as u64).to_le_bytes());
    for i in res {
        out.extend_from_slice(&i.to_le_bytes());
    }
}

struct Large {
    points: (u64, String),
    queries: (u64, String),
    depth: u32,
    index: String,
    results: BTreeMap<(String, usize), String>,
}

fn parse_large(rel: &str) -> Large {
    let text = read(rel);
    let mut l = Large {
        points: (0, String::new()),
        queries: (0, String::new()),
        depth: 0,
        index: String::new(),
        results: BTreeMap::new(),
    };
    for line in text.lines().filter(|l| !l.starts_with('#')) {
        let f: Vec<&str> = line.split(' ').collect();
        match f[0] {
            "points" => l.points = (f[1].parse().unwrap(), f[2].to_string()),
            "queries" => l.queries = (f[1].parse().unwrap(), f[2].to_string()),
            "index" => {
                assert_eq!(f[1], "depth");
                l.depth = f[2].parse().unwrap();
                l.index = f[3].to_string();
            }
            "result" => {
                l.results.insert((f[1].to_string(), f[2].parse().unwrap()), f[3].to_string());
            }
            other => panic!("unknown line {other}"),
        }
    }
    l
}

fn query_digest(qs: &[[f64; 3]]) -> String {
    let mut h = Sha256::new();
    for q in qs {
        for x in q {
            h.update(&x.to_bits().to_le_bytes());
        }
    }
    to_hex(&h.finish())
}

/// Runs `run(kind, q, out)` for every query and compares the per-block digests.
fn check_blocks(l: &Large, qs: &[[f64; 3]], kinds: &[&str], run: impl Fn(&str, [f64; 3], &mut Vec<u8>)) {
    let mut seen = 0;
    for kind in kinds {
        for (b, block) in qs.chunks(100).enumerate() {
            let mut bytes = Vec::new();
            for &q in block {
                run(kind, q, &mut bytes);
            }
            let want = l.results.get(&(kind.to_string(), b)).unwrap_or_else(|| panic!("no result {kind} {b}"));
            assert_eq!(&to_hex(&sha256(&bytes)), want, "{kind} block {b}");
            seen += 1;
        }
    }
    assert_eq!(seen, l.results.len(), "every fixture block is checked");
}

// ------------------------------------------------------------------ AC-1, AC-3

#[test]
fn ac1_ac3_uniform_1e5_equals_brute_force() {
    let l = parse_large("uniform_1e5/expected.txt");
    let (pts, qs) = uniform_cloud();
    assert_eq!((pts.len() as u64, cloud_digest(&pts)), l.points);
    assert_eq!((qs.len() as u64, query_digest(&qs)), l.queries);
    let buf = PointBuffer::from_points(&pts);
    let idx = PointIndex::build(&buf, 1).unwrap();
    assert_eq!(idx.depth(), l.depth);
    assert_eq!(to_hex(&sha256(&idx.to_bytes().unwrap())), l.index, "AC-3 index digest");
    check_blocks(&l, &qs, &["knn1", "knn16", "knn1024", "radius", "box"], |kind, q, out| match kind {
        "knn1" => enc_nb(out, &idx.knn(q, 1).unwrap()),
        "knn16" => enc_nb(out, &idx.knn(q, 16).unwrap()),
        "knn1024" => enc_nb(out, &idx.knn(q, 1024).unwrap()),
        "radius" => enc_nb(out, &idx.radius(q, 0.002).unwrap()),
        "box" => {
            let b = Aabb::new(std::array::from_fn(|k| q[k] - 0.002), std::array::from_fn(|k| q[k] + 0.002)).unwrap();
            enc_idx(out, &idx.in_box(&b).unwrap())
        }
        _ => unreachable!(),
    });
}

#[test]
fn ac3_thread_count_does_not_change_the_index() {
    let (pts, _) = uniform_cloud();
    let buf = PointBuffer::from_points(&pts);
    let one = PointIndex::build(&buf, 1).unwrap().to_bytes().unwrap();
    for t in [4, 16] {
        assert_eq!(PointIndex::build(&buf, t).unwrap().to_bytes().unwrap(), one, "threads {t}");
    }
}

// ------------------------------------------------------------------ AC-2

#[test]
fn ac2_lattice_ties_in_index_order() {
    let l = parse_large("lattice_ties/expected.txt");
    let (pts, qs) = lattice_cloud();
    assert_eq!((pts.len() as u64, cloud_digest(&pts)), l.points);
    assert_eq!((qs.len() as u64, query_digest(&qs)), l.queries);
    let buf = PointBuffer::from_points(&pts);
    let idx = PointIndex::build(&buf, 16).unwrap();
    assert_eq!(idx.depth(), l.depth);
    assert_eq!(to_hex(&sha256(&idx.to_bytes().unwrap())), l.index);
    check_blocks(&l, &qs, &["knn26", "radius"], |kind, q, out| match kind {
        "knn26" => enc_nb(out, &idx.knn(q, 26).unwrap()),
        "radius" => enc_nb(out, &idx.radius(q, 0.0015).unwrap()),
        _ => unreachable!(),
    });
}

// ------------------------------------------------------------------ edge cases

#[test]
fn edge_cases_equal_brute_force() {
    let text = read("kdtree_edge/cases.txt");
    let mut lines = text.lines().filter(|l| !l.starts_with('#')).peekable();
    let mut cases = 0;
    let mut queries = 0;
    while let Some(head) = lines.next() {
        let f: Vec<&str> = head.split(' ').collect();
        assert_eq!(f[0], "case");
        let name = f[1];
        let n: usize = f[2].parse().unwrap();
        let mut pts = Vec::with_capacity(n);
        for _ in 0..n {
            let p: Vec<&str> = lines.next().unwrap().split(' ').collect();
            assert_eq!(p[0], "p");
            pts.push([hexf(p[1]), hexf(p[2]), hexf(p[3])]);
        }
        let buf = PointBuffer::from_points(&pts);
        let idx = PointIndex::build(&buf, 1).unwrap();
        let ix: Vec<&str> = lines.next().unwrap().split(' ').collect();
        assert_eq!(ix[0], "index");
        assert_eq!(idx.depth(), ix[1].parse::<u32>().unwrap(), "{name} depth");
        assert_eq!(to_hex(&sha256(&idx.to_bytes().unwrap())), ix[2], "{name} index digest");
        assert_eq!(idx.memory_bytes(), overhead_bytes(n as u64).unwrap(), "{name} memory");
        for line in lines.by_ref() {
            if line == "end" {
                break;
            }
            let (lhs, rhs) = line.split_once(" = ").unwrap();
            let l: Vec<&str> = lhs.split(' ').collect();
            let r: Vec<&str> = rhs.split(' ').collect();
            let count: usize = r[0].parse().unwrap();
            assert_eq!(r.len(), count + 1);
            let nb = |r: &[&str]| -> Vec<Neighbour> {
                r.iter()
                    .map(|s| {
                        let (i, d) = s.split_once(':').unwrap();
                        Neighbour { index: i.parse().unwrap(), dist2: hexf(d) }
                    })
                    .collect()
            };
            match l[0] {
                "knn" => {
                    let q = [hexf(l[1]), hexf(l[2]), hexf(l[3])];
                    let k: usize = l[4].parse().unwrap();
                    let got = idx.knn(q, k).unwrap();
                    assert_bits_eq(&got, &nb(&r[1..]), &format!("{name}: {lhs}"));
                }
                "radius" => {
                    let q = [hexf(l[1]), hexf(l[2]), hexf(l[3])];
                    let got = idx.radius(q, hexf(l[4])).unwrap();
                    assert_bits_eq(&got, &nb(&r[1..]), &format!("{name}: {lhs}"));
                }
                "box" => {
                    let lo = [hexf(l[1]), hexf(l[2]), hexf(l[3])];
                    let hi = [hexf(l[4]), hexf(l[5]), hexf(l[6])];
                    let got = idx.in_box(&Aabb::new(lo, hi).unwrap()).unwrap();
                    let want: Vec<u64> = r[1..].iter().map(|s| s.parse().unwrap()).collect();
                    assert_eq!(got, want, "{name}: {lhs}");
                }
                other => panic!("unknown query {other}"),
            }
            queries += 1;
        }
        cases += 1;
    }
    assert_eq!(cases, 20);
    assert!(queries > 400, "{queries}");
}

fn assert_bits_eq(got: &[Neighbour], want: &[Neighbour], ctx: &str) {
    assert_eq!(got.len(), want.len(), "{ctx}: count");
    for (i, (g, w)) in got.iter().zip(want).enumerate() {
        assert_eq!((g.index, g.dist2.to_bits()), (w.index, w.dist2.to_bits()), "{ctx}: hit {i}");
    }
}

// ------------------------------------------------------------------ AC-4

#[test]
fn ac4_overhead_at_1e8_points_is_at_most_12_bytes_per_point() {
    let n = 100_000_000u64;
    assert_eq!(leaf_depth(n), 23);
    let bytes = overhead_bytes(n).unwrap();
    assert!(bytes <= 12 * n, "{bytes}");
    // The worst ratio over every point count up to 2^32 is at the smallest count of each depth.
    for d in 1..=28u32 {
        let n = 16 * (1u64 << (d - 1)) + 1;
        assert_eq!(leaf_depth(n), d);
        assert_eq!(leaf_depth(n - 1), d - 1);
        assert!(overhead_bytes(n).unwrap() <= 12 * n, "n = {n}");
    }
    assert_eq!(overhead_bytes(MAX_POINTS + 1), None);
}

#[test]
fn ac4_memory_accounting_is_exact() {
    let mut g = SplitMix(5);
    for n in [0usize, 1, 16, 17, 100, 1000, 4097] {
        let pts: Vec<[f64; 3]> = (0..n).map(|_| [g.unit(), g.unit(), g.unit()]).collect();
        let buf = PointBuffer::from_points(&pts);
        let idx = PointIndex::build(&buf, 1).unwrap();
        let internal = (1usize << idx.depth()) - 1;
        assert_eq!(idx.perm().len(), n);
        assert_eq!(idx.splits().len(), internal);
        assert_eq!(idx.axes().len(), internal);
        let want = std::mem::size_of::<PointIndex>() + 4 * n + 9 * internal;
        assert_eq!(idx.memory_bytes(), want as u64, "n = {n}");
        assert_eq!(overhead_bytes(n as u64), Some(want as u64));
    }
}

/// AC-4 on a real 10^8-point cloud: needs about 4 GB; run with `--ignored` on a large runner.
#[test]
#[ignore]
fn ac4_seeded_1e8_cloud() {
    let n = 100_000_000usize;
    let mut g = SplitMix(4);
    let buf = PointBuffer::new(
        (0..n).map(|_| g.unit()).collect(),
        (0..n).map(|_| g.unit()).collect(),
        (0..n).map(|_| g.unit()).collect(),
    );
    let idx = PointIndex::build(&buf, 8).unwrap();
    assert!(idx.memory_bytes() <= 12 * n as u64, "{}", idx.memory_bytes());
}

// ------------------------------------------------------------------ AC-5 and the input domain

#[test]
fn ac5_typed_query_errors() {
    let buf = PointBuffer::from_points(&[[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]]);
    let idx = PointIndex::build(&buf, 1).unwrap();
    let o = [0.0; 3];
    assert_eq!(idx.knn(o, 0), Err(PointIndexError::BadK { k: 0 }));
    assert_eq!(idx.knn(o, 1025), Err(PointIndexError::BadK { k: 1025 }));
    assert_eq!(idx.knn(o, usize::MAX).unwrap_err().code(), "bad_k");
    assert_eq!(idx.knn([f64::NAN, 0.0, 0.0], 1), Err(PointIndexError::NonFiniteQuery { axis: 0 }));
    assert_eq!(idx.knn([0.0, f64::INFINITY, 0.0], 1), Err(PointIndexError::NonFiniteQuery { axis: 1 }));
    assert_eq!(idx.knn([0.0, 0.0, -f64::INFINITY], 1), Err(PointIndexError::NonFiniteQuery { axis: 2 }));
    // The query point is checked before k.
    assert_eq!(idx.knn([f64::NAN, 0.0, 0.0], 0), Err(PointIndexError::NonFiniteQuery { axis: 0 }));
    let big = f64::from_bits(COORD_LIMIT.to_bits() + 1);
    assert_eq!(idx.knn([0.0, big, 0.0], 1), Err(PointIndexError::QueryOutOfRange { axis: 1 }));
    assert_eq!(idx.radius([0.0, 0.0, -big], 1.0), Err(PointIndexError::QueryOutOfRange { axis: 2 }));
    assert_eq!(idx.knn([COORD_LIMIT, -COORD_LIMIT, 0.0], 2).unwrap().len(), 2);
    for r in [-1.0, -f64::MIN_POSITIVE, f64::NAN, f64::INFINITY, -f64::INFINITY, big] {
        assert_eq!(idx.radius(o, r), Err(PointIndexError::BadRadius), "r = {r}");
    }
    assert_eq!(idx.radius([f64::NAN; 3], -1.0), Err(PointIndexError::NonFiniteQuery { axis: 0 }));
    // -0.0 is a valid radius (same as +0.0).
    assert_eq!(idx.radius(o, -0.0).unwrap(), vec![Neighbour { index: 0, dist2: 0.0 }]);
    assert_eq!(idx.radius(o, COORD_LIMIT).unwrap().len(), 2);
    assert_eq!(PointIndexError::BadRadius.to_string(), "bad_radius");
    assert_eq!(PointIndexError::BadK { k: 0 }.to_string(), "bad_k (0 not in 1..=1024)");
}

#[test]
fn build_input_domain_first_offender_wins() {
    let ok = PointBuffer::from_points(&[[0.0; 3]]);
    assert_eq!(PointIndex::build(&ok, 0), Err(PointIndexError::ZeroThreads));
    let short_y = PointBuffer::new(vec![0.0, 1.0], vec![0.0], vec![0.0, 1.0]);
    assert_eq!(PointIndex::build(&short_y, 1), Err(PointIndexError::ColumnLength { axis: 1, expected: 2, found: 1 }));
    let long_z = PointBuffer::new(vec![0.0], vec![0.0], vec![0.0, 1.0]);
    assert_eq!(PointIndex::build(&long_z, 1), Err(PointIndexError::ColumnLength { axis: 2, expected: 1, found: 2 }));
    // Threads are checked before the columns.
    assert_eq!(PointIndex::build(&short_y, 0), Err(PointIndexError::ZeroThreads));
    let big = f64::from_bits(COORD_LIMIT.to_bits() + 1);
    let bad = PointBuffer::new(vec![0.0, 0.0, big], vec![0.0, f64::NAN, 0.0], vec![0.0, 0.0, 0.0]);
    assert_eq!(PointIndex::build(&bad, 1), Err(PointIndexError::NonFinitePoint { index: 1, axis: 1 }));
    let bad = PointBuffer::new(vec![0.0, big, 0.0], vec![0.0, 0.0, f64::NAN], vec![0.0, 0.0, 0.0]);
    assert_eq!(PointIndex::build(&bad, 1), Err(PointIndexError::PointOutOfRange { index: 1, axis: 0 }));
    let bad = PointBuffer::new(vec![0.0, 0.0], vec![0.0, 0.0], vec![0.0, -f64::INFINITY]);
    assert_eq!(PointIndex::build(&bad, 1), Err(PointIndexError::NonFinitePoint { index: 1, axis: 2 }));
    // The limit itself is accepted.
    let edge = PointBuffer::from_points(&[[COORD_LIMIT, -COORD_LIMIT, 0.0], [-COORD_LIMIT, COORD_LIMIT, 0.0]]);
    let idx = PointIndex::build(&edge, 1).unwrap();
    let far = idx.knn([COORD_LIMIT; 3], 2).unwrap();
    assert!(far.iter().all(|n| n.dist2.is_finite()));
    assert_eq!(PointIndexError::NonFinitePoint { index: 3, axis: 2 }.to_string(), "non_finite_point (point 3, axis 2)");
}

#[test]
fn empty_index_answers_empty() {
    let buf = PointBuffer::default();
    let idx = PointIndex::build(&buf, 1).unwrap();
    assert!(idx.is_empty());
    assert_eq!(idx.depth(), 0);
    assert!(idx.knn([0.0; 3], 1024).unwrap().is_empty());
    assert!(idx.radius([0.0; 3], 1.0).unwrap().is_empty());
    assert!(idx.in_box(&Aabb::new([-1.0; 3], [1.0; 3]).unwrap()).unwrap().is_empty());
    // Errors are still reported on an empty index.
    assert_eq!(idx.knn([0.0; 3], 0), Err(PointIndexError::BadK { k: 0 }));
}

// ------------------------------------------------------------------ test-only brute force

fn brute_knn(pts: &[[f64; 3]], q: [f64; 3], k: usize) -> Vec<Neighbour> {
    let mut all = brute_radius(pts, q, f64::INFINITY);
    all.truncate(k);
    all
}

fn brute_radius(pts: &[[f64; 3]], q: [f64; 3], r2: f64) -> Vec<Neighbour> {
    let mut all: Vec<Neighbour> = pts
        .iter()
        .enumerate()
        .map(|(i, &p)| Neighbour { index: i as u64, dist2: dist2(p, q) })
        .filter(|n| n.dist2 <= r2)
        .collect();
    all.sort_by(|a, b| a.dist2.total_cmp(&b.dist2).then(a.index.cmp(&b.index)));
    all
}

/// Random clouds of every leaf-boundary size, with coordinates snapped to a coarse grid so
/// duplicates and distance ties are frequent, against the test-only brute force.
#[test]
fn randomised_clouds_equal_brute_force() {
    let mut g = SplitMix(99);
    for round in 0..60u64 {
        let n = match round % 6 {
            0 => (g.next() % 40) as usize,
            1 => 16 * (1 + (g.next() % 8) as usize) + (g.next() % 3) as usize,
            _ => (g.next() % 3000) as usize,
        };
        let grid = [1.0, 0.25, 2f64.powi(-20), 1e-300][(round % 4) as usize];
        let snap = |x: f64| (x * 8.0).floor() * grid;
        let pts: Vec<[f64; 3]> = (0..n).map(|_| [snap(g.unit()), snap(g.unit()), snap(g.unit())]).collect();
        let buf = PointBuffer::from_points(&pts);
        let idx = PointIndex::build(&buf, 1).unwrap();
        for _ in 0..20 {
            let q = if n > 0 && g.next() % 2 == 0 {
                pts[(g.next() % n as u64) as usize]
            } else {
                [snap(g.unit()) + grid * 0.5, snap(g.unit()), snap(g.unit())]
            };
            let k = [1usize, 2, 7, 16, 17, 64, 1024][(g.next() % 7) as usize];
            assert_bits_eq(&idx.knn(q, k).unwrap(), &brute_knn(&pts, q, k), &format!("round {round} knn {k}"));
            let r = grid * (g.next() % 4) as f64;
            assert_bits_eq(&idx.radius(q, r).unwrap(), &brute_radius(&pts, q, r * r), &format!("round {round} r {r}"));
            let lo = [snap(g.unit()), snap(g.unit()), snap(g.unit())];
            let hi: [f64; 3] = std::array::from_fn(|k| lo[k] + grid * (g.next() % 4) as f64);
            let want: Vec<u64> = pts
                .iter()
                .enumerate()
                .filter(|(_, p)| (0..3).all(|k| lo[k] <= p[k] && p[k] <= hi[k]))
                .map(|(i, _)| i as u64)
                .collect();
            assert_eq!(idx.in_box(&Aabb::new(lo, hi).unwrap()).unwrap(), want, "round {round} box");
        }
    }
}

#[test]
fn tree_shape_follows_the_pinned_rules() {
    // Leaf depth: smallest d with ceil(n / 2^d) <= 16.
    for (n, d) in [(0u64, 0u32), (1, 0), (16, 0), (17, 1), (32, 1), (33, 2), (100_000, 13), (1 << 32, 28)] {
        assert_eq!(leaf_depth(n), d, "n = {n}");
    }
    // Leaves hold 8..=16 points once n > 16, and each leaf range is ascending.
    let mut g = SplitMix(7);
    let pts: Vec<[f64; 3]> = (0..1000).map(|_| [g.unit(), g.unit() * 4.0, g.unit() * 2.0]).collect();
    let buf = PointBuffer::from_points(&pts);
    let idx = PointIndex::build(&buf, 1).unwrap();
    assert_eq!(idx.depth(), 6);
    // The root splits on y, the largest extent.
    assert_eq!(idx.axes()[0], 1);
    let mut sorted: Vec<u32> = idx.perm().to_vec();
    sorted.sort_unstable();
    assert_eq!(sorted, (0..1000).collect::<Vec<u32>>());
    // Leaf j at depth L: halve (start, count) by the bits of j, left child = count / 2.
    let mut seen = 0;
    for j in 0..(1usize << idx.depth()) {
        let (mut s, mut c) = (0usize, 1000usize);
        for b in (0..idx.depth()).rev() {
            let h = c / 2;
            if (j >> b) & 1 == 1 {
                s += h;
                c -= h;
            } else {
                c = h;
            }
        }
        assert!((15..=16).contains(&c), "leaf {j} holds {c}");
        assert_eq!(s, seen);
        seen += c;
        assert!(idx.perm()[s..s + c].windows(2).all(|w| w[0] < w[1]), "leaf {j} ascending");
    }
    assert_eq!(seen, 1000);
}
