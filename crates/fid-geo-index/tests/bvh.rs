//! Acceptance criteria of spec node GEO.INDEX.BVH (AC-1 to AC-4) plus the input-domain rules.
//!
//! Expected results come from `fixtures/geo/index/`, written by the independent stdlib-Python
//! reference `fixtures/geo/index/make_bvh_fixtures.py` (brute force over every triangle with a
//! bit-exact model of the pinned tests, plus an exact `fractions` check); none come from this
//! crate. The sphere mesh and the query stream are regenerated here from the pinned generators
//! and checked against the fixture's `mesh_digest` and `query_digest` before use. The error
//! cases at the end are hand-labelled from the leaf's Design.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::ops::ControlFlow;

use fid_geo_frame::xform::{Rigid, Similarity, Transform, XformError, IDENTITY};
use fid_geo_index::{BvhError, PrimitiveBvh, QueryField, COORD_LIMIT};
use fid_geo_types::{Aabb, FrameRef, Mesh};
use fid_kci::kci::{sha256, to_hex, Sha256};

const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/geo/index/");

fn read(rel: &str) -> String {
    std::fs::read_to_string(format!("{ROOT}{rel}")).unwrap()
}

fn hexf(s: &str) -> f64 {
    f64::from_bits(u64::from_str_radix(s, 16).unwrap())
}

fn hexf3(s: &[&str]) -> [f64; 3] {
    [hexf(s[0]), hexf(s[1]), hexf(s[2])]
}

// ------------------------------------------------------------------ pinned generators

const N_SUB: u64 = 71;
const N_QUERIES: usize = 10_000;
const N_NEAREST: usize = 1_000;
const SEED: u64 = 0x6765_6F2E_6276_6801;

fn sphere_mesh() -> (Vec<[f64; 3]>, Vec<[u64; 3]>) {
    let sqrt = |x: f64| fid_num::f::sqrt(x).unwrap();
    let phi = (1.0 + sqrt(5.0)) / 2.0;
    let v: [[f64; 3]; 12] = [
        [-1.0, phi, 0.0],
        [1.0, phi, 0.0],
        [-1.0, -phi, 0.0],
        [1.0, -phi, 0.0],
        [0.0, -1.0, phi],
        [0.0, 1.0, phi],
        [0.0, -1.0, -phi],
        [0.0, 1.0, -phi],
        [phi, 0.0, -1.0],
        [phi, 0.0, 1.0],
        [-phi, 0.0, -1.0],
        [-phi, 0.0, 1.0],
    ];
    let f: [[usize; 3]; 20] = [
        [0, 11, 5],
        [0, 5, 1],
        [0, 1, 7],
        [0, 7, 10],
        [0, 10, 11],
        [1, 5, 9],
        [5, 11, 4],
        [11, 10, 2],
        [10, 7, 6],
        [7, 1, 8],
        [3, 9, 4],
        [3, 4, 2],
        [3, 2, 6],
        [3, 6, 8],
        [3, 8, 9],
        [4, 9, 5],
        [2, 4, 11],
        [6, 2, 10],
        [8, 6, 7],
        [9, 8, 1],
    ];
    let n = N_SUB;
    let fn_ = n as f64;
    let mut verts = Vec::new();
    let mut tris = Vec::new();
    for face in f {
        let (a, b, c) = (v[face[0]], v[face[1]], v[face[2]]);
        let base = verts.len() as u64;
        for i in 0..=n {
            for j in 0..=(n - i) {
                let (wa, wb, wc) = ((n - i - j) as f64, i as f64, j as f64);
                let p: [f64; 3] = std::array::from_fn(|k| ((a[k] * wa + b[k] * wb) + c[k] * wc) / fn_);
                let len = sqrt((p[0] * p[0] + p[1] * p[1]) + p[2] * p[2]);
                verts.push([p[0] / len, p[1] / len, p[2] / len]);
            }
        }
        let idx = |i: u64, j: u64| base + i * (n + 1) - (i * (i.wrapping_sub(1))) / 2 + j;
        for i in 0..n {
            for j in 0..(n - i) {
                tris.push([idx(i, j), idx(i + 1, j), idx(i, j + 1)]);
                if j + 1 < n - i {
                    tris.push([idx(i + 1, j), idx(i + 1, j + 1), idx(i, j + 1)]);
                }
            }
        }
    }
    (verts, tris)
}

fn payload_of(t: u64) -> u64 {
    t.wrapping_mul(2_654_435_761) & 0xffff_ffff
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
    fn uni(&mut self, a: f64, b: f64) -> f64 {
        let u = (self.next() >> 11) as f64 * (1.0 / 9_007_199_254_740_992.0);
        a + (b - a) * u
    }
}

#[derive(Debug, Clone, Copy)]
enum Query {
    Sphere([f64; 3], f64),
    Box([f64; 3], [f64; 3]),
    Ray([f64; 3], [f64; 3], f64, f64),
}

fn queries() -> (Vec<Query>, Vec<[f64; 3]>) {
    let mut g = SplitMix(SEED);
    let mut qs = Vec::new();
    for i in 0..N_QUERIES {
        match i % 3 {
            0 => {
                let c = [g.uni(-1.5, 1.5), g.uni(-1.5, 1.5), g.uni(-1.5, 1.5)];
                let mut r = g.uni(0.0, 0.3);
                if i % 30 == 0 {
                    r = 0.0;
                }
                qs.push(Query::Sphere(c, r));
            }
            1 => {
                let c = [g.uni(-1.5, 1.5), g.uni(-1.5, 1.5), g.uni(-1.5, 1.5)];
                let h = [g.uni(0.0, 0.25), g.uni(0.0, 0.25), g.uni(0.0, 0.25)];
                qs.push(Query::Box(std::array::from_fn(|k| c[k] - h[k]), std::array::from_fn(|k| c[k] + h[k])));
            }
            _ => {
                let o = [g.uni(-2.0, 2.0), g.uni(-2.0, 2.0), g.uni(-2.0, 2.0)];
                let mut d = [g.uni(-1.0, 1.0), g.uni(-1.0, 1.0), g.uni(-1.0, 1.0)];
                if i % 7 == 0 {
                    d[0] = 0.0;
                }
                if i % 11 == 0 {
                    d[1] = 0.0;
                }
                let (a, b) = (g.uni(-1.0, 3.0), g.uni(-1.0, 3.0));
                let (t0, t1) = if a <= b { (a, b) } else { (b, a) };
                qs.push(Query::Ray(o, d, t0, t1));
            }
        }
    }
    let pts = (0..N_NEAREST).map(|_| [g.uni(-1.5, 1.5), g.uni(-1.5, 1.5), g.uni(-1.5, 1.5)]).collect();
    (qs, pts)
}

fn mesh_digest(verts: &[[f64; 3]], tris: &[[u64; 3]]) -> String {
    let mut h = Sha256::new();
    h.update(&(verts.len() as u64).to_le_bytes());
    for v in verts {
        for x in v {
            h.update(&x.to_bits().to_le_bytes());
        }
    }
    h.update(&(tris.len() as u64).to_le_bytes());
    for t in tris {
        for i in t {
            h.update(&i.to_le_bytes());
        }
    }
    to_hex(&h.finish())
}

fn query_digest(qs: &[Query], pts: &[[f64; 3]]) -> String {
    let mut h = Sha256::new();
    let mut tagged: Vec<(&[u8], Vec<f64>)> = Vec::new();
    for q in qs {
        tagged.push(match *q {
            Query::Sphere(c, r) => (&b"sphere"[..], vec![c[0], c[1], c[2], r]),
            Query::Box(lo, hi) => (&b"box"[..], vec![lo[0], lo[1], lo[2], hi[0], hi[1], hi[2]]),
            Query::Ray(o, d, t0, t1) => (&b"ray"[..], vec![o[0], o[1], o[2], d[0], d[1], d[2], t0, t1]),
        });
    }
    for (kind, xs) in &tagged {
        h.update(kind);
        for x in xs {
            h.update(&x.to_bits().to_le_bytes());
        }
    }
    for p in pts {
        for x in p {
            h.update(&x.to_bits().to_le_bytes());
        }
    }
    to_hex(&h.finish())
}

fn run(bvh: &PrimitiveBvh, q: &Query) -> Vec<u64> {
    match *q {
        Query::Sphere(c, r) => bvh.sphere(c, r),
        Query::Box(lo, hi) => bvh.box_query(lo, hi),
        Query::Ray(o, d, t0, t1) => bvh.ray(o, d, t0, t1),
    }
    .unwrap()
}

fn kind(q: &Query) -> &'static str {
    match q {
        Query::Sphere(..) => "sphere",
        Query::Box(..) => "box",
        Query::Ray(..) => "ray",
    }
}

fn result_bytes(ids: &[u64]) -> Vec<u8> {
    let mut b = (ids.len() as u64).to_le_bytes().to_vec();
    for t in ids {
        b.extend_from_slice(&t.to_le_bytes());
    }
    b
}

struct SphereFixture {
    header: std::collections::BTreeMap<String, String>,
    q: Vec<(String, usize, String)>,
    nearest: Vec<(u64, u64)>,
    visit: Vec<Vec<(u32, u64)>>,
}

fn sphere_fixture() -> SphereFixture {
    let text = read("mesh_sphere_1e5/expected.txt");
    let mut fx = SphereFixture { header: Default::default(), q: Vec::new(), nearest: Vec::new(), visit: Vec::new() };
    for line in text.lines().filter(|l| !l.starts_with('#')) {
        let f: Vec<&str> = line.split(' ').collect();
        match f[0] {
            "q" => {
                assert_eq!(f[1].parse::<usize>().unwrap(), fx.q.len());
                fx.q.push((f[2].to_owned(), f[3].parse().unwrap(), f[4].to_owned()));
            }
            "nearest" => fx.nearest.push((f[2].parse().unwrap(), u64::from_str_radix(f[3], 16).unwrap())),
            "visit" => fx.visit.push(
                f[2..]
                    .iter()
                    .map(|p| {
                        let (n, lb) = p.split_once(':').unwrap();
                        (n.parse().unwrap(), u64::from_str_radix(lb, 16).unwrap())
                    })
                    .collect(),
            ),
            k => {
                fx.header.insert(k.to_owned(), f[1].to_owned());
            }
        }
    }
    fx
}

fn sphere_bvh(threads: usize) -> (PrimitiveBvh, SphereFixture) {
    let fx = sphere_fixture();
    let (verts, tris) = sphere_mesh();
    assert_eq!(
        mesh_digest(&verts, &tris),
        fx.header["mesh_digest"],
        "pinned mesh generator differs from the reference"
    );
    assert_eq!(tris.len().to_string(), fx.header["triangles"]);
    let frame = FrameRef::from_bytes(std::array::from_fn(|i| i as u8));
    let mesh = Mesh::from_points(frame, &verts, &tris).unwrap();
    let payload: Vec<u64> = (0..tris.len() as u64).map(payload_of).collect();
    (PrimitiveBvh::build(&mesh, &payload, threads).unwrap(), fx)
}

// ------------------------------------------------------------------ AC-1, AC-3

#[test]
fn ac1_seeded_queries_equal_brute_force_and_ac3_digests() {
    let (bvh, fx) = sphere_bvh(1);
    let (qs, pts) = queries();
    assert_eq!(query_digest(&qs, &pts), fx.header["query_digest"], "pinned query stream differs");
    assert_eq!(fx.q.len(), N_QUERIES);

    let mut total = Sha256::new();
    let mut nonempty = 0;
    for (i, (q, (k, count, hex16))) in qs.iter().zip(&fx.q).enumerate() {
        let ids = run(&bvh, q);
        assert!(ids.windows(2).all(|w| w[0] < w[1]), "query {i}: not strictly ascending");
        let rb = result_bytes(&ids);
        total.update(&rb);
        assert_eq!(kind(q), k.as_str(), "query {i}");
        assert_eq!(ids.len(), *count, "query {i} ({k}): count");
        assert_eq!(&to_hex(&sha256(&rb))[..16], hex16.as_str(), "query {i} ({k}): ids");
        if !ids.is_empty() {
            nonempty += 1;
        }
    }
    assert!(nonempty > 1000, "the stream must exercise non-empty results ({nonempty})");
    // AC-3: this digest is compared on x86-64 and ARM64 (test matrix) and wasm32 (CI job).
    assert_eq!(to_hex(&total.finish()), fx.header["result_digest"]);

    assert_eq!(fx.nearest.len(), N_NEAREST);
    for (j, (p, &(t, d2))) in pts.iter().zip(&fx.nearest).enumerate() {
        let got = bvh.nearest_box(*p).unwrap();
        assert_eq!((got.triangle, got.distance_sq.to_bits()), (t, d2), "nearest {j}");
    }
}

#[test]
fn ac3_hierarchy_is_byte_identical_for_any_thread_count() {
    let (one, fx) = sphere_bvh(1);
    let (eight, _) = sphere_bvh(8);
    let a = one.to_bytes().unwrap();
    let b = eight.to_bytes().unwrap();
    assert_eq!(a, b);
    assert_eq!(a.len().to_string(), fx.header["bvh_bytes"]);
    assert_eq!(one.nodes().len().to_string(), fx.header["nodes"]);
    assert_eq!(to_hex(&sha256(&a)), fx.header["bvh_digest"], "hierarchy differs from the reference build model");
    // Leaf sizes and payload round trip.
    for n in one.nodes() {
        assert!(n.count() as usize <= fid_geo_index::LEAF_SIZE);
        if n.is_leaf() {
            assert!(n.count() >= 1);
        }
    }
    assert_eq!(one.payload(5), Some(payload_of(5)));
    assert_eq!(one.payload(one.triangle_count()), None);
}

// ------------------------------------------------------------------ AC-4

#[test]
fn ac4_ordered_visit_is_non_decreasing_and_matches_reference() {
    let (bvh, fx) = sphere_bvh(1);
    let (_, pts) = queries();
    let leaves = bvh.nodes().iter().filter(|n| n.is_leaf()).count();
    for (j, p) in pts.iter().take(50).enumerate() {
        let mut seen: Vec<(f64, u32)> = Vec::new();
        bvh.ordered_visit(*p, |leaf| {
            seen.push((leaf.lower_bound_sq(), leaf.node()));
            ControlFlow::Continue(())
        })
        .unwrap();
        assert_eq!(seen.len(), leaves, "point {j}: every leaf once");
        for w in seen.windows(2) {
            assert!(w[0].0 <= w[1].0, "point {j}: lower bounds decrease");
            if w[0].0 == w[1].0 {
                assert!(w[0].1 < w[1].1, "point {j}: tie not broken by node index");
            }
        }
        if let Some(want) = fx.visit.get(j) {
            let got: Vec<(u32, u64)> = seen.iter().take(want.len()).map(|&(lb, n)| (n, lb.to_bits())).collect();
            assert_eq!(&got, want, "point {j}: first leaves differ from the reference order");
        }
    }
    assert_eq!(fx.visit.len(), 5);
    // Early stop.
    let mut calls = 0;
    bvh.ordered_visit(pts[0], |_| {
        calls += 1;
        ControlFlow::Break(())
    })
    .unwrap();
    assert_eq!(calls, 1);
}

// ------------------------------------------------------------------ AC-2

fn ids(s: &str) -> Vec<u64> {
    if s == "-" {
        Vec::new()
    } else {
        s.split(',').map(|x| x.parse().unwrap()).collect()
    }
}

fn grazing_bvh() -> (PrimitiveBvh, Vec<String>) {
    let text = read("grazing_rays/cases.txt");
    let mut frame = [0u8; 16];
    let mut verts = Vec::new();
    let mut tris = Vec::new();
    let mut payload = Vec::new();
    let mut cases = Vec::new();
    for line in text.lines().filter(|l| !l.starts_with('#')) {
        let f: Vec<&str> = line.split(' ').collect();
        match f[0] {
            "frame" => {
                for (i, b) in frame.iter_mut().enumerate() {
                    *b = u8::from_str_radix(&f[1][2 * i..2 * i + 2], 16).unwrap();
                }
            }
            "vertex" => verts.push(hexf3(&f[2..5])),
            "tri" => {
                tris.push([f[2].parse().unwrap(), f[3].parse().unwrap(), f[4].parse().unwrap()]);
                payload.push(f[5].parse().unwrap());
            }
            _ => cases.push(line.to_owned()),
        }
    }
    let mesh = Mesh::from_points(FrameRef::from_bytes(frame), &verts, &tris).unwrap();
    (PrimitiveBvh::build(&mesh, &payload, 1).unwrap(), cases)
}

#[test]
fn ac2_grazing_queries_miss_nothing() {
    let (bvh, cases) = grazing_bvh();
    assert_eq!(bvh.payload(0), Some(1000));
    let mut rays = 0;
    for line in &cases {
        let f: Vec<&str> = line.split(' ').collect();
        let tail = |key: &str| ids(f.iter().find_map(|x| x.strip_prefix(key)).unwrap());
        let (expect, exact) = (tail("expect="), tail("exact="));
        let got = match f[0] {
            "ray" => {
                rays += 1;
                bvh.ray(hexf3(&f[1..4]), hexf3(&f[4..7]), hexf(f[7]), hexf(f[8]))
            }
            "box" => bvh.box_query(hexf3(&f[1..4]), hexf3(&f[4..7])),
            "sphere" => bvh.sphere(hexf3(&f[1..4]), hexf(f[4])),
            k => panic!("unknown case {k}"),
        }
        .unwrap();
        for t in &exact {
            assert!(got.contains(t), "{line}: exact hit {t} missing");
        }
        assert_eq!(got, expect, "{line}");
    }
    assert!(rays >= 20);
}

#[test]
fn rays_from_another_frame_map_through_geo_frame_transforms() {
    let (bvh, _) = grazing_bvh();
    let base = bvh.ray([-1.0, 0.5, 0.0], [1.0, 0.0, 0.0], 0.0, 10.0).unwrap();
    assert!(!base.is_empty());
    let id = Transform::Rigid(Rigid::new(IDENTITY, [0.0; 3]).unwrap());
    assert_eq!(bvh.ray_transformed(&id, [-1.0, 0.5, 0.0], [1.0, 0.0, 0.0], 0.0, 10.0).unwrap(), base);
    let shift = Transform::Rigid(Rigid::new(IDENTITY, [10.0, 0.0, 0.0]).unwrap());
    assert_eq!(bvh.ray_transformed(&shift, [-11.0, 0.5, 0.0], [1.0, 0.0, 0.0], 0.0, 10.0).unwrap(), base);
    let scale = Transform::Similarity(Similarity::new(Rigid::new(IDENTITY, [0.0; 3]).unwrap(), 2.0).unwrap());
    assert_eq!(bvh.ray_transformed(&scale, [-0.5, 0.25, 0.0], [0.5, 0.0, 0.0], 0.0, 10.0).unwrap(), base);
    // Overflow in the mapping is refused, not queried.
    let huge = Transform::Similarity(Similarity::new(Rigid::new(IDENTITY, [0.0; 3]).unwrap(), 1e300).unwrap());
    assert_eq!(
        bvh.ray_transformed(&huge, [1e10, 0.0, 0.0], [1.0, 0.0, 0.0], 0.0, 1.0),
        Err(BvhError::Transform(XformError::NonFinite))
    );
    // Non-finite input is refused before mapping.
    assert_eq!(
        bvh.ray_transformed(&id, [0.0; 3], [1.0, 0.0, 0.0], 0.0, f64::INFINITY),
        Err(BvhError::NonFiniteQuery { field: QueryField::TMax })
    );
    // A mapped result beyond the coordinate limit is refused at the range boundary.
    let big = Transform::Similarity(Similarity::new(Rigid::new(IDENTITY, [0.0; 3]).unwrap(), 4.0).unwrap());
    assert_eq!(
        bvh.ray_transformed(&big, [COORD_LIMIT, 0.0, 0.0], [1.0, 0.0, 0.0], 0.0, 1.0),
        Err(BvhError::QueryOutOfRange { field: QueryField::Origin })
    );
}

// ------------------------------------------------------------------ input domain (hand-labelled)

const F0: FrameRef = FrameRef::from_bytes([1; 16]);

fn unit_tri(dx: f64) -> [[f64; 3]; 3] {
    [[dx, 0.0, 0.0], [dx + 1.0, 0.0, 0.0], [dx, 1.0, 0.0]]
}

#[test]
fn build_refuses_bad_meshes_with_typed_errors() {
    let tri = unit_tri(0.0);
    let b =
        |tris: &[[[f64; 3]; 3]], payload: &[u64], threads| PrimitiveBvh::build_triangles(F0, tris, payload, threads);
    assert_eq!(b(&[tri], &[0], 0), Err(BvhError::ZeroThreads));
    assert_eq!(b(&[], &[], 1), Err(BvhError::EmptyMesh));
    assert_eq!(b(&[tri], &[], 1), Err(BvhError::PayloadLength { expected: 1, found: 0 }));
    let empty = Mesh::from_points(F0, &[[0.0; 3]], &[]).unwrap();
    assert_eq!(PrimitiveBvh::build(&empty, &[], 1), Err(BvhError::EmptyMesh));

    let mut nan = [tri, tri];
    nan[1][2][1] = f64::NAN;
    assert_eq!(b(&nan, &[0, 1], 1), Err(BvhError::NonFinitePrimitive { triangle: 1, corner: 2, axis: 1 }));
    let mut inf = [tri];
    inf[0][0][2] = f64::NEG_INFINITY;
    assert_eq!(b(&inf, &[0], 1), Err(BvhError::NonFinitePrimitive { triangle: 0, corner: 0, axis: 2 }));
    let mut far = [tri];
    far[0][1][0] = COORD_LIMIT * 2.0;
    assert_eq!(b(&far, &[0], 1), Err(BvhError::PrimitiveOutOfRange { triangle: 0, corner: 1, axis: 0 }));
    let mut edge = [tri];
    edge[0][1][0] = COORD_LIMIT;
    edge[0][2][0] = -COORD_LIMIT;
    assert!(b(&edge, &[0], 1).is_ok(), "the limit itself is accepted");

}

#[test]
fn degenerate_triangles_are_accepted_and_found() {
    let tri = unit_tri(0.0);
    // Two equal corners (also -0.0 against +0.0), all three corners equal, collinear corners.
    let dup = [[0.0, 0.0, 0.0], [-0.0, 0.0, 0.0], [1.0, 1.0, 1.0]];
    let point = [[3.0, 3.0, 3.0]; 3];
    let line = [[0.0, 0.0, 0.0], [1.0, 1.0, 1.0], [2.0, 2.0, 2.0]];
    let bvh = PrimitiveBvh::build_triangles(F0, &[tri, dup, point, line], &[0, 1, 2, 3], 1).unwrap();
    assert_eq!(bvh.sphere([1.0, 1.0, 1.0], 0.0).unwrap(), vec![1, 3]);
    assert_eq!(bvh.sphere([3.0, 3.0, 3.0], 0.0).unwrap(), vec![2]);
    assert_eq!(bvh.ray([3.0, 3.0, -1.0], [0.0, 0.0, 1.0], 0.0, 10.0).unwrap(), vec![2]);
    assert_eq!(bvh.box_query([0.5, 0.5, 0.5], [0.5, 0.5, 0.5]).unwrap(), vec![1, 3]);
    // A repeated vertex index in a measured mesh (kept by GEO.TYPES.MESH) is indexed too.
    let mesh = Mesh::from_points(F0, &[[0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]], &[[0, 1, 2], [0, 2, 2]]).unwrap();
    let bvh = PrimitiveBvh::build(&mesh, &[7, 8], 1).unwrap();
    assert_eq!(bvh.box_query([0.0, 0.5, 0.0], [0.0, 0.5, 0.0]).unwrap(), vec![0, 1]);
    assert_eq!(bvh.payload(1), Some(8));
}

// ------------------------------------------------------------------ brute force through real hierarchies

const C4: f64 = 4.0 * f64::EPSILON;
const TINY: f64 = f64::MIN_POSITIVE;

fn tmax(a: f64, b: f64) -> f64 {
    if b.total_cmp(&a).is_gt() {
        b
    } else {
        a
    }
}

fn tmin(a: f64, b: f64) -> f64 {
    if b.total_cmp(&a).is_lt() {
        b
    } else {
        a
    }
}

/// Test-local restatement of the pinned per-box tests (NODE.md Design), applied to every box.
#[allow(clippy::needless_range_loop)]
fn brute(b: &Aabb, q: &Query) -> bool {
    let (lo, hi) = (b.min(), b.max());
    let gap = |c: f64, l: f64, h: f64| if c < l { l - c } else if c > h { c - h } else { 0.0 };
    match *q {
        Query::Box(qlo, qhi) => (0..3).all(|k| lo[k] <= qhi[k] && qlo[k] <= hi[k]),
        Query::Sphere(c, r) => {
            let g: [f64; 3] = std::array::from_fn(|k| gap(c[k], lo[k], hi[k]));
            (g[0] * g[0] + g[1] * g[1]) + g[2] * g[2] <= r * r * (1.0 + C4) + TINY
        }
        Query::Ray(o, d, t0, t1) => {
            let up = |t: f64| if t.is_finite() { t + (t.abs() * C4 + TINY) } else { t };
            let down = |t: f64| if t.is_finite() { t - (t.abs() * C4 + TINY) } else { t };
            let (mut a, mut z) = (t0, t1);
            for k in 0..3 {
                if d[k] == 0.0 {
                    if !(lo[k]..=hi[k]).contains(&o[k]) {
                        return false;
                    }
                    continue;
                }
                let (ta, tb) = ((lo[k] - o[k]) / d[k], (hi[k] - o[k]) / d[k]);
                let (near, far) = if ta <= tb { (ta, tb) } else { (tb, ta) };
                a = tmax(a, down(near));
                z = tmin(z, up(far));
            }
            a <= z
        }
    }
}

fn check_against_brute(bvh: &PrimitiveBvh, qs: &[Query]) {
    let n = bvh.triangle_count();
    for (i, q) in qs.iter().enumerate() {
        let want: Vec<u64> = (0..n).filter(|&t| brute(&bvh.triangle_aabb(t).unwrap(), q)).collect();
        assert_eq!(run(bvh, q), want, "query {i}: {q:?}");
    }
}

fn mixed_queries(g: &mut SplitMix, count: usize, span: f64, extra: &[Query]) -> Vec<Query> {
    let mut qs = extra.to_vec();
    for i in 0..count {
        let p = [g.uni(-span, span), g.uni(-span, span), g.uni(-span, span)];
        let s = g.uni(0.0, span);
        qs.push(match i % 3 {
            0 => Query::Sphere(p, s),
            1 => Query::Box(p.map(|x| x - s), p.map(|x| x + s)),
            _ => {
                let mut d = [g.uni(-1.0, 1.0), g.uni(-1.0, 1.0), g.uni(-1.0, 1.0)];
                if i % 4 == 0 {
                    d[i % 3] = 0.0;
                }
                Query::Ray(p, d, -s, s)
            }
        });
    }
    qs
}

#[test]
fn identical_triangles_match_brute_force() {
    // 200 copies of one triangle: every split is by triangle index alone.
    let tris = vec![[[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]]; 200];
    let payload: Vec<u64> = (0..200).collect();
    let bvh = PrimitiveBvh::build_triangles(F0, &tris, &payload, 1).unwrap();
    assert!(bvh.nodes().len() > 1);
    let all: Vec<u64> = (0..200).collect();
    assert_eq!(bvh.sphere([0.25, 0.25, 0.0], 0.0).unwrap(), all);
    assert_eq!(bvh.nearest_box([5.0, 5.0, 5.0]).unwrap().triangle, 0);
    let extra = [
        Query::Ray([-1.0, 0.0, 0.0], [1.0, 0.0, 0.0], 0.0, 1.0),
        Query::Box([1.0, 1.0, 0.0], [2.0, 2.0, 0.0]),
        Query::Sphere([1.0, 1.0, 1.0], 1.0),
    ];
    let mut g = SplitMix(7);
    check_against_brute(&bvh, &mixed_queries(&mut g, 300, 2.0, &extra));
}

#[test]
fn extreme_coordinates_match_brute_force() {
    let l = COORD_LIMIT;
    let sub = f64::from_bits(1); // smallest subnormal
    let mut tris = vec![
        [[l, l, l], [-l, l, l], [l, -l, -l]],
        [[-l, -l, -l], [-l, -l, -l], [-l, -l, -l]],
        [[l, 0.0, 0.0], [l, sub, 0.0], [l, 0.0, sub]],
        [[0.0, 0.0, 0.0], [sub, 0.0, 0.0], [0.0, sub, 0.0]],
        [[-sub, -sub, -sub], [0.0, 0.0, 0.0], [sub, sub, sub]],
        [[TINY, TINY, 0.0], [2.0 * TINY, TINY, 0.0], [TINY, 2.0 * TINY, 0.0]],
    ];
    let mut g = SplitMix(11);
    for _ in 0..60 {
        let c = [g.uni(-1.0, 1.0), g.uni(-1.0, 1.0), g.uni(-1.0, 1.0)];
        let e = g.uni(0.0, 0.1);
        tris.push([c, [c[0] + e, c[1], c[2]], [c[0], c[1] + e, c[2] + e]]);
    }
    let payload: Vec<u64> = (0..tris.len() as u64).collect();
    let bvh = PrimitiveBvh::build_triangles(F0, &tris, &payload, 2).unwrap();
    let extra = [
        Query::Sphere([0.0; 3], 0.0),
        Query::Sphere([sub, sub, sub], 0.0),
        Query::Sphere([l, l, l], l),
        Query::Sphere([-l, -l, -l], 0.0),
        Query::Box([-l, -l, -l], [l, l, l]),
        Query::Box([sub; 3], [sub; 3]),
        Query::Box([l, 0.0, 0.0], [l, 0.0, 0.0]),
        Query::Ray([0.0; 3], [1.0, 1.0, 1.0], -l, l),
        Query::Ray([-l, 0.0, 0.0], [l, sub, 0.0], 0.0, 2.0),
        Query::Ray([0.0; 3], [sub, sub, 0.0], 0.0, 1.0),
        Query::Ray([0.0; 3], [l, sub, sub], -l, l),
        Query::Ray([l, l, l], [-1.0, 0.0, 0.0], 0.0, l),
        Query::Ray([TINY, TINY, 0.0], [0.0, 0.0, -sub], -1.0, 1.0),
    ];
    check_against_brute(&bvh, &mixed_queries(&mut g, 300, 1.0, &extra));
}

fn nf<T>(field: QueryField) -> Result<T, BvhError> {
    Err(BvhError::NonFiniteQuery { field })
}

fn oor<T>(field: QueryField) -> Result<T, BvhError> {
    Err(BvhError::QueryOutOfRange { field })
}

#[test]
fn queries_refuse_bad_input_with_typed_errors() {
    let tris: Vec<[[f64; 3]; 3]> = (0..20).map(|i| unit_tri(i as f64 * 2.0)).collect();
    let payload: Vec<u64> = (0..20).collect();
    let bvh = PrimitiveBvh::build_triangles(F0, &tris, &payload, 3).unwrap();

    assert_eq!(bvh.sphere([f64::NAN, 0.0, 0.0], 1.0), nf(QueryField::Point));
    assert_eq!(bvh.sphere([0.0; 3], f64::INFINITY), nf(QueryField::Radius));
    assert_eq!(bvh.sphere([0.0; 3], COORD_LIMIT * 2.0), oor(QueryField::Radius));
    assert_eq!(bvh.sphere([0.0; 3], -1.0), Err(BvhError::NegativeRadius));
    assert_eq!(bvh.sphere([0.0; 3], -0.0).unwrap(), vec![0]);

    assert_eq!(bvh.box_query([0.0; 3], [1.0, 1.0, f64::NAN]), nf(QueryField::BoxMax));
    assert_eq!(bvh.box_query([-COORD_LIMIT * 2.0, 0.0, 0.0], [1.0; 3]), oor(QueryField::BoxMin));
    assert_eq!(bvh.box_query([0.0, 0.0, 1.0], [1.0, 1.0, 0.0]), Err(BvhError::InvertedQueryBox { axis: 2 }));
    let b = Aabb::new([2.0, 0.0, 0.0], [3.0, 0.0, 0.0]).unwrap();
    assert_eq!(bvh.aabb(&b).unwrap(), vec![1]);

    let (o, d) = ([0.0, 0.5, 0.0], [1.0, 0.0, 0.0]);
    assert_eq!(bvh.ray(o, [0.0, -0.0, 0.0], 0.0, 1.0), Err(BvhError::ZeroDirection));
    assert_eq!(bvh.ray(o, d, 0.0, f64::INFINITY), nf(QueryField::TMax));
    assert_eq!(bvh.ray(o, d, f64::NEG_INFINITY, 1.0), nf(QueryField::TMin));
    assert_eq!(bvh.ray(o, d, 0.0, COORD_LIMIT * 2.0), oor(QueryField::TMax));
    assert_eq!(bvh.ray(o, [COORD_LIMIT * 2.0, 0.0, 0.0], 0.0, 1.0), oor(QueryField::Direction));
    assert_eq!(bvh.ray([f64::NAN; 3], d, 0.0, 1.0), nf(QueryField::Origin));
    assert_eq!(bvh.ray(o, d, 2.0, 1.0), Err(BvhError::InvertedRange));
    assert_eq!(bvh.ray(o, d, 0.0, COORD_LIMIT).unwrap(), payload);

    assert_eq!(bvh.nearest_box([0.0, f64::INFINITY, 0.0]), nf(QueryField::Point));
    assert_eq!(bvh.ordered_visit([f64::NAN; 3], |_| ControlFlow::Continue(())), nf(QueryField::Point));
    let n = bvh.nearest_box([5.0, 0.25, 0.0]).unwrap();
    assert_eq!((n.triangle, n.distance_sq), (2, 0.0));
    // Equal box distances: the smallest triangle index wins (boxes [2,3] and [4,5] at x = 3.5).
    let n = bvh.nearest_box([3.5, 0.0, 0.0]).unwrap();
    assert_eq!((n.triangle, n.distance_sq), (1, 0.25));
    assert_eq!(BvhError::ZeroDirection.code(), "zero_direction");
    assert_eq!(BvhError::NonFiniteQuery { field: QueryField::TMax }.to_string(), "non_finite_query (t_max)");
}
