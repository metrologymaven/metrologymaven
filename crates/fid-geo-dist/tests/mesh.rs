//! Acceptance criteria of spec node GEO.DIST.MESH (AC-1 to AC-6), the fuzz corpus and the
//! input-domain rules.
//!
//! Expected values come from `fixtures/geo/dist/`, written by the independent stdlib-Python
//! reference `tools/fixtures/geo_dist.py`: an exact rational oracle (`fractions`) and a
//! bit-exact float model of the pinned algorithm, checked against each other there. None come
//! from this crate. The sphere mesh and its query stream are regenerated here from the pinned
//! generators and checked against the fixture's `mesh_digest` and `query_digest` before use.
//! The brute-force-versus-hierarchy fuzz and the error cases are hand-labelled from the leaf's
//! Design.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use fid_geo_dist::mesh::{COORD_LIMIT, DEGENERACY_GATE};
use fid_geo_dist::{
    closest_on_mesh, closest_on_triangle, DistError, Feature, MeshHit, MeshProjection, MeshQuery, MeshTarget,
    TriangleSubset,
};
use fid_geo_index::PrimitiveBvh;
use fid_geo_types::{FrameRef, Mesh};
use fid_kci::kci::{to_hex, Sha256};

const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/geo/dist/");

fn read(rel: &str) -> String {
    std::fs::read_to_string(format!("{ROOT}{rel}")).unwrap()
}

fn hexf(s: &str) -> f64 {
    f64::from_bits(u64::from_str_radix(s, 16).unwrap())
}

fn hexf3(s: &[&str]) -> [f64; 3] {
    [hexf(s[0]), hexf(s[1]), hexf(s[2])]
}

fn bits3(v: [f64; 3]) -> [u64; 3] {
    [v[0].to_bits(), v[1].to_bits(), v[2].to_bits()]
}

fn frame() -> FrameRef {
    FrameRef::from_bytes([0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15])
}

fn hit(r: MeshProjection) -> MeshHit {
    match r {
        MeshProjection::Hit(h) => h,
        MeshProjection::NoHit => panic!("expected a hit"),
    }
}

/// Bitwise equality of two projections (f64 `==` would identify -0.0 and +0.0).
fn same(x: &MeshProjection, y: &MeshProjection) -> bool {
    match (x, y) {
        (MeshProjection::NoHit, MeshProjection::NoHit) => true,
        (MeshProjection::Hit(a), MeshProjection::Hit(b)) => {
            bits3(a.foot) == bits3(b.foot)
                && a.dist.to_bits() == b.dist.to_bits()
                && a.dist2.to_bits() == b.dist2.to_bits()
                && a.tri == b.tri
                && a.feature == b.feature
        }
        _ => false,
    }
}

fn target_of(verts: &[[f64; 3]], tris: &[[u64; 3]]) -> (Mesh, PrimitiveBvh) {
    let mesh = Mesh::from_points(frame(), verts, tris).unwrap();
    let payload: Vec<u64> = (0..tris.len() as u64).collect();
    let bvh = PrimitiveBvh::build(&mesh, &payload, 1).unwrap();
    (mesh, bvh)
}

// ------------------------------------------------------------------ AC-2 regions

#[test]
fn ac2_triangle_regions() {
    let doc = common::parse_json(&read("triangle_regions.json"));
    let tris = doc.get("triangles").arr();
    assert_eq!(tris.len(), 50);
    let h3 = |j: &common::Json| -> [f64; 3] {
        let a = j.arr();
        [hexf(a[0].str()), hexf(a[1].str()), hexf(a[2].str())]
    };
    let mut seen = [0usize; 7];
    for t in tris {
        let (a, b, c) = (h3(t.get("a")), h3(t.get("b")), h3(t.get("c")));
        let size = hexf(t.get("size").str());
        let qs = t.get("queries").arr();
        assert_eq!(qs.len(), 7);
        for q in qs {
            let p = h3(q.get("p"));
            let r = closest_on_triangle(p, a, b, c).unwrap();
            let want = q.get("feature").num() as u8;
            seen[usize::from(want)] += 1;
            assert_eq!(r.feature.code(), want, "feature at {p:?}");
            let m = q.get("model");
            assert_eq!(bits3(r.foot), bits3(h3(m.get("foot"))), "model foot at {p:?}");
            assert_eq!(r.dist2.to_bits(), hexf(m.get("dist2").str()).to_bits());
            assert_eq!(r.feature.code(), m.get("feature").num() as u8);
            let exact = h3(q.get("foot"));
            let e: Vec<f64> = (0..3).map(|k| r.foot[k] - exact[k]).collect();
            let err = ((e[0] * e[0] + e[1] * e[1]) + e[2] * e[2]).sqrt();
            assert!(err <= 1e-15 * size, "foot error {err} > 1e-15 * {size} at {p:?}");
        }
    }
    assert_eq!(seen, [50; 7], "every Voronoi region is exercised 50 times");
}

// ------------------------------------------------------------------ fuzz vs oracle

#[test]
fn fuzz_triangles_match_model_and_exact_oracle() {
    let text = read("tri_fuzz/cases.txt");
    let mut n = 0;
    let mut must = 0;
    for ln in text.lines().filter(|l| l.starts_with("case ")) {
        let f: Vec<&str> = ln.split(' ').collect();
        let kind = f[1];
        let p = hexf3(&f[2..5]);
        let (a, b, c) = (hexf3(&f[5..8]), hexf3(&f[8..11]), hexf3(&f[11..14]));
        let r = closest_on_triangle(p, a, b, c).unwrap();
        assert!(r.dist2.is_finite() && r.foot.iter().all(|x| x.is_finite()), "{kind}: NaN at {ln}");
        assert_eq!(bits3(r.foot), bits3(hexf3(&f[14..17])), "{kind}: foot differs from the model: {ln}");
        assert_eq!(r.dist2.to_bits(), hexf(f[17]).to_bits(), "{kind}: dist2 differs from the model: {ln}");
        assert_eq!(r.feature.code().to_string(), f[18], "{kind}: feature differs from the model: {ln}");
        let exact_d = hexf(f[20]).sqrt();
        let tol = hexf(f[21]);
        assert!((r.dist2.sqrt() - exact_d).abs() <= tol, "{kind}: distance off the exact oracle: {ln}");
        if f[22] == "1" {
            assert_eq!(r.feature.code().to_string(), f[19], "{kind}: feature differs from the exact oracle: {ln}");
            must += 1;
        }
        n += 1;
    }
    assert_eq!(n, 2100);
    assert!(must >= 600, "{must}");
}

// ------------------------------------------------------------------ AC-4 ties

#[test]
fn ac4_shared_vertex_ties_choose_lowest_index() {
    let text = read("shared_vertex_ties/cases.txt");
    let mut verts = Vec::new();
    let mut tris = Vec::new();
    let mut qs = Vec::new();
    for ln in text.lines() {
        let f: Vec<&str> = ln.split(' ').collect();
        match f[0] {
            "v" => verts.push(hexf3(&f[1..4])),
            "t" => tris.push([f[1].parse().unwrap(), f[2].parse().unwrap(), f[3].parse().unwrap()]),
            "q" => qs.push(f.iter().map(|s| s.to_string()).collect::<Vec<_>>()),
            _ => {}
        }
    }
    let (mesh, bvh) = target_of(&verts, &tris);
    let target = MeshTarget::new(&mesh, &bvh).unwrap();
    let mut ties = 0;
    for f in &qs {
        let p = hexf3(&[&f[1], &f[2], &f[3]]);
        let r = target.closest(p, &MeshQuery::default()).unwrap();
        let brute = target.closest_brute(p, &MeshQuery::default()).unwrap();
        assert!(same(&r, &brute), "hierarchy and brute force differ at {p:?}");
        let h = hit(r);
        assert_eq!(h.tri.to_string(), f[4], "lowest tied triangle at {p:?}");
        assert_eq!(h.feature.code().to_string(), f[5]);
        assert_eq!(bits3(h.foot), bits3(hexf3(&[&f[6], &f[7], &f[8]])));
        assert_eq!(h.dist2.to_bits(), hexf(&f[9]).to_bits());
        if f[10] != "1" {
            ties += 1;
        }
    }
    assert!(ties >= 20, "{ties}");
}

// ------------------------------------------------------------------ AC-1 / AC-6 sphere

const N_SUB: u64 = 71;
const N_QUERIES: usize = 100_000;
const SEED: u64 = 0x6765_6F2E_6469_7374;

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
        a + (b - a) * ((self.next() >> 11) as f64 * (1.0 / 9_007_199_254_740_992.0))
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

fn sqrt(x: f64) -> f64 {
    fid_num::f::sqrt(x).unwrap()
}

fn sphere_mesh() -> (Vec<[f64; 3]>, Vec<[u64; 3]>) {
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

fn sphere_queries(verts: &[[f64; 3]], tris: &[[u64; 3]]) -> Vec<[f64; 3]> {
    let mut g = SplitMix(SEED);
    let mut qs = Vec::with_capacity(N_QUERIES);
    for i in 0..N_QUERIES {
        if i % 50 == 0 {
            qs.push(verts[g.below(verts.len() as u64) as usize]);
        } else if i % 50 == 1 {
            let t = tris[g.below(tris.len() as u64) as usize];
            let k = g.below(3) as usize;
            let (s, e) = (verts[t[k] as usize], verts[t[(k + 1) % 3] as usize]);
            qs.push(std::array::from_fn(|j| (s[j] + e[j]) * 0.5));
        } else {
            let d = [g.uni(-1.0, 1.0), g.uni(-1.0, 1.0), g.uni(-1.0, 1.0)];
            let r = g.uni(0.95, 1.05);
            let len = sqrt((d[0] * d[0] + d[1] * d[1]) + d[2] * d[2]);
            let s = r / len;
            qs.push([d[0] * s, d[1] * s, d[2] * s]);
        }
    }
    qs
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

fn result_bytes(h: &MeshHit) -> Vec<u8> {
    let mut b = Vec::with_capacity(49);
    b.extend_from_slice(&h.tri.to_le_bytes());
    b.push(h.feature.code());
    for x in h.foot.iter().chain([h.dist2, h.dist].iter()) {
        b.extend_from_slice(&x.to_bits().to_le_bytes());
    }
    b
}

fn hx(x: f64) -> String {
    format!("{:016x}", x.to_bits())
}

#[test]
fn ac1_ac6_sphere_equals_brute_force_reference() {
    let text = read("mesh_sphere_1e5/expected.txt");
    let mut header = std::collections::BTreeMap::new();
    let mut samples = std::collections::BTreeMap::new();
    for ln in text.lines().filter(|l| !l.starts_with('#')) {
        let (k, v) = ln.split_once(' ').unwrap();
        if k == "r" {
            let (i, rest) = v.split_once(' ').unwrap();
            samples.insert(i.parse::<usize>().unwrap(), rest.to_string());
        } else {
            header.insert(k.to_string(), v.to_string());
        }
    }
    let (verts, tris) = sphere_mesh();
    assert_eq!(mesh_digest(&verts, &tris), header["mesh_digest"]);
    let qs = sphere_queries(&verts, &tris);
    let mut qh = Sha256::new();
    for q in &qs {
        for x in q {
            qh.update(&x.to_bits().to_le_bytes());
        }
    }
    assert_eq!(to_hex(&qh.finish()), header["query_digest"]);

    let (mesh, bvh) = target_of(&verts, &tris);
    let target = MeshTarget::new(&mesh, &bvh).unwrap();
    let opts = MeshQuery::default();
    let mut total = Sha256::new();
    for (i, &p) in qs.iter().enumerate() {
        let r = target.closest(p, &opts).unwrap();
        let h = hit(r);
        total.update(&result_bytes(&h));
        if let Some(line) = samples.get(&i) {
            let got = format!(
                "{} {} {} {} {} {} {}",
                h.tri,
                h.feature.code(),
                hx(h.foot[0]),
                hx(h.foot[1]),
                hx(h.foot[2]),
                hx(h.dist2),
                hx(h.dist)
            );
            assert_eq!(&got, line, "query {i}");
        }
        if i % 2000 == 7 {
            let brute = target.closest_brute(p, &opts).unwrap();
            assert!(same(&r, &brute), "query {i}: hierarchy differs from the brute-force scan");
        }
    }
    assert_eq!(samples.len(), N_QUERIES / 100);
    // AC-6: the same digest on every target that runs this test (Linux, Windows, macOS, wasm32).
    assert_eq!(to_hex(&total.finish()), header["result_digest"]);
}

// ------------------------------------------------------------------ AC-3 no hit

fn plane_grid(n: u64) -> (Vec<[f64; 3]>, Vec<[u64; 3]>) {
    let mut verts = Vec::new();
    for j in 0..=n {
        for i in 0..=n {
            verts.push([i as f64 / n as f64, j as f64 / n as f64, 0.0]);
        }
    }
    let id = |i: u64, j: u64| j * (n + 1) + i;
    let mut tris = Vec::new();
    for j in 0..n {
        for i in 0..n {
            tris.push([id(i, j), id(i + 1, j), id(i + 1, j + 1)]);
            tris.push([id(i, j), id(i + 1, j + 1), id(i, j + 1)]);
        }
    }
    (verts, tris)
}

#[test]
fn ac3_max_distance_gives_no_hit() {
    let (verts, tris) = plane_grid(16);
    let (mesh, bvh) = target_of(&verts, &tris);
    let target = MeshTarget::new(&mesh, &bvh).unwrap();
    let q1 = MeshQuery { max_distance: Some(0.001), subset: None };
    let mut g = SplitMix(7);
    for _ in 0..500 {
        let (x, y) = (g.uni(0.0, 1.0), g.uni(0.0, 1.0));
        for z in [0.002, -0.002] {
            assert_eq!(target.closest([x, y, z], &q1).unwrap(), MeshProjection::NoHit);
            assert_eq!(target.closest_brute([x, y, z], &q1).unwrap(), MeshProjection::NoHit);
        }
        // Beyond an edge of the sheet, 2 mm away in the plane.
        assert_eq!(target.closest([1.002, y, 0.0], &q1).unwrap(), MeshProjection::NoHit);
        let h = hit(target.closest([x, y, 0.0005], &q1).unwrap());
        assert!((h.dist - 0.0005).abs() < 1e-15);
    }
    // The limit is inclusive: dist2 = d*d with the same rounding is a hit.
    let h = hit(target.closest([0.3, 0.3, 0.001], &q1).unwrap());
    assert_eq!(h.dist2, 0.001 * 0.001);
    // A zero limit hits only points on the mesh.
    let q0 = MeshQuery { max_distance: Some(0.0), subset: None };
    assert!(matches!(target.closest([0.25, 0.5, 0.0], &q0).unwrap(), MeshProjection::Hit(_)));
    assert_eq!(target.closest([0.25, 0.5, 1e-150], &q0).unwrap(), MeshProjection::NoHit);
}

// ------------------------------------------------------------------ AC-5 degenerate

#[test]
fn ac5_collinear_triangle_equals_segment_distance() {
    let (a, b, c) = ([0.0, 0.0, 0.0], [2.0, 0.0, 0.0], [1.0, 0.0, 0.0]);
    // (query, expected foot, expected feature)
    let cases = [
        ([1.5, 1.0, 0.0], [1.5, 0.0, 0.0], Feature::Edge(0)),
        ([0.5, 0.0, 3.0], [0.5, 0.0, 0.0], Feature::Edge(0)),
        ([1.0, -2.0, 0.0], [1.0, 0.0, 0.0], Feature::Vertex(2)),
        ([-1.0, 1.0, 1.0], [0.0, 0.0, 0.0], Feature::Vertex(0)),
        ([5.0, 0.0, 0.0], [2.0, 0.0, 0.0], Feature::Vertex(1)),
        ([0.25, 0.0, 0.0], [0.25, 0.0, 0.0], Feature::Edge(0)),
    ];
    for (p, foot, feature) in cases {
        let r = closest_on_triangle(p, a, b, c).unwrap();
        assert_eq!(r.foot, foot, "{p:?}");
        assert_eq!(r.feature, feature, "{p:?}");
        let d: [f64; 3] = std::array::from_fn(|k| p[k] - foot[k]);
        assert_eq!(r.dist2, (d[0] * d[0] + d[1] * d[1]) + d[2] * d[2]);
    }
    // Repeated corners and a single point.
    let r = closest_on_triangle([1.0, 1.0, 0.0], a, a, b).unwrap();
    assert_eq!((r.foot, r.feature), ([1.0, 0.0, 0.0], Feature::Edge(1)));
    let r = closest_on_triangle([1.0, 1.0, 1.0], b, b, b).unwrap();
    assert_eq!((r.foot, r.dist2, r.feature), (b, 3.0, Feature::Vertex(0)));
    // In a mesh, next to a regular triangle.
    let verts = [a, b, c, [0.0, 5.0, 0.0], [1.0, 5.0, 0.0], [0.0, 6.0, 0.0]];
    let (mesh, bvh) = target_of(&verts, &[[3, 4, 5], [0, 1, 2]]);
    let h = hit(closest_on_mesh([1.5, 1.0, 0.0], &mesh, &bvh, &MeshQuery::default()).unwrap());
    assert_eq!((h.tri, h.foot, h.dist, h.feature), (1, [1.5, 0.0, 0.0], 1.0, Feature::Edge(0)));
}

// ------------------------------------------------------------------ boundaries (hand-labelled)

#[test]
fn boundary_queries_take_the_lowest_feature_code() {
    let (a, b, c) = ([0.0, 0.0, 0.0], [4.0, 0.0, 0.0], [0.0, 4.0, 0.0]);
    let cases = [
        ([0.0, 0.0, 0.0], Feature::Vertex(0)),
        ([4.0, 0.0, 1.0], Feature::Vertex(1)),
        ([0.0, 4.0, -1.0], Feature::Vertex(2)),
        ([2.0, 0.0, 0.0], Feature::Edge(0)),
        ([2.0, 2.0, 3.0], Feature::Edge(1)),
        ([0.0, 2.0, 0.0], Feature::Edge(2)),
        ([1.0, 1.0, 0.0], Feature::Interior),
        ([1.0, 1.0, -7.0], Feature::Interior),
        ([0.0, -1.0, 0.0], Feature::Vertex(0)), // on the vertex-0 / edge-0 region boundary
        ([4.0, -1.0, 0.0], Feature::Vertex(1)), // on the edge-0 / vertex-1 region boundary
        ([2.0, -1.0, 2.0], Feature::Edge(0)),
        ([5.0, 1.0, 0.0], Feature::Vertex(1)), // on the vertex-1 / edge-1 region boundary
    ];
    for (p, feature) in cases {
        assert_eq!(closest_on_triangle(p, a, b, c).unwrap().feature, feature, "{p:?}");
    }
    let codes: Vec<u8> = [0, 1, 2]
        .map(Feature::Vertex)
        .into_iter()
        .chain([0, 1, 2].map(Feature::Edge))
        .chain([Feature::Interior])
        .map(Feature::code)
        .collect();
    assert_eq!(codes, [0, 1, 2, 3, 4, 5, 6]);
    assert_eq!(Feature::Edge(255).code(), 255);
}

#[test]
fn sliver_and_near_degenerate_triangles_never_give_nan() {
    let mut g = SplitMix(11);
    for i in 0..20_000 {
        let a = [g.uni(-1.0, 1.0), g.uni(-1.0, 1.0), g.uni(-1.0, 1.0)];
        let b = [g.uni(-1.0, 1.0), g.uni(-1.0, 1.0), g.uni(-1.0, 1.0)];
        let t = g.uni(-1.0, 2.0);
        let eps = [0.0, 1e-300, 1e-200, 1e-30, 1e-17, 1e-12, 1e-8][i % 7];
        let c: [f64; 3] = std::array::from_fn(|k| a[k] + t * (b[k] - a[k]) + eps * g.uni(-1.0, 1.0));
        let p = [g.uni(-3.0, 3.0), g.uni(-3.0, 3.0), g.uni(-3.0, 3.0)];
        let p = if i % 5 == 0 { a } else { p };
        let r = closest_on_triangle(p, a, b, c).unwrap();
        assert!(r.dist2.is_finite() && r.dist2 >= 0.0 && r.foot.iter().all(|x| x.is_finite()));
        // The foot is never farther than the nearest corner.
        let dv = [a, b, c].map(|v| {
            let d: [f64; 3] = std::array::from_fn(|k| p[k] - v[k]);
            (d[0] * d[0] + d[1] * d[1]) + d[2] * d[2]
        });
        assert!(r.dist2 <= dv[0] && r.dist2 <= dv[1] && r.dist2 <= dv[2], "{p:?} {a:?} {b:?} {c:?}");
    }
    // Extreme magnitudes at the limit do not overflow.
    let l = COORD_LIMIT;
    let r = closest_on_triangle([l, l, l], [-l, -l, -l], [l, -l, -l], [-l, l, -l]).unwrap();
    assert!(r.dist2.is_finite());
    let r = closest_on_triangle([0.0; 3], [1e-300, 0.0, 0.0], [0.0, 1e-300, 0.0], [0.0, 0.0, 1e-300]).unwrap();
    assert!(r.dist2.is_finite());
    assert_eq!(DEGENERACY_GATE, 1.0 / 1_099_511_627_776.0);
}

// ------------------------------------------------------------------ hierarchy vs brute force

#[test]
fn hierarchy_equals_brute_force_on_random_meshes() {
    let mut g = SplitMix(0x5eed);
    for round in 0..60 {
        let mut verts: Vec<[f64; 3]> = Vec::new();
        let mut tris: Vec<[u64; 3]> = Vec::new();
        let dyadic = round % 3 == 0;
        let nv = 30 + g.below(150);
        for _ in 0..nv {
            let v = if dyadic {
                [g.below(9) as f64 * 0.5, g.below(9) as f64 * 0.5, g.below(3) as f64 * 0.5]
            } else {
                [g.uni(-1.0, 1.0), g.uni(-1.0, 1.0), g.uni(-0.2, 0.2)]
            };
            verts.push(v);
        }
        let nt = 20 + g.below(300);
        for _ in 0..nt {
            let t = [g.below(nv), g.below(nv), g.below(nv)];
            let t = if g.below(10) == 0 { [t[0], t[0], t[1]] } else { t };
            tris.push(t);
            if g.below(10) == 0 {
                tris.push(t); // duplicate: an exact tie
            }
        }
        let (mesh, bvh) = target_of(&verts, &tris);
        let target = MeshTarget::new(&mesh, &bvh).unwrap();
        let n = tris.len() as u64;
        let ids: Vec<u64> = (0..n).filter(|_| g.below(3) == 0).collect();
        let subset = TriangleSubset::new(&ids, n).unwrap();
        for qi in 0..200 {
            let p = match qi % 4 {
                0 => verts[g.below(nv) as usize],
                1 if dyadic => [g.below(17) as f64 * 0.25, g.below(17) as f64 * 0.25, g.below(5) as f64 * 0.25 - 0.5],
                _ => [g.uni(-1.5, 1.5), g.uni(-1.5, 1.5), g.uni(-1.0, 1.0)],
            };
            let max_distance = match qi % 5 {
                0 => Some(g.uni(0.0, 0.5)),
                1 => Some(0.0),
                _ => None,
            };
            for sub in [None, Some(&subset)] {
                let q = MeshQuery { max_distance, subset: sub };
                let x = target.closest(p, &q).unwrap();
                let y = target.closest_brute(p, &q).unwrap();
                assert!(same(&x, &y), "round {round} query {qi}: {x:?} != {y:?}");
                if let (Some(_), MeshProjection::Hit(h)) = (sub, x) {
                    assert!(ids.binary_search(&h.tri).is_ok());
                }
            }
        }
    }
}

// ------------------------------------------------------------------ input domain

#[test]
fn errors_are_typed() {
    let (a, b, c) = ([0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]);
    for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert_eq!(closest_on_triangle([bad, 0.0, 0.0], a, b, c), Err(DistError::NonFiniteQuery));
        assert_eq!(closest_on_triangle([0.0; 3], a, [0.0, bad, 0.0], c), Err(DistError::BadCorner { corner: 1 }));
    }
    let over = COORD_LIMIT * 2.0;
    assert_eq!(closest_on_triangle([0.0, 0.0, over], a, b, c), Err(DistError::QueryOutOfRange));
    assert_eq!(closest_on_triangle([0.0; 3], a, b, [0.0, -over, 0.0]), Err(DistError::BadCorner { corner: 2 }));
    assert!(closest_on_triangle([COORD_LIMIT; 3], a, b, c).is_ok());

    let (verts, tris) = plane_grid(2);
    let (mesh, bvh) = target_of(&verts, &tris);
    let target = MeshTarget::new(&mesh, &bvh).unwrap();
    for d in [f64::NAN, -1.0, f64::INFINITY, -0.0 - 1e-300] {
        let q = MeshQuery { max_distance: Some(d), subset: None };
        assert_eq!(target.closest([0.0; 3], &q), Err(DistError::InvalidMaxDistance));
        assert_eq!(target.closest_brute([0.0; 3], &q), Err(DistError::InvalidMaxDistance));
    }
    assert_eq!(target.closest([f64::NAN; 3], &MeshQuery::default()), Err(DistError::NonFiniteQuery));
    assert_eq!(target.closest([over; 3], &MeshQuery::default()), Err(DistError::QueryOutOfRange));

    let n = mesh.triangle_count();
    assert_eq!(TriangleSubset::new(&[1, 1], n), Err(DistError::InvalidSubset { position: 1 }));
    assert_eq!(TriangleSubset::new(&[3, 2], n), Err(DistError::InvalidSubset { position: 1 }));
    assert_eq!(TriangleSubset::new(&[0, n], n), Err(DistError::InvalidSubset { position: 1 }));
    let empty = TriangleSubset::new(&[], n).unwrap();
    let q = MeshQuery { max_distance: None, subset: Some(&empty) };
    assert_eq!(target.closest([0.5; 3], &q).unwrap(), MeshProjection::NoHit);
    let other = TriangleSubset::new(&[0], n + 1).unwrap();
    let q = MeshQuery { max_distance: None, subset: Some(&other) };
    assert_eq!(target.closest([0.5; 3], &q), Err(DistError::SubsetMismatch));
    // A subset restricts the answer: only triangle 5 is allowed.
    let only = TriangleSubset::new(&[5], n).unwrap();
    let q = MeshQuery { max_distance: None, subset: Some(&only) };
    assert_eq!(hit(target.closest([0.0, 0.0, 0.0], &q).unwrap()).tri, 5);

    // A hierarchy over another mesh is refused.
    let (verts2, _) = plane_grid(2);
    let moved: Vec<[f64; 3]> = verts2.iter().map(|v| [v[0] + 0.5, v[1], v[2]]).collect();
    let (_, bvh2) = target_of(&moved, &tris);
    assert_eq!(MeshTarget::new(&mesh, &bvh2).err(), Some(DistError::IndexMismatch { triangle: Some(0) }));
    let (_, bvh3) = target_of(&verts, &tris[..4]);
    assert_eq!(MeshTarget::new(&mesh, &bvh3).err(), Some(DistError::IndexMismatch { triangle: None }));
    let other_frame = Mesh::from_points(FrameRef::from_bytes([9; 16]), &verts, &tris).unwrap();
    assert_eq!(MeshTarget::new(&other_frame, &bvh).err(), Some(DistError::IndexMismatch { triangle: None }));
    // A mesh corner beyond the limit (accepted by the hierarchy, refused here).
    let mut far = verts.clone();
    far[4] = [over, 0.0, 0.0];
    let (mesh4, bvh4) = target_of(&far, &tris);
    assert!(matches!(MeshTarget::new(&mesh4, &bvh4), Err(DistError::BadTriangle { corner: _, triangle: _ })));

    for e in [
        DistError::NonFiniteQuery,
        DistError::QueryOutOfRange,
        DistError::BadCorner { corner: 0 },
        DistError::BadTriangle { triangle: 3, corner: 1 },
        DistError::InvalidMaxDistance,
        DistError::InvalidSubset { position: 2 },
        DistError::SubsetMismatch,
        DistError::IndexMismatch { triangle: None },
        DistError::IndexMismatch { triangle: Some(4) },
        DistError::Inconsistent,
    ] {
        assert!(!e.code().is_empty() && !e.to_string().is_empty());
    }
}
