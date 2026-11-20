//! Acceptance criteria of spec node GEO.DIST.SIGN (AC-1 to AC-5) and the topology, error and
//! determinism rules of `fid_geo_dist::sign`.
//!
//! Expected values come from `fixtures/geo/dist/sign_cube/`, `sign_open_flipped/` and
//! `sphere_concave_convex/`, written by the independent stdlib-Python reference
//! `tools/fixtures/geo_dist_sign.py`: labels from exact rational / integer inside tests, and
//! bit-exact model results (status, distance and bound bits) for samples. None come from this
//! crate. The point streams are regenerated here from the pinned generators and checked against
//! the fixtures' `points_sha256` before use. The remaining cases are hand-labelled from the
//! leaf's rules.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::BTreeMap;
use std::f64::consts::{FRAC_PI_2, PI};

use fid_geo_dist::{
    cad_sign, DistError, Feature, MeshHit, MeshProjection, MeshQuery, MeshTarget, PseudoNormals, SignError, SignStatus,
    SignTarget, SignUndefined, Signed,
};
use fid_geo_index::PrimitiveBvh;
use fid_geo_types::{FrameRef, Mesh};
use fid_kci::kci::{to_hex, Sha256};

const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/geo/");

fn read(rel: &str) -> String {
    std::fs::read_to_string(format!("{ROOT}{rel}")).unwrap()
}

fn pow2(k: i32) -> f64 {
    f64::from_bits(((k + 1023) as u64) << 52)
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
        (self.next() >> 11) as f64 * pow2(-53)
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

struct MeshData {
    frame: FrameRef,
    verts: Vec<[f64; 3]>,
    tris: Vec<[u64; 3]>,
}

fn parse_mesh(text: &str, hex: bool) -> MeshData {
    let mut frame = None;
    let mut verts = Vec::new();
    let mut tris = Vec::new();
    for ln in text.lines() {
        let f: Vec<&str> = ln.split_whitespace().collect();
        match f.first() {
            Some(&"frame") => {
                let mut b = [0u8; 16];
                for (i, slot) in b.iter_mut().enumerate() {
                    *slot = u8::from_str_radix(&f[1][2 * i..2 * i + 2], 16).unwrap();
                }
                frame = Some(FrameRef::from_bytes(b));
            }
            Some(&"v") => {
                let x = |s: &str| {
                    if hex {
                        f64::from_bits(u64::from_str_radix(s, 16).unwrap())
                    } else {
                        s.parse::<f64>().unwrap()
                    }
                };
                verts.push([x(f[1]), x(f[2]), x(f[3])]);
            }
            Some(&"t") => tris.push([f[1].parse().unwrap(), f[2].parse().unwrap(), f[3].parse().unwrap()]),
            _ => {}
        }
    }
    MeshData { frame: frame.unwrap(), verts, tris }
}

fn build(m: &MeshData) -> (Mesh, PrimitiveBvh) {
    let mesh = Mesh::from_points(m.frame, &m.verts, &m.tris).unwrap();
    let payload: Vec<u64> = (0..m.tris.len() as u64).collect();
    let bvh = PrimitiveBvh::build(&mesh, &payload, 1).unwrap();
    (mesh, bvh)
}

fn mesh_of(verts: &[[f64; 3]], tris: &[[u64; 3]]) -> (Mesh, PrimitiveBvh) {
    let frame = FrameRef::from_bytes([7; 16]);
    build(&MeshData { frame, verts: verts.to_vec(), tris: tris.to_vec() })
}

fn cube() -> MeshData {
    parse_mesh(&read("types/mesh_cube.txt"), false)
}

/// The fixture header (`key value`), labels (`l` lines, concatenated) and samples
/// (`s index status distance-bits bound-bits`).
struct Expected {
    header: BTreeMap<String, String>,
    labels: Vec<u8>,
    analytic: Vec<u8>,
    samples: BTreeMap<usize, (u8, u64, u64)>,
}

fn parse_expected(text: &str) -> Expected {
    let mut header = BTreeMap::new();
    let mut labels = Vec::new();
    let mut analytic = Vec::new();
    let mut samples = BTreeMap::new();
    for ln in text.lines().filter(|l| !l.starts_with('#')) {
        let (k, v) = ln.split_once(' ').unwrap();
        match k {
            "l" => labels.extend_from_slice(v.as_bytes()),
            "a" => analytic.extend_from_slice(v.as_bytes()),
            "s" => {
                let f: Vec<&str> = v.split(' ').collect();
                let h = |s: &str| u64::from_str_radix(s, 16).unwrap();
                samples.insert(f[0].parse().unwrap(), (f[1].parse().unwrap(), h(f[2]), h(f[3])));
            }
            _ => {
                header.insert(k.to_string(), v.to_string());
            }
        }
    }
    Expected { header, labels, analytic, samples }
}

fn points_digest(pts: &[[f64; 3]]) -> String {
    let mut h = Sha256::new();
    for p in pts {
        for x in p {
            h.update(&x.to_bits().to_le_bytes());
        }
    }
    to_hex(&h.finish())
}

/// The status a label allows: exact for `o i + -`, the same or Undecidable within the 2^-30 band
/// (`O I`), on the surface with `+0.0` for `z`.
fn check_label(label: u8, r: &Signed, i: usize) {
    assert!(r.bound.is_finite() && r.bound >= 0.0, "point {i}: bound {}", r.bound);
    match label {
        b'o' | b'+' => {
            assert_eq!(r.status, SignStatus::Outside, "point {i}");
            assert_eq!(r.distance.to_bits(), r.unsigned.to_bits(), "point {i}");
        }
        b'i' | b'-' => {
            assert_eq!(r.status, SignStatus::Inside, "point {i}");
            assert_eq!(r.distance.to_bits(), (-r.unsigned).to_bits(), "point {i}");
        }
        b'O' => assert!(matches!(r.status, SignStatus::Outside | SignStatus::Undecidable), "point {i}: {r:?}"),
        b'I' => assert!(matches!(r.status, SignStatus::Inside | SignStatus::Undecidable), "point {i}: {r:?}"),
        b'z' => {
            assert_eq!(r.status, SignStatus::OnSurface, "point {i}");
            assert_eq!(r.distance.to_bits(), 0, "point {i}");
        }
        _ => panic!("unknown label {label}"),
    }
}

fn check_sample(ex: &Expected, i: usize, r: &Signed) -> bool {
    match ex.samples.get(&i) {
        Some(&(code, d, b)) => {
            assert_eq!(
                (r.status.code(), r.distance.to_bits(), r.bound.to_bits()),
                (code, d, b),
                "point {i}: differs from the bit-exact model"
            );
            true
        }
        None => false,
    }
}

// ------------------------------------------------------------------ AC-1 cube

/// The pinned AC-1 stream (`cube_points` in tools/fixtures/geo_dist_sign.py).
fn cube_points(n: usize, seed: u64) -> Vec<[f64; 3]> {
    let mut g = SplitMix(seed);
    let mut pts = Vec::with_capacity(n);
    for _ in 0..n {
        let kind = g.below(8);
        let p = match kind {
            0 => [-0.5 + 2.0 * g.unit(), -0.5 + 2.0 * g.unit(), -0.5 + 2.0 * g.unit()],
            1 => {
                let f = g.below(6) as usize;
                let (ax, side) = (f >> 1, (f & 1) as f64);
                let h = pow2(-(1 + g.below(40) as i32));
                let mut p = [g.unit(), g.unit(), g.unit()];
                p[ax] = side + (2.0 * g.unit() - 1.0) * h;
                p
            }
            2 | 7 => {
                let e = g.below(12) as usize;
                let (ax, b) = (e % 3, e / 3);
                let (o1, o2) = ((ax + 1) % 3, (ax + 2) % 3);
                let h = pow2(-(1 + g.below(if kind == 2 { 40 } else { 6 }) as i32));
                let mut p = [0.0; 3];
                p[ax] = g.unit();
                p[o1] = (b & 1) as f64;
                p[o2] = ((b >> 1) & 1) as f64;
                for j in [o1, o2] {
                    p[j] += (2.0 * g.unit() - 1.0) * h;
                }
                p
            }
            3 => {
                let c = g.below(8);
                let h = pow2(-(1 + g.below(40) as i32));
                let mut p = [0.0; 3];
                for (j, slot) in p.iter_mut().enumerate() {
                    *slot = ((c >> j) & 1) as f64 + (2.0 * g.unit() - 1.0) * h;
                }
                p
            }
            4 => {
                let f = g.below(6) as usize;
                let (ax, side) = (f >> 1, (f & 1) as f64);
                let mut p = [0.0; 3];
                for slot in p.iter_mut() {
                    *slot = (g.next() >> 44) as f64 * pow2(-20);
                }
                p[ax] = side;
                p
            }
            5 => [-8.0 + 17.0 * g.unit(), -8.0 + 17.0 * g.unit(), -8.0 + 17.0 * g.unit()],
            _ => [0.25 + 0.5 * g.unit(), 0.25 + 0.5 * g.unit(), 0.25 + 0.5 * g.unit()],
        };
        pts.push(p);
    }
    pts
}

fn seed_of(ex: &Expected) -> u64 {
    u64::from_str_radix(ex.header["seed"].trim_start_matches("0x"), 16).unwrap()
}

#[test]
fn ac1_cube_signs_agree_with_exact_inside_test() {
    let ex = parse_expected(&read("dist/sign_cube/expected.txt"));
    let n: usize = ex.header["count"].parse().unwrap();
    assert_eq!(n, 100_000);
    assert_eq!(ex.labels.len(), n);
    let pts = cube_points(n, seed_of(&ex));
    assert_eq!(points_digest(&pts), ex.header["points_sha256"]);

    let m = cube();
    let (mesh, bvh) = build(&m);
    let st = SignTarget::new(MeshTarget::new(&mesh, &bvh).unwrap()).unwrap();
    let (mut sampled, mut band_undecided) = (0usize, 0usize);
    for (i, p) in pts.iter().enumerate() {
        let r = st.signed_distance(*p).unwrap();
        check_label(ex.labels[i], &r, i);
        if r.status == SignStatus::Undecidable {
            band_undecided += 1;
        }
        if check_sample(&ex, i, &r) {
            sampled += 1;
        }
    }
    assert_eq!(sampled, ex.samples.len());
    assert_eq!(sampled, 2000);
    // Undecidable is allowed only within 2^-30 m of the surface; report how often it happened.
    println!("AC-1: {n} points, {band_undecided} undecidable (all within 2^-30 m of the surface)");
}

// ------------------------------------------------------------------ AC-2 concave and convex edges

#[test]
fn ac2_concave_convex_edges_agree_with_analytic_sign() {
    let m = parse_mesh(&read("dist/sphere_concave_convex/mesh.txt"), true);
    let ex = parse_expected(&read("dist/sphere_concave_convex/expected.txt"));
    let generated: usize = ex.header["generated"].parse().unwrap();
    assert_eq!(ex.labels.len(), generated);
    assert_eq!(ex.analytic.len(), generated);
    assert_eq!(ex.header["kept"], "10000");

    let mut edges: Vec<(u64, u64)> = Vec::new();
    for t in &m.tris {
        for (a, b) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
            edges.push(if a < b { (a, b) } else { (b, a) });
        }
    }
    edges.sort_unstable();
    edges.dedup();

    let mut g = SplitMix(seed_of(&ex));
    let mut pts = Vec::with_capacity(generated);
    for _ in 0..generated {
        let (lo, hi) = edges[g.below(edges.len() as u64) as usize];
        let (a, b) = (m.verts[lo as usize], m.verts[hi as usize]);
        let t = g.unit();
        let h = pow2(-(5 + g.below(3) as i32));
        let mut q = [0.0; 3];
        for (j, slot) in q.iter_mut().enumerate() {
            *slot = (a[j] + t * (b[j] - a[j])) + (2.0 * g.unit() - 1.0) * h;
        }
        pts.push(q);
    }
    assert_eq!(points_digest(&pts), ex.header["points_sha256"]);

    let (mesh, bvh) = build(&m);
    assert!(mesh.facts().closed && mesh.facts().oriented);
    let st = SignTarget::new(MeshTarget::new(&mesh, &bvh).unwrap()).unwrap();
    let (mut kept, mut sampled) = (0usize, 0usize);
    for (i, p) in pts.iter().enumerate() {
        // Every generated point: the exact mesh side (or Undecidable within 2^-30 m of the mesh).
        let r = st.signed_distance(*p).unwrap();
        let side = ex.labels[i];
        check_label(side, &r, i);
        // Points outside the mesh-approximation band (exact mesh side equals analytic side):
        // the certified sign is the analytic surface's.
        let want = match (side, ex.analytic[i]) {
            (b'o', b'+') => Some(SignStatus::Outside),
            (b'i', b'-') => Some(SignStatus::Inside),
            _ => None,
        };
        if let Some(w) = want {
            kept += 1;
            assert_eq!(r.status, w, "point {i}: analytic side");
        }
        if check_sample(&ex, i, &r) {
            sampled += 1;
        }
    }
    assert_eq!(kept, 10_000);
    assert_eq!(sampled, ex.samples.len());
}

// ------------------------------------------------------------------ AC-3 open and flipped

#[test]
fn ac3_open_flipped_is_undefined_with_correct_distance() {
    let m = parse_mesh(&read("types/mesh_open_flipped.txt"), false);
    let (mesh, bvh) = build(&m);
    let st = SignTarget::new(MeshTarget::new(&mesh, &bvh).unwrap()).unwrap();
    // Hand-labelled: (point, exact unsigned distance). The first projects onto the boundary edge
    // 1-6 (the hole's diagonal), the second into the flipped triangle 0, the third near the hole
    // onto boundary edge 1-5.
    let cases: [([f64; 3], f64); 3] =
        [([1.5, 0.5, 0.5], 0.5), ([0.75, 0.25, -0.5], 0.5), ([1.5, 0.2, 0.6], 0.538_516_480_713_450_4)];
    for (p, d) in cases {
        let r = st.signed_distance(p).unwrap();
        assert!(matches!(r.status, SignStatus::Undefined(_)), "{p:?}: {r:?}");
        assert!(!r.sign_defined());
        assert_eq!(r.distance.to_bits(), r.unsigned.to_bits());
        assert!((r.unsigned - d).abs() <= 1e-15, "{p:?}: {} vs {d}", r.unsigned);
        assert!(r.bound < 1e-14, "{p:?}: bound {}", r.bound);
    }
    // The whole component holds misoriented edges.
    assert_eq!(st.normals().triangle(5), Some(Some(SignUndefined::Misoriented)));
    assert_eq!(
        st.normals().edge(1, 6).unwrap().feature.status,
        Some(SignUndefined::Misoriented),
        "boundary edge inside a misoriented component reports the more severe reason"
    );

    // Bit-exact model results for 400 seeded points.
    let ex = parse_expected(&read("dist/sign_open_flipped/expected.txt"));
    let n: usize = ex.header["count"].parse().unwrap();
    let mut g = SplitMix(seed_of(&ex));
    let pts: Vec<[f64; 3]> =
        (0..n).map(|_| [-0.5 + 2.0 * g.unit(), -0.5 + 2.0 * g.unit(), -0.5 + 2.0 * g.unit()]).collect();
    assert_eq!(points_digest(&pts), ex.header["points_sha256"]);
    for (i, p) in pts.iter().enumerate() {
        let r = st.signed_distance(*p).unwrap();
        assert!(!r.sign_defined() || r.status == SignStatus::OnSurface);
        assert!(check_sample(&ex, i, &r));
    }
}

#[test]
fn open_but_oriented_mesh_flags_only_the_boundary() {
    let mut m = cube();
    m.tris.pop(); // remove triangle 11 (1 6 5): a hole in the face x = 1
    let (mesh, bvh) = build(&m);
    let st = SignTarget::new(MeshTarget::new(&mesh, &bvh).unwrap()).unwrap();
    let pn = st.normals();
    assert_eq!(pn.edge(1, 6).unwrap().feature.status, Some(SignUndefined::Boundary));
    assert_eq!(pn.vertex(1).unwrap().status, Some(SignUndefined::Boundary));
    assert_eq!(pn.vertex(3).unwrap().status, None);
    assert_eq!(pn.triangle(10), Some(None), "a triangle with a boundary edge keeps its interior sign");
    let r = st.signed_distance([1.5, 0.5, 0.5]).unwrap();
    assert_eq!(r.status, SignStatus::Undefined(SignUndefined::Boundary));
    assert_eq!(st.signed_distance([0.5, 0.25, -0.5]).unwrap().status, SignStatus::Outside);
    assert_eq!(st.signed_distance([0.25, 0.5, 0.75]).unwrap().status, SignStatus::Inside);
    assert_eq!(st.signed_distance([0.5, 1.5, 0.25]).unwrap().status, SignStatus::Outside);
}

// ------------------------------------------------------------------ AC-4 zero distance

#[test]
fn ac4_on_surface_is_positive_zero() {
    let m = cube();
    let (mesh, bvh) = build(&m);
    let st = SignTarget::new(MeshTarget::new(&mesh, &bvh).unwrap()).unwrap();
    for p in [
        [1.0, 1.0, 1.0],
        [0.5, 0.0, 0.0],
        [0.25, 0.5, 1.0],
        [0.5, 0.5, 0.0],
        [-0.0, 0.5, 0.5],
        [0.0, 0.375, 0.625],
        [1.0, 0.75, 0.25],
    ] {
        let r = st.signed_distance(p).unwrap();
        assert_eq!(r.status, SignStatus::OnSurface, "{p:?}");
        assert_eq!(r.distance.to_bits(), 0, "{p:?}: signed distance must be +0.0");
        assert_eq!(r.unsigned.to_bits(), 0);
        assert!(r.sign_defined());
        assert!(r.bound < 1e-15, "{p:?}: bound {}", r.bound);
    }
}

// ------------------------------------------------------------------ AC-5 CAD form

fn global_feature(mesh: &Mesh, h: &MeshHit) -> Option<(u64, u64)> {
    let t = mesh.triangle(h.tri).unwrap();
    match h.feature {
        Feature::Edge(k) => {
            let (a, b) = (t[k as usize], t[(k as usize + 1) % 3]);
            Some(if a < b { (a, b) } else { (b, a) })
        }
        Feature::Vertex(i) => Some((t[i as usize], u64::MAX)),
        Feature::Interior => None,
    }
}

#[test]
fn ac5_cad_rule_matches_mesh_rule() {
    let m = cube();
    let (mesh, bvh) = build(&m);
    let target = MeshTarget::new(&mesh, &bvh).unwrap();
    let st = SignTarget::new(target).unwrap();
    // Edge 5-6 (x = 1, z = 1): convex, faces with outward normals +x and +z, each angle pi.
    let en = st.normals().edge(5, 6).unwrap();
    assert_eq!(en.feature.status, None);
    assert_eq!(en.feature.normal, [PI, 0.0, PI]);
    let edge_in = [([1.0, 0.0, 0.0], PI), ([0.0, 0.0, 1.0], PI)];
    // Vertex 6 (1, 1, 1): three faces, each with incident angle pi/2.
    let vert_in = [([1.0, 0.0, 0.0], FRAC_PI_2), ([0.0, 1.0, 0.0], FRAC_PI_2), ([0.0, 0.0, 1.0], FRAC_PI_2)];
    let mut g = SplitMix(0x6164_352E_6361_6473);
    let (mut on_edge, mut on_vertex) = (0, 0);
    for _ in 0..4000 {
        let near_vertex = g.below(2) == 1;
        let s = pow2(-(1 + g.below(30) as i32));
        let mut p = if near_vertex { [1.0, 1.0, 1.0] } else { [1.0, g.unit(), 1.0] };
        for x in p.iter_mut() {
            *x += (2.0 * g.unit() - 1.0) * s;
        }
        let hit = match target.closest(p, &MeshQuery::default()).unwrap() {
            MeshProjection::Hit(h) => h,
            MeshProjection::NoHit => panic!("no hit"),
        };
        let r = st.signed(p, &hit).unwrap();
        let (feature, incident): (_, &[([f64; 3], f64)]) =
            if near_vertex { ((6, u64::MAX), &vert_in) } else { ((5, 6), &edge_in) };
        if global_feature(&mesh, &hit) != Some(feature) || hit.dist == 0.0 {
            continue;
        }
        let c = cad_sign(p, hit.foot, 0.0, incident).unwrap();
        assert_eq!(c.status, r.status, "{p:?}");
        assert!(c.sign_defined());
        assert_eq!(c.distance.is_sign_negative(), r.distance.is_sign_negative());
        if near_vertex {
            on_vertex += 1;
        } else {
            on_edge += 1;
        }
    }
    assert!(on_edge > 300 && on_vertex > 100, "{on_edge} {on_vertex}");
    // The CAD pseudo-normal of the edge is bit-identical to the mesh's.
    let c = cad_sign([1.5, 0.5, 1.5], [1.0, 0.5, 1.0], 0.0, &edge_in).unwrap();
    assert_eq!(c.status, SignStatus::Outside);
    assert_eq!(cad_sign([0.75, 0.5, 0.75], [1.0, 0.5, 1.0], 0.0, &edge_in).unwrap().status, SignStatus::Inside);
}

fn unit(n: [f64; 3]) -> [f64; 3] {
    let l = fid_num::f::sqrt((n[0] * n[0] + n[1] * n[1]) + n[2] * n[2]).unwrap();
    [n[0] / l, n[1] / l, n[2] / l]
}

fn tri_normal(a: [f64; 3], b: [f64; 3], c: [f64; 3]) -> [f64; 3] {
    let (u, v) = ([b[0] - a[0], b[1] - a[1], b[2] - a[2]], [c[0] - a[0], c[1] - a[1], c[2] - a[2]]);
    [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]]
}

/// AC-5 at a concave edge of the star-shaped mesh: the CAD rule with the two face normals and
/// angles pi gives the mesh rule's status wherever the mesh foot lies on that edge.
#[test]
fn ac5_cad_rule_matches_mesh_rule_at_concave_edge() {
    let m = parse_mesh(&read("dist/sphere_concave_convex/mesh.txt"), true);
    let (mesh, bvh) = build(&m);
    let target = MeshTarget::new(&mesh, &bvh).unwrap();
    let st = SignTarget::new(target).unwrap();
    let v = |i: u64| m.verts[i as usize];
    // The first concave edge in sorted order: the opposite vertex of the second triangle lies
    // above the first triangle's plane.
    let mut found = None;
    'search: for (t1, a) in m.tris.iter().enumerate() {
        for k in 0..3 {
            let (s, e) = (a[k], a[(k + 1) % 3]);
            for (t2, b) in m.tris.iter().enumerate() {
                if t2 != t1 && b.contains(&s) && b.contains(&e) {
                    let d = *b.iter().find(|&&x| x != s && x != e).unwrap();
                    let n1 = tri_normal(v(a[0]), v(a[1]), v(a[2]));
                    let w = [v(d)[0] - v(s)[0], v(d)[1] - v(s)[1], v(d)[2] - v(s)[2]];
                    if (w[0] * n1[0] + w[1] * n1[1]) + w[2] * n1[2] > 1e-4 {
                        found = Some((s, e, t1, t2));
                        break 'search;
                    }
                }
            }
        }
    }
    let (s, e, t1, t2) = found.unwrap();
    let n = |t: usize| {
        let x = m.tris[t];
        unit(tri_normal(v(x[0]), v(x[1]), v(x[2])))
    };
    let incident = [(n(t1), PI), (n(t2), PI)];
    let key = if s < e { (s, e) } else { (e, s) };
    let mut g = SplitMix(0x636f_6e63_6176_6501);
    let (mut agree, mut inside) = (0, 0);
    for _ in 0..3000 {
        let t = g.unit();
        let h = pow2(-(6 + g.below(6) as i32));
        // Mostly along the inward bisector -(n1 + n2), where a concave edge is the foot.
        let depth = h * (0.5 + 0.5 * g.unit());
        let mut p = [0.0; 3];
        for (j, slot) in p.iter_mut().enumerate() {
            let bis = incident[0].0[j] + incident[1].0[j];
            *slot = ((v(s)[j] + t * (v(e)[j] - v(s)[j])) - bis * depth) + (2.0 * g.unit() - 1.0) * (h * 0.0625);
        }
        let hit = match target.closest(p, &MeshQuery::default()).unwrap() {
            MeshProjection::Hit(h) => h,
            MeshProjection::NoHit => panic!("no hit"),
        };
        if global_feature(&mesh, &hit) != Some(key) || hit.dist == 0.0 {
            continue;
        }
        let r = st.signed(p, &hit).unwrap();
        let c = cad_sign(p, hit.foot, 0.0, &incident).unwrap();
        if r.sign_defined() && c.sign_defined() {
            assert_eq!(c.status, r.status, "{p:?}");
            agree += 1;
            if r.status == SignStatus::Inside {
                inside += 1;
            }
        }
    }
    // A concave edge is the closest feature only from inside.
    assert!(agree > 100 && inside == agree, "{agree} {inside}");
}

#[test]
fn cad_rule_honours_foot_error_and_zero() {
    let inc = [([0.0, 0.0, 1.0], PI)];
    let up = cad_sign([0.0, 0.0, 1e-6], [0.0, 0.0, 0.0], 0.0, &inc).unwrap();
    assert_eq!(up.status, SignStatus::Outside);
    assert!((up.distance - 1e-6).abs() <= up.bound && up.bound < 1e-20);
    let uncertain = cad_sign([0.0, 0.0, 1e-6], [0.0, 0.0, 0.0], 1e-5, &inc).unwrap();
    assert_eq!(uncertain.status, SignStatus::Undecidable);
    assert!(!uncertain.sign_defined());
    assert!(uncertain.bound >= 1e-5);
    let zero = cad_sign([1.0, 2.0, 3.0], [1.0, 2.0, 3.0], 0.0, &inc).unwrap();
    assert_eq!(zero.status, SignStatus::OnSurface);
    assert_eq!(zero.distance.to_bits(), 0);
    let tangent = cad_sign([1.0, 0.0, 0.0], [0.0, 0.0, 0.0], 0.0, &inc).unwrap();
    assert_eq!(tangent.status, SignStatus::Undecidable);
}

#[test]
fn cad_rule_rejects_bad_input() {
    let ok = [([0.0, 0.0, 1.0], PI)];
    let bad = |f: &'static str, i: u64| Err(SignError::BadCadInput { field: f, index: i });
    assert_eq!(cad_sign([0.0; 3], [0.0; 3], 0.0, &[]), bad("incident", 0));
    assert_eq!(cad_sign([0.0; 3], [0.0; 3], -1.0, &ok), bad("foot_error", 0));
    assert_eq!(cad_sign([0.0; 3], [f64::NAN, 0.0, 0.0], 0.0, &ok), bad("foot", 0));
    assert_eq!(cad_sign([0.0; 3], [0.0; 3], 0.0, &[ok[0], ([0.0, 0.0, 1.0], 0.0)]), bad("angle", 1));
    assert_eq!(cad_sign([0.0; 3], [0.0; 3], 0.0, &[([f64::INFINITY, 0.0, 0.0], 1.0)]), bad("normal", 0));
    assert_eq!(cad_sign([0.0; 3], [0.0; 3], 0.0, &[([0.0, 0.0, 1.0], 7.0)]), bad("angle", 0));
    assert_eq!(cad_sign([f64::NAN, 0.0, 0.0], [0.0; 3], 0.0, &ok), Err(SignError::Dist(DistError::NonFiniteQuery)));
}

// ------------------------------------------------------------------ pseudo-normals and topology

#[test]
fn cube_pseudo_normals() {
    let m = cube();
    let (mesh, _) = build(&m);
    let pn = PseudoNormals::build(&mesh).unwrap();
    assert_eq!(pn, PseudoNormals::build(&mesh).unwrap(), "deterministic");
    assert_eq!(pn.edges().len(), 18);
    assert!(pn.edges().windows(2).all(|w| (w[0].lo, w[0].hi) < (w[1].lo, w[1].hi)), "sorted");
    for e in pn.edges() {
        assert_eq!(e.feature.status, None);
        assert!(e.feature.error.iter().all(|&x| x < 1e-13), "{:?}", e.feature.error);
    }
    // Face diagonal 0-2 (bottom face, both triangles normal -z).
    assert_eq!(pn.edge(0, 2).unwrap().feature.normal, [0.0, 0.0, -2.0 * PI]);
    for v in 0..8u64 {
        let n = pn.vertex(v).unwrap();
        assert_eq!(n.status, None);
        let c = m.verts[v as usize];
        for (k, &ck) in c.iter().enumerate() {
            // Each face contributes pi/2 of angle at a corner: N = (pi/2) (outward diagonal).
            let want = if ck == 1.0 { FRAC_PI_2 } else { -FRAC_PI_2 };
            assert!((n.normal[k] - want).abs() <= n.error[k] + 1e-15, "vertex {v}: {:?}", n.normal);
            assert!(n.error[k] < 1e-13);
        }
    }
    for t in 0..12 {
        assert_eq!(pn.triangle(t), Some(None));
    }
    assert_eq!(pn.triangle(12), None);
}

/// Two tetrahedra meeting at one vertex: every edge is manifold, the shared vertex is not.
#[test]
fn pinched_vertex_is_undefined() {
    let verts = [
        [0.0, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, 0.0, 1.0],
        [0.0, 0.0, -1.0],
        [-1.0, 0.0, -1.0],
        [0.0, -1.0, -1.0],
    ];
    // Outward-oriented tetrahedra (0 1 2 3) and (0 4 5 6), sharing vertex 0 only.
    let tris = [[0, 2, 1], [0, 1, 3], [0, 3, 2], [1, 2, 3], [0, 6, 4], [0, 4, 5], [0, 5, 6], [4, 6, 5]];
    let (mesh, bvh) = mesh_of(&verts, &tris);
    assert!(mesh.facts().closed && mesh.facts().oriented);
    let st = SignTarget::new(MeshTarget::new(&mesh, &bvh).unwrap()).unwrap();
    assert_eq!(st.normals().vertex(0).unwrap().status, Some(SignUndefined::NonManifoldVertex));
    assert_eq!(st.normals().vertex(1).unwrap().status, None);
    // Away from the pinch the signs are defined.
    assert_eq!(st.signed_distance([0.6, 0.6, 0.6]).unwrap().status, SignStatus::Outside);
    assert_eq!(st.signed_distance([0.2, 0.2, 0.2]).unwrap().status, SignStatus::Inside);
}

#[test]
fn nonmanifold_edge_and_degenerate_triangle() {
    let m = cube();
    // A fin: a third triangle on edge 5-6 makes it non-manifold, and the component with it.
    let mut verts = m.verts.clone();
    verts.push([2.0, 0.5, 2.0]);
    let mut tris = m.tris.clone();
    tris.push([5, 6, 8]);
    let (mesh, bvh) = mesh_of(&verts, &tris);
    let st = SignTarget::new(MeshTarget::new(&mesh, &bvh).unwrap()).unwrap();
    assert_eq!(st.normals().edge(5, 6).unwrap().feature.status, Some(SignUndefined::NonManifold));
    assert_eq!(st.signed_distance([0.5, 0.5, -0.5]).unwrap().status, SignStatus::Undefined(SignUndefined::NonManifold));

    // A repeated-index triangle on edge 0-1: degenerate, and queries near it are undefined.
    let mut tris = m.tris.clone();
    tris.push([0, 0, 1]);
    let (mesh, bvh) = mesh_of(&m.verts, &tris);
    let st = SignTarget::new(MeshTarget::new(&mesh, &bvh).unwrap()).unwrap();
    assert_eq!(st.normals().triangle(12), Some(Some(SignUndefined::Degenerate)));
    assert_eq!(st.normals().vertex(0).unwrap().status, Some(SignUndefined::Degenerate));
    assert_eq!(
        st.signed_distance([0.5, -0.25, -0.25]).unwrap().status,
        SignStatus::Undefined(SignUndefined::Degenerate)
    );
    assert_eq!(st.signed_distance([0.5, 0.5, 1.5]).unwrap().status, SignStatus::Outside);
    assert_eq!(st.signed_distance([0.5, 0.5, 0.75]).unwrap().status, SignStatus::Inside);
}

/// A closed box thinner than the rounding error: the far sheet is a candidate that disagrees, so
/// the sign is not given (never a guess).
#[test]
fn thinner_than_rounding_is_undecidable() {
    let h = pow2(-60);
    let verts = [
        [0.0, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        [1.0, 1.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, 0.0, h],
        [1.0, 0.0, h],
        [1.0, 1.0, h],
        [0.0, 1.0, h],
    ];
    let m = cube();
    let (mesh, bvh) = mesh_of(&verts, &m.tris);
    let st = SignTarget::new(MeshTarget::new(&mesh, &bvh).unwrap()).unwrap();
    let r = st.signed_distance([0.3, 0.6, 0.5]).unwrap();
    assert_eq!(r.status, SignStatus::Undecidable);
    assert!(!r.sign_defined());
    assert_eq!(r.distance.to_bits(), r.unsigned.to_bits());
    assert!((r.unsigned - 0.5).abs() <= r.bound);
}

/// REQ-004: a computed zero distance still runs the topology check. A point exactly on the hole's
/// boundary edge 1-6 computes to zero distance and is Undefined, with distance +0.0.
#[test]
fn computed_zero_on_boundary_is_undefined() {
    let mut m = cube();
    m.tris.pop();
    let (mesh, bvh) = build(&m);
    let st = SignTarget::new(MeshTarget::new(&mesh, &bvh).unwrap()).unwrap();
    for p in [[1.0, 0.5, 0.5], [1.0, 0.0, 0.25], [1.0, 1.0, 1.0]] {
        let r = st.signed_distance(p).unwrap();
        assert_eq!(r.unsigned.to_bits(), 0, "{p:?}");
        assert_eq!(r.status, SignStatus::Undefined(SignUndefined::Boundary), "{p:?}");
        assert_eq!(r.distance.to_bits(), 0);
        assert!(!r.sign_defined());
    }
    // Away from the hole a computed zero is OnSurface.
    assert_eq!(st.signed_distance([0.5, 0.5, 0.0]).unwrap().status, SignStatus::OnSurface);
}

/// The sign of the exact `a + b + c - 1` for binary64 values in `[2^-60, 1]` (all integer
/// multiples of 2^-120, so the sum is exact in `i128`), and its magnitude rounded to `f64`.
fn residual_sign(p: [f64; 3]) -> (i8, f64) {
    // Every coordinate here is at least 2^-60 or zero, so 2^-120 is a common quantum.
    let q = |x: f64| -> i128 {
        assert!(x == 0.0 || x >= pow2(-60));
        (x * pow2(120)) as i128
    };
    let r = q(p[0]) + q(p[1]) + q(p[2]) - (1i128 << 120);
    ((r > 0) as i8 - (r < 0) as i8, (r.unsigned_abs() as f64) * pow2(-120))
}

/// REQ-004 on a tilted face: points computed within rounding of the plane x + y + z = 1 either
/// get the exact sign of the residual, OnSurface with a bound covering the exact distance, or
/// Undecidable; never a wrong sign.
#[test]
fn tilted_triangle_near_plane() {
    let verts = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    let tris = [[0, 2, 1], [0, 1, 3], [0, 3, 2], [1, 2, 3]];
    let (mesh, bvh) = mesh_of(&verts, &tris);
    let st = SignTarget::new(MeshTarget::new(&mesh, &bvh).unwrap()).unwrap();
    let mut g = SplitMix(0x7469_6c74_6564_0001);
    let (mut zero, mut undecided, mut signed) = (0, 0, 0);
    for _ in 0..4000 {
        let a = 0.05 + 0.4 * g.unit();
        let b = 0.05 + 0.4 * g.unit();
        let mut c = (1.0 - a) - b;
        // Nudge by 0 to 2^30 ulps either way.
        let span = 1u64 << g.below(31);
        let mag = g.below(span) as i64;
        let k = if g.below(2) == 0 { -mag } else { mag };
        c = f64::from_bits((c.to_bits() as i64 + k) as u64);
        let p = [a, b, c];
        let (sgn, res) = residual_sign(p);
        let r = st.signed_distance(p).unwrap();
        // Exact distance to the plane (the foot is inside the face): |res| / sqrt(3) <= |res|.
        let d_exact_hi = res;
        let d_exact_lo = res * 0.577;
        assert!((r.unsigned - d_exact_lo).abs() <= r.bound + (d_exact_hi - d_exact_lo), "{p:?}");
        match r.status {
            SignStatus::Outside => {
                assert_eq!(sgn, 1, "{p:?}");
                signed += 1;
            }
            SignStatus::Inside => {
                assert_eq!(sgn, -1, "{p:?}");
                signed += 1;
            }
            SignStatus::OnSurface => {
                assert_eq!(r.distance.to_bits(), 0);
                assert!(d_exact_lo <= r.bound, "{p:?}: exact distance beyond the bound");
                zero += 1;
            }
            SignStatus::Undecidable => undecided += 1,
            SignStatus::Undefined(u) => panic!("{p:?}: {u:?}"),
        }
    }
    println!("tilted: {signed} signed, {zero} on surface, {undecided} undecidable");
    assert!(signed + zero + undecided == 4000 && signed > 1000 && zero > 0 && undecided > 0);
}

// ------------------------------------------------------------------ errors and determinism

#[test]
fn errors() {
    let m = cube();
    let (mesh, bvh) = build(&m);
    let target = MeshTarget::new(&mesh, &bvh).unwrap();
    let st = SignTarget::new(target).unwrap();
    assert_eq!(st.signed_distance([f64::NAN, 0.0, 0.0]), Err(SignError::Dist(DistError::NonFiniteQuery)));
    assert_eq!(st.signed_distance([1e300, 0.0, 0.0]), Err(SignError::Dist(DistError::QueryOutOfRange)));
    let hit = MeshHit { foot: [0.0; 3], dist: 1.0, dist2: 1.0, tri: 12, feature: Feature::Interior };
    assert_eq!(st.signed([0.0; 3], &hit), Err(SignError::InvalidHit));
    let hit = MeshHit { tri: 0, dist: -1.0, ..hit };
    assert_eq!(st.signed([0.0; 3], &hit), Err(SignError::InvalidHit));

    assert_eq!(SignError::InvalidHit.code(), "invalid_hit");
    assert_eq!(SignUndefined::NonManifoldVertex.code(), "non_manifold_vertex");
}

/// The result depends only on the point and the hit's distance and triangle: a brute-force hit
/// gives the same bits as the indexed one.
#[test]
fn brute_and_indexed_hits_agree() {
    let m = parse_mesh(&read("dist/sphere_concave_convex/mesh.txt"), true);
    let (mesh, bvh) = build(&m);
    let target = MeshTarget::new(&mesh, &bvh).unwrap();
    let st = SignTarget::new(target).unwrap();
    let mut g = SplitMix(42);
    for _ in 0..300 {
        let p = [-2.0 + 4.0 * g.unit(), -2.0 + 4.0 * g.unit(), -2.0 + 4.0 * g.unit()];
        let a = st.signed_distance(p).unwrap();
        let hit = match target.closest_brute(p, &MeshQuery::default()).unwrap() {
            MeshProjection::Hit(h) => h,
            MeshProjection::NoHit => panic!(),
        };
        let b = st.signed(p, &hit).unwrap();
        assert_eq!(
            (a.status, a.distance.to_bits(), a.bound.to_bits()),
            (b.status, b.distance.to_bits(), b.bound.to_bits())
        );
        assert!(a.sign_defined(), "{p:?}: {a:?}");
    }
}
