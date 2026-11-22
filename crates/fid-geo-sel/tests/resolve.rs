//! Acceptance criteria of spec node GEO.SEL.RESOLVE (AC-1 .. AC-6) and REQ-001 .. REQ-006.
//!
//! Includes a completely independent test-only brute-force reference resolver (`mod brute_force`)
//! and checks against the independent stdlib-Python fixture `fixtures/geo/sel/lasso_cases/cases.txt`
//! (seed 72, written by `tools/fixtures/geo_sel_resolve.py`).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use core::cmp::Ordering;
use fid_geo_frame::{Rigid, Transform};
use fid_geo_index::PrimitiveBvh;
use fid_geo_sel::{
    point_in_polygon, resolve_lasso, resolve_lasso_faces, resolve_pick, resolve_pick_faces, resolve_pick_hit, Camera,
    Domain, IndexSet, Lasso, Projection, ResolveMode, SelError, TriangleRule, DEFAULT_DEPTH_TOL_FRACTION,
    MAX_POLYGON_VERTICES,
};
use fid_geo_types::{digest, AttributeSet, FrameRef, GeometryElement, Mesh, PointBuffer, PointSet};
use fid_kci::kci::Sha256;

const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/");

fn read_fixture(path: &str) -> String {
    std::fs::read_to_string(format!("{ROOT}{path}")).unwrap()
}

fn unhex16(s: &str) -> [u8; 16] {
    assert_eq!(s.len(), 32);
    let mut out = [0u8; 16];
    for (i, o) in out.iter_mut().enumerate() {
        *o = u8::from_str_radix(&s[2 * i..2 * i + 2], 16).unwrap();
    }
    out
}

fn hexf(s: &str) -> f64 {
    f64::from_bits(u64::from_str_radix(s, 16).unwrap())
}

fn parse_ranges(s: &str) -> Vec<(u64, u64)> {
    let t = s.trim();
    if t.is_empty() || t == "-" {
        return Vec::new();
    }
    t.split_whitespace()
        .map(|tok| {
            let (a, b) = tok.split_once(':').unwrap();
            (a.parse().unwrap(), b.parse().unwrap())
        })
        .collect()
}

// ------------------------------------------------------------------ fixture elements

fn load_pointset_small() -> (PointSet, GeometryElement) {
    let text = read_fixture("geo/types/pointset_small.txt");
    let mut frame = None;
    let mut buf = PointBuffer::default();
    for line in text.lines() {
        let t = line.trim();
        if let Some(rest) = t.strip_prefix("frame ") {
            frame = Some(FrameRef::from_bytes(unhex16(rest)));
        } else if let Some(rest) = t.strip_prefix("p ") {
            let p: Vec<&str> = rest.split_whitespace().collect();
            buf.x.push(hexf(p[1]));
            buf.y.push(hexf(p[2]));
            buf.z.push(hexf(p[3]));
        }
    }
    let ps = PointSet::new(frame.unwrap(), buf, AttributeSet::default()).unwrap();
    let el = GeometryElement::PointSet(ps.clone());
    (ps, el)
}

/// Exact `mesh_sphere_1e5` from `fixtures/geo/index/make_bvh_fixtures.py` (N_SUB = 71, 100 820 triangles).
fn build_mesh_sphere_1e5() -> Mesh {
    let phi = (1.0_f64 + 5.0_f64.sqrt()) / 2.0;
    let ico_v: [[f64; 3]; 12] = [
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
    let ico_f: [[usize; 3]; 20] = [
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
    let n = 71usize;
    let fn_ = n as f64;
    let mut buf = PointBuffer::default();
    let mut tris = Vec::with_capacity(20 * n * n);
    for [ia, ib, ic] in ico_f {
        let (a, b, c) = (ico_v[ia], ico_v[ib], ico_v[ic]);
        let base = buf.len() as usize;
        for i in 0..=n {
            for j in 0..=(n - i) {
                let (wa, wb, wc) = ((n - i - j) as f64, i as f64, j as f64);
                let px = ((a[0] * wa + b[0] * wb) + c[0] * wc) / fn_;
                let py = ((a[1] * wa + b[1] * wb) + c[1] * wc) / fn_;
                let pz = ((a[2] * wa + b[2] * wb) + c[2] * wc) / fn_;
                let ln = ((px * px + py * py) + pz * pz).sqrt();
                buf.x.push(px / ln);
                buf.y.push(py / ln);
                buf.z.push(pz / ln);
            }
        }
        let idx = |i: usize, j: usize| -> u64 { (base + i * (n + 1) - (i * i.saturating_sub(1)) / 2 + j) as u64 };
        for i in 0..n {
            for j in 0..(n - i) {
                tris.push([idx(i, j), idx(i + 1, j), idx(i, j + 1)]);
                if j + 1 < n - i {
                    tris.push([idx(i + 1, j), idx(i + 1, j + 1), idx(i, j + 1)]);
                }
            }
        }
    }
    assert_eq!(tris.len(), 100_820);
    Mesh::new(FrameRef::from_bytes([0x11; 16]), buf, AttributeSet::default(), tris).unwrap()
}

// ------------------------------------------------------------------ fixture parser

struct LassoCase {
    index: usize,
    concave: bool,
    camera: Camera,
    lasso: Lasso,
    ps_through: Vec<(u64, u64)>,
    ps_visible: Vec<(u64, u64)>,
}

struct PickCase {
    index: usize,
    camera: Camera,
    pixel: (f64, f64),
    expected_tri: Option<u64>,
    expected_t_bits: Option<u64>,
}

struct EdgeCase {
    name: String,
    polygon: Vec<(f64, f64)>,
    points: Vec<((f64, f64), bool)>,
}

struct Fixture {
    lassos: Vec<LassoCase>,
    picks: Vec<PickCase>,
    edges: Vec<EdgeCase>,
}

fn parse_camera(toks: &[&str]) -> Camera {
    assert_eq!(toks.len(), 19);
    let kind = toks[0];
    let w: u32 = toks[1].parse().unwrap();
    let h: u32 = toks[2].parse().unwrap();
    let p0 = hexf(toks[3]);
    let aspect = hexf(toks[4]);
    let near = hexf(toks[5]);
    let far = hexf(toks[6]);
    let r = [
        [hexf(toks[7]), hexf(toks[8]), hexf(toks[9])],
        [hexf(toks[10]), hexf(toks[11]), hexf(toks[12])],
        [hexf(toks[13]), hexf(toks[14]), hexf(toks[15])],
    ];
    let t = [hexf(toks[16]), hexf(toks[17]), hexf(toks[18])];
    let view = Rigid::new(r, t).unwrap();
    let proj = match kind {
        "persp" => Projection::Perspective { fov_y: p0, aspect, near, far },
        "ortho" => Projection::Ortho { height: p0, aspect, near, far },
        other => panic!("unknown camera kind {other}"),
    };
    Camera { view, proj, viewport: (w, h) }
}

fn load_fixture() -> Fixture {
    let text = read_fixture("geo/sel/lasso_cases/cases.txt");
    let mut lassos = Vec::new();
    let mut picks = Vec::new();
    let mut edges = Vec::new();
    for line in text.lines() {
        let t = line.trim();
        if t.is_empty() || t.starts_with('#') || t.starts_with("seed ") || t.contains("_count ") {
            continue;
        }
        if let Some(rest) = t.strip_prefix("lasso ") {
            let parts: Vec<&str> = rest.split('|').collect();
            assert_eq!(parts.len(), 4);
            let head: Vec<&str> = parts[0].split_whitespace().collect();
            let index: usize = head[0].parse().unwrap();
            let concave = match head[1] {
                "convex" => false,
                "concave" => true,
                other => panic!("bad shape {other}"),
            };
            let camera = parse_camera(&head[2..]);
            let ptoks: Vec<&str> = parts[1].split_whitespace().collect();
            let n_verts: usize = ptoks[0].parse().unwrap();
            assert_eq!(ptoks.len(), 1 + 2 * n_verts);
            let mut pixels = Vec::with_capacity(n_verts);
            for k in 0..n_verts {
                pixels.push((hexf(ptoks[1 + 2 * k]), hexf(ptoks[2 + 2 * k])));
            }
            let ps_through = parse_ranges(parts[2]);
            let ps_visible = parse_ranges(parts[3]);
            lassos.push(LassoCase { index, concave, camera, lasso: Lasso { pixels }, ps_through, ps_visible });
        } else if let Some(rest) = t.strip_prefix("pick ") {
            let parts: Vec<&str> = rest.split('|').collect();
            assert_eq!(parts.len(), 3);
            let head: Vec<&str> = parts[0].split_whitespace().collect();
            let index: usize = head[0].parse().unwrap();
            let camera = parse_camera(&head[1..]);
            let ptoks: Vec<&str> = parts[1].split_whitespace().collect();
            let pixel = (hexf(ptoks[0]), hexf(ptoks[1]));
            let htoks: Vec<&str> = parts[2].split_whitespace().collect();
            let (expected_tri, expected_t_bits) = if htoks[0] == "none" {
                (None, None)
            } else {
                (Some(htoks[0].parse().unwrap()), Some(u64::from_str_radix(htoks[1], 16).unwrap()))
            };
            picks.push(PickCase { index, camera, pixel, expected_tri, expected_t_bits });
        } else if let Some(rest) = t.strip_prefix("edge ") {
            let parts: Vec<&str> = rest.split('|').collect();
            assert_eq!(parts.len(), 2);
            let head: Vec<&str> = parts[0].split_whitespace().collect();
            let name = head[0].to_string();
            let n_verts: usize = head[1].parse().unwrap();
            let mut polygon = Vec::with_capacity(n_verts);
            for k in 0..n_verts {
                polygon.push((hexf(head[2 + 2 * k]), hexf(head[3 + 2 * k])));
            }
            let ptoks: Vec<&str> = parts[1].split_whitespace().collect();
            let n_pts: usize = ptoks[0].parse().unwrap();
            let mut points = Vec::with_capacity(n_pts);
            for k in 0..n_pts {
                let px = hexf(ptoks[1 + 3 * k]);
                let py = hexf(ptoks[2 + 3 * k]);
                let inside = ptoks[3 + 3 * k] == "1";
                points.push(((px, py), inside));
            }
            edges.push(EdgeCase { name, polygon, points });
        }
    }
    Fixture { lassos, picks, edges }
}

// ------------------------------------------------------------------ independent brute-force reference

/// Test-only brute-force reference resolver sharing zero code with `src/resolve.rs` or `src/raster.rs`.
mod brute_force {
    use super::*;

    struct RefCam {
        r: [[f64; 3]; 3],
        t: [f64; 3],
        persp: bool,
        half_w: f64,
        half_h: f64,
        near: f64,
        far: f64,
        w: u32,
        h: u32,
        wf: f64,
        hf: f64,
    }

    impl RefCam {
        fn new(cam: &Camera) -> RefCam {
            let (persp, half_w, half_h, near, far) = match cam.proj {
                Projection::Perspective { fov_y, aspect, near, far } => {
                    let hh = fid_num::f::tan(fov_y * 0.5).unwrap();
                    (true, hh * aspect, hh, near, far)
                }
                Projection::Ortho { height, aspect, near, far } => {
                    let hh = height * 0.5;
                    (false, hh * aspect, hh, near, far)
                }
            };
            RefCam {
                r: cam.view.r(),
                t: cam.view.t(),
                persp,
                half_w,
                half_h,
                near,
                far,
                w: cam.viewport.0,
                h: cam.viewport.1,
                wf: f64::from(cam.viewport.0),
                hf: f64::from(cam.viewport.1),
            }
        }

        #[inline]
        fn view(&self, p: [f64; 3]) -> [f64; 3] {
            let r = &self.r;
            let t = &self.t;
            [
                ((r[0][0] * p[0] + r[0][1] * p[1]) + r[0][2] * p[2]) + t[0],
                ((r[1][0] * p[0] + r[1][1] * p[1]) + r[1][2] * p[2]) + t[1],
                ((r[2][0] * p[0] + r[2][1] * p[1]) + r[2][2] * p[2]) + t[2],
            ]
        }

        #[inline]
        fn proj(&self, c: [f64; 3]) -> Option<(f64, f64, f64)> {
            let d = -c[2];
            if d < self.near || d > self.far {
                return None;
            }
            let (ndc_x, ndc_y) = if self.persp {
                (c[0] / (d * self.half_w), c[1] / (d * self.half_h))
            } else {
                (c[0] / self.half_w, c[1] / self.half_h)
            };
            Some(((ndc_x + 1.0) * 0.5 * self.wf, (1.0 - ndc_y) * 0.5 * self.hf, d))
        }
    }

    #[inline]
    fn pip(px: f64, py: f64, poly: &[(f64, f64)]) -> bool {
        let mut inside = false;
        let mut a = *poly.last().unwrap();
        for &b in poly {
            let (x0, y0, x1, y1) = if a.1 < b.1 { (a.0, a.1, b.0, b.1) } else { (b.0, b.1, a.0, a.1) };
            if y0 <= py && py < y1 && (x1 - x0) * (py - y0) > (px - x0) * (y1 - y0) {
                inside = !inside;
            }
            a = b;
        }
        inside
    }

    fn clip_plane(poly: &[[f64; 3]], plane: f64, keep_ge: bool) -> Vec<[f64; 3]> {
        let mut out = Vec::new();
        if poly.is_empty() {
            return out;
        }
        let inside = |v: [f64; 3]| if keep_ge { -v[2] >= plane } else { -v[2] <= plane };
        let mut prev = *poly.last().unwrap();
        let mut prev_in = inside(prev);
        for &cur in poly {
            let cur_in = inside(cur);
            if cur_in != prev_in {
                let da = -prev[2];
                let db = -cur[2];
                let t = (plane - da) / (db - da);
                out.push([prev[0] + t * (cur[0] - prev[0]), prev[1] + t * (cur[1] - prev[1]), -plane]);
            }
            if cur_in {
                out.push(cur);
            }
            prev = cur;
            prev_in = cur_in;
        }
        out
    }

    #[inline]
    fn raster_tri(
        zbuf: &mut [f64],
        w: u32,
        h: u32,
        persp: bool,
        v0: (f64, f64, f64),
        mut v1: (f64, f64, f64),
        mut v2: (f64, f64, f64),
    ) {
        let mut area2 = (v1.0 - v0.0) * (v2.1 - v0.1) - (v1.1 - v0.1) * (v2.0 - v0.0);
        if area2 == 0.0 || !area2.is_finite() {
            return;
        }
        if area2 < 0.0 {
            core::mem::swap(&mut v1, &mut v2);
            area2 = -area2;
        }
        let min_x = v0.0.min(v1.0).min(v2.0);
        let max_x = v0.0.max(v1.0).max(v2.0);
        let min_y = v0.1.min(v1.1).min(v2.1);
        let max_y = v0.1.max(v1.1).max(v2.1);
        if max_x < 0.5 || min_x > f64::from(w) - 0.5 || max_y < 0.5 || min_y > f64::from(h) - 0.5 {
            return;
        }
        let c0 = ((min_x - 0.5).ceil().max(0.0) as u32).min(w - 1);
        let c1 = ((max_x - 0.5).floor().max(0.0) as u32).min(w - 1);
        let r0 = ((min_y - 0.5).ceil().max(0.0) as u32).min(h - 1);
        let r1 = ((max_y - 0.5).floor().max(0.0) as u32).min(h - 1);
        for row in r0..=r1 {
            let py = f64::from(row) + 0.5;
            for col in c0..=c1 {
                let px = f64::from(col) + 0.5;
                let w0 = (v2.0 - v1.0) * (py - v1.1) - (v2.1 - v1.1) * (px - v1.0);
                let w1 = (v0.0 - v2.0) * (py - v2.1) - (v0.1 - v2.1) * (px - v2.0);
                let w2 = (v1.0 - v0.0) * (py - v0.1) - (v1.1 - v0.1) * (px - v0.0);
                if w0 >= 0.0 && w1 >= 0.0 && w2 >= 0.0 {
                    let l0 = w0 / area2;
                    let l1 = w1 / area2;
                    let l2 = w2 / area2;
                    let d = if persp {
                        1.0 / ((l0 / v0.2 + l1 / v1.2) + l2 / v2.2)
                    } else {
                        (l0 * v0.2 + l1 * v1.2) + l2 * v2.2
                    };
                    let idx = (row as usize) * (w as usize) + (col as usize);
                    if d.total_cmp(&zbuf[idx]) == Ordering::Less {
                        zbuf[idx] = d;
                    }
                }
            }
        }
    }

    pub fn build_mesh_zbuf(verts: &[[f64; 3]], tris: &[[usize; 3]], cam: &Camera) -> Vec<f64> {
        let rc = RefCam::new(cam);
        let mut zbuf = vec![f64::INFINITY; (rc.w as usize) * (rc.h as usize)];
        for &[i0, i1, i2] in tris {
            let c0 = rc.view(verts[i0]);
            let c1 = rc.view(verts[i1]);
            let c2 = rc.view(verts[i2]);
            let (d0, d1, d2) = (-c0[2], -c1[2], -c2[2]);
            if d0 >= rc.near && d0 <= rc.far && d1 >= rc.near && d1 <= rc.far && d2 >= rc.near && d2 <= rc.far {
                if let (Some(v0), Some(v1), Some(v2)) = (rc.proj(c0), rc.proj(c1), rc.proj(c2)) {
                    raster_tri(&mut zbuf, rc.w, rc.h, rc.persp, v0, v1, v2);
                }
                continue;
            }
            if (d0 < rc.near && d1 < rc.near && d2 < rc.near) || (d0 > rc.far && d1 > rc.far && d2 > rc.far) {
                continue;
            }
            let p1 = clip_plane(&[c0, c1, c2], rc.near, true);
            let p2 = clip_plane(&p1, rc.far, false);
            if p2.len() < 3 {
                continue;
            }
            let sp: Vec<(f64, f64, f64)> = p2.iter().filter_map(|&c| rc.proj(c)).collect();
            if sp.len() != p2.len() {
                continue;
            }
            for k in 1..(sp.len() - 1) {
                raster_tri(&mut zbuf, rc.w, rc.h, rc.persp, sp[0], sp[k], sp[k + 1]);
            }
        }
        zbuf
    }

    #[inline]
    fn is_vis(zbuf: &[f64], w: u32, h: u32, sx: f64, sy: f64, d: f64, tol: f64) -> bool {
        if sx < 0.0 || sx >= f64::from(w) || sy < 0.0 || sy >= f64::from(h) {
            return false;
        }
        let col = sx.floor() as usize;
        let row = sy.floor() as usize;
        let bd = zbuf[row * (w as usize) + col];
        bd.is_infinite() || d.total_cmp(&(bd + tol)) != Ordering::Greater
    }

    pub fn resolve_points(ps: &PointSet, cam: &Camera, poly: &[(f64, f64)], visible: bool) -> IndexSet {
        let rc = RefCam::new(cam);
        let tol = (rc.far - rc.near) * DEFAULT_DEPTH_TOL_FRACTION;
        let zbuf = if visible {
            let mut z = vec![f64::INFINITY; (rc.w as usize) * (rc.h as usize)];
            for i in 0..ps.len() {
                if let Some((sx, sy, d)) = rc.proj(rc.view(ps.point(i).unwrap())) {
                    if sx >= 0.0 && sx < rc.wf && sy >= 0.0 && sy < rc.hf {
                        let idx = (sy.floor() as usize) * (rc.w as usize) + (sx.floor() as usize);
                        if d.total_cmp(&z[idx]) == Ordering::Less {
                            z[idx] = d;
                        }
                    }
                }
            }
            Some(z)
        } else {
            None
        };
        let mut hits = Vec::new();
        for i in 0..ps.len() {
            if let Some((sx, sy, d)) = rc.proj(rc.view(ps.point(i).unwrap())) {
                if pip(sx, sy, poly) && zbuf.as_ref().is_none_or(|z| is_vis(z, rc.w, rc.h, sx, sy, d, tol)) {
                    hits.push(i);
                }
            }
        }
        IndexSet::from_indices(Domain::Points, &hits, ps.len()).unwrap()
    }

    pub fn resolve_mesh_tris(
        verts: &[[f64; 3]],
        tris: &[[usize; 3]],
        cam: &Camera,
        poly: &[(f64, f64)],
        rule: TriangleRule,
        zbuf: Option<&[f64]>,
    ) -> IndexSet {
        let rc = RefCam::new(cam);
        let tol = (rc.far - rc.near) * DEFAULT_DEPTH_TOL_FRACTION;
        let test_pt = |p: [f64; 3]| -> bool {
            let Some((sx, sy, d)) = rc.proj(rc.view(p)) else {
                return false;
            };
            if !pip(sx, sy, poly) {
                return false;
            }
            zbuf.is_none_or(|z| is_vis(z, rc.w, rc.h, sx, sy, d, tol))
        };
        let mut hits = Vec::new();
        for (t, &[i0, i1, i2]) in tris.iter().enumerate() {
            let a = verts[i0];
            let b = verts[i1];
            let c = verts[i2];
            let ok = match rule {
                TriangleRule::AllVertices => test_pt(a) && test_pt(b) && test_pt(c),
                TriangleRule::AnyVertex => test_pt(a) || test_pt(b) || test_pt(c),
                TriangleRule::Centroid => {
                    test_pt([((a[0] + b[0]) + c[0]) / 3.0, ((a[1] + b[1]) + c[1]) / 3.0, ((a[2] + b[2]) + c[2]) / 3.0])
                }
            };
            if ok {
                hits.push(t as u64);
            }
        }
        IndexSet::from_indices(Domain::Triangles, &hits, tris.len() as u64).unwrap()
    }

    pub fn pick_mesh_bruteforce(mesh: &Mesh, cam: &Camera, pixel: (f64, f64)) -> Option<(u64, f64)> {
        let (w, h) = (f64::from(cam.viewport.0), f64::from(cam.viewport.1));
        let ndc_x = (pixel.0 / w) * 2.0 - 1.0;
        let ndc_y = 1.0 - (pixel.1 / h) * 2.0;
        let (o_cam, d_cam) = match cam.proj {
            Projection::Perspective { fov_y, aspect, .. } => {
                let hh = fid_num::f::tan(fov_y * 0.5).unwrap();
                ([0.0, 0.0, 0.0], [ndc_x * (hh * aspect), ndc_y * hh, -1.0])
            }
            Projection::Ortho { height, aspect, .. } => {
                let hh = height * 0.5;
                ([ndc_x * (hh * aspect), ndc_y * hh, 0.0], [0.0, 0.0, -1.0])
            }
        };
        let inv = Transform::from(cam.view).inverse().unwrap();
        let o = inv.apply_point(&o_cam).unwrap();
        let d = inv.apply_dir(&d_cam).unwrap();
        let (near, far) = cam.near_far();

        let dot = |u: [f64; 3], v: [f64; 3]| (u[0] * v[0] + u[1] * v[1]) + u[2] * v[2];
        let cross = |u: [f64; 3], v: [f64; 3]| {
            [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]]
        };
        let sub = |u: [f64; 3], v: [f64; 3]| [u[0] - v[0], u[1] - v[1], u[2] - v[2]];

        let mut best: Option<(f64, u64)> = None;
        for t_idx in 0..mesh.triangle_count() {
            let [i0, i1, i2] = mesh.triangle(t_idx).unwrap();
            let a = mesh.vertex(i0).unwrap();
            let b = mesh.vertex(i1).unwrap();
            let c = mesh.vertex(i2).unwrap();
            let e1 = sub(b, a);
            let e2 = sub(c, a);
            let pvec = cross(d, e2);
            let det = dot(e1, pvec);
            if det == 0.0 {
                continue;
            }
            let inv_det = 1.0 / det;
            let tvec = sub(o, a);
            let u = dot(tvec, pvec) * inv_det;
            if !(0.0..=1.0).contains(&u) {
                continue;
            }
            let qvec = cross(tvec, e1);
            let v = dot(d, qvec) * inv_det;
            if v < 0.0 || u + v > 1.0 {
                continue;
            }
            let t = dot(e2, qvec) * inv_det;
            if t < near || t > far {
                continue;
            }
            let better = match best {
                None => true,
                Some((bt, bi)) => t.total_cmp(&bt).then(t_idx.cmp(&bi)) == Ordering::Less,
            };
            if better {
                best = Some((t, t_idx));
            }
        }
        best.map(|(t, idx)| (idx, t))
    }
}

// ------------------------------------------------------------------ AC-1 & AC-4

#[test]
fn ac1_pointset_200_seeded_lassos_match_brute_force_and_python_reference() {
    let fix = load_fixture();
    assert_eq!(fix.lassos.len(), 200);
    assert_eq!(fix.lassos.iter().filter(|c| !c.concave).count(), 100);
    assert_eq!(fix.lassos.iter().filter(|c| c.concave).count(), 100);

    let (ps, ps_el) = load_pointset_small();
    let ps_digest = digest(&ps_el);

    let mut acc = Sha256::new();
    let mut ps_nonempty = 0usize;
    let mut ps_occluded = 0usize;

    for case in &fix.lassos {
        let got_t = resolve_lasso(
            &ps_el,
            &case.camera,
            &case.lasso,
            Domain::Points,
            TriangleRule::AllVertices,
            ResolveMode::Through,
        )
        .unwrap();
        let bf_t = brute_force::resolve_points(&ps, &case.camera, &case.lasso.pixels, false);
        assert_eq!(got_t, bf_t, "pointset through case {} vs rust brute force", case.index);
        assert_eq!(got_t.ranges(), case.ps_through.as_slice(), "pointset through case {} vs python ref", case.index);

        let got_v = resolve_lasso(
            &ps_el,
            &case.camera,
            &case.lasso,
            Domain::Points,
            TriangleRule::AllVertices,
            ResolveMode::visible_default(),
        )
        .unwrap();
        let bf_v = brute_force::resolve_points(&ps, &case.camera, &case.lasso.pixels, true);
        assert_eq!(got_v, bf_v, "pointset visible case {} vs rust brute force", case.index);
        assert_eq!(got_v.ranges(), case.ps_visible.as_slice(), "pointset visible case {} vs python ref", case.index);

        if !got_t.is_empty() {
            ps_nonempty += 1;
        }
        if got_v.len() < got_t.len() {
            ps_occluded += 1;
        }
        acc.update(&got_t.digest(ps_digest).to_bytes());
        acc.update(&got_v.digest(ps_digest).to_bytes());
    }
    assert!(ps_nonempty >= 40, "expected >= 40 non-empty pointset selections, got {ps_nonempty}");
    assert!(ps_occluded >= 20, "expected >= 20 occluded pointset selections, got {ps_occluded}");

    // Repeat check: digest must be stable across calls.
    let mut acc2 = Sha256::new();
    for case in &fix.lassos {
        let gt = resolve_lasso(
            &ps_el,
            &case.camera,
            &case.lasso,
            Domain::Points,
            TriangleRule::AllVertices,
            ResolveMode::Through,
        )
        .unwrap();
        let gv = resolve_lasso(
            &ps_el,
            &case.camera,
            &case.lasso,
            Domain::Points,
            TriangleRule::AllVertices,
            ResolveMode::visible_default(),
        )
        .unwrap();
        acc2.update(&gt.digest(ps_digest).to_bytes());
        acc2.update(&gv.digest(ps_digest).to_bytes());
    }
    assert_eq!(acc.finish(), acc2.finish());
}

fn check_sphere_chunk(start: usize, end: usize, expected_hex: &str) {
    let fix = load_fixture();
    let sphere = build_mesh_sphere_1e5();
    let sphere_el = GeometryElement::Mesh(sphere.clone());
    let sphere_digest = digest(&sphere_el);

    let sphere_verts: Vec<[f64; 3]> = (0..sphere.vertex_count()).map(|i| sphere.vertex(i).unwrap()).collect();
    let sphere_tris: Vec<[usize; 3]> = (0..sphere.triangle_count())
        .map(|t| {
            let [a, b, c] = sphere.triangle(t).unwrap();
            [a as usize, b as usize, c as usize]
        })
        .collect();

    let mut acc = Sha256::new();
    let mut sphere_nonempty = 0usize;
    let mut sphere_occluded = 0usize;

    for case in &fix.lassos[start..end] {
        let bf_zbuf = brute_force::build_mesh_zbuf(&sphere_verts, &sphere_tris, &case.camera);
        for rule in TriangleRule::ALL {
            let got_t =
                resolve_lasso(&sphere_el, &case.camera, &case.lasso, Domain::Triangles, rule, ResolveMode::Through)
                    .unwrap();
            let bf_t = brute_force::resolve_mesh_tris(
                &sphere_verts,
                &sphere_tris,
                &case.camera,
                &case.lasso.pixels,
                rule,
                None,
            );
            assert_eq!(got_t, bf_t, "sphere through case {} rule {:?} vs brute force", case.index, rule);

            let got_v = resolve_lasso(
                &sphere_el,
                &case.camera,
                &case.lasso,
                Domain::Triangles,
                rule,
                ResolveMode::visible_default(),
            )
            .unwrap();
            let bf_v = brute_force::resolve_mesh_tris(
                &sphere_verts,
                &sphere_tris,
                &case.camera,
                &case.lasso.pixels,
                rule,
                Some(&bf_zbuf),
            );
            assert_eq!(got_v, bf_v, "sphere visible case {} rule {:?} vs brute force", case.index, rule);

            if rule == TriangleRule::Centroid && !got_t.is_empty() {
                sphere_nonempty += 1;
                if got_v.len() < got_t.len() {
                    sphere_occluded += 1;
                }
            }
            acc.update(&got_t.digest(sphere_digest).to_bytes());
            acc.update(&got_v.digest(sphere_digest).to_bytes());
        }
    }
    assert!(sphere_nonempty >= 35, "chunk {start}..{end}: expected >= 35 non-empty, got {sphere_nonempty}");
    assert!(sphere_occluded >= 35, "chunk {start}..{end}: expected >= 35 occluded, got {sphere_occluded}");

    let d = acc.finish();
    let hex: String = d.iter().map(|b| format!("{b:02x}")).collect();
    assert_eq!(hex, expected_hex, "AC-4 cross-target digest drifted for chunk {start}..{end}");
}

#[test]
fn ac1_and_ac4_sphere_chunk_0() {
    check_sphere_chunk(0, 50, "f4b2918d47a1e4f9c1e6af47b76dcd5a1b3a8660a2386754add3746773330e33");
}

#[test]
fn ac1_and_ac4_sphere_chunk_1() {
    check_sphere_chunk(50, 100, "fbc4bcbcad1cdb2319341e33027edbf82e3ea3f16acc33a1ff6a58e49bfdc263");
}

#[test]
fn ac1_and_ac4_sphere_chunk_2() {
    check_sphere_chunk(100, 150, "c0279e53182223b1183d767f8f902dc9dbf350db593660e7125043b4ff937e2f");
}

#[test]
fn ac1_and_ac4_sphere_chunk_3() {
    check_sphere_chunk(150, 200, "ec135e226959957b5c9a6ad3c4fcd676069d6af28207bc25c7184c79f5a9357f");
}

// ------------------------------------------------------------------ AC-2

#[test]
fn ac2_100_seeded_picks_match_brute_force_and_python_reference() {
    let fix = load_fixture();
    assert_eq!(fix.picks.len(), 100);

    let sphere = build_mesh_sphere_1e5();
    let sphere_el = GeometryElement::Mesh(sphere.clone());
    let payload: Vec<u64> = (0..sphere.triangle_count()).collect();
    let bvh = PrimitiveBvh::build(&sphere, &payload, 1).unwrap();

    let mut hit_count = 0usize;
    let mut miss_count = 0usize;
    for case in &fix.picks {
        let got_hit =
            resolve_pick_hit(&sphere_el, Some(&bvh), &case.camera, case.pixel, 0.0, Domain::Triangles).unwrap();
        let got_set = resolve_pick(&sphere_el, Some(&bvh), &case.camera, case.pixel, 0.0, Domain::Triangles).unwrap();
        let bf = brute_force::pick_mesh_bruteforce(&sphere, &case.camera, case.pixel);

        match (got_hit, bf) {
            (None, None) => {
                assert!(got_set.is_empty());
                assert_eq!(case.expected_tri, None, "pick {} expected hit vs none", case.index);
                miss_count += 1;
            }
            (Some(h), Some((bf_tri, bf_t))) => {
                assert_eq!(h.domain, Domain::Triangles);
                assert_eq!(h.index, bf_tri, "pick {} tri mismatch vs rust brute force", case.index);
                assert_eq!(
                    h.depth.to_bits(),
                    bf_t.to_bits(),
                    "pick {} t bits mismatch vs rust brute force",
                    case.index
                );
                assert_eq!(Some(h.index), case.expected_tri, "pick {} tri mismatch vs python ref", case.index);
                assert_eq!(
                    Some(h.depth.to_bits()),
                    case.expected_t_bits,
                    "pick {} t bits mismatch vs python ref",
                    case.index
                );
                assert_eq!(got_set.ranges(), &[(h.index, h.index + 1)]);
                hit_count += 1;
            }
            (a, b) => panic!("pick {} disagreement: resolver={a:?} brute_force={b:?}", case.index),
        }
    }
    assert!(hit_count >= 75, "expected >= 75 sphere pick hits, got {hit_count}");
    assert!(miss_count >= 5, "expected >= 5 sphere pick misses, got {miss_count}");

    // Also test point pick with pixel tolerance and tie-breaking by depth -> d2 -> index.
    let frame = FrameRef::from_bytes([0x22; 16]);
    let ps = PointSet::from_points(
        frame,
        &[
            [0.0, 0.0, -5.0], // index 0: depth 5, pixel (50, 50)
            [0.0, 0.0, -3.0], // index 1: depth 3, pixel (50, 50) -> nearest!
            [0.0, 0.0, -3.0], // index 2: tie depth & pixel -> loses to index 1
            [0.5, 0.0, -2.0], // index 3: outside 2px tol
        ],
    )
    .unwrap();
    let el = GeometryElement::PointSet(ps);
    let cam = Camera {
        view: Rigid::IDENTITY,
        proj: Projection::Ortho { height: 2.0, aspect: 1.0, near: 1.0, far: 10.0 },
        viewport: (100, 100),
    };
    let picked = resolve_pick(&el, None, &cam, (50.0, 50.0), 2.0, Domain::Points).unwrap();
    assert_eq!(picked.ranges(), &[(1, 2)]);
}

// ------------------------------------------------------------------ AC-3

#[test]
fn ac3_points_on_lasso_edges_follow_half_open_convention() {
    let fix = load_fixture();
    assert_eq!(fix.edges.len(), 6);

    // Power-of-two orthographic camera (height = 128, viewport = 128x128, half_h = 64):
    // division by 64 and multiplication by 64 are exact exponent shifts in IEEE-754 binary64,
    // so world [px - 64.0, 64.0 - py, -5.0] projects to pixel (px, py) with zero rounding error.
    let cam = Camera {
        view: Rigid::IDENTITY,
        proj: Projection::Ortho { height: 128.0, aspect: 1.0, near: 1.0, far: 10.0 },
        viewport: (128, 128),
    };

    let mut upper_results = Vec::new();
    let mut lower_results = Vec::new();

    for ec in &fix.edges {
        let mut world_pts = Vec::with_capacity(ec.points.len());
        let mut expected_indices = Vec::new();
        for (idx, &((px, py), expected)) in ec.points.iter().enumerate() {
            let got = point_in_polygon(px, py, &ec.polygon).unwrap();
            assert_eq!(got, expected, "edge case {} point ({px}, {py})", ec.name);
            world_pts.push([px - 64.0, 64.0 - py, -5.0]);
            if expected {
                expected_indices.push(idx as u64);
            }
            if ec.name == "shared_diag_upper" {
                upper_results.push(got);
            } else if ec.name == "shared_diag_lower" {
                lower_results.push(got);
            }
        }
        // End-to-end through resolve_lasso on PointSet as well.
        let ps = PointSet::from_points(FrameRef::from_bytes([0x33; 16]), &world_pts).unwrap();
        let el = GeometryElement::PointSet(ps);
        let lasso = Lasso::new(ec.polygon.clone()).unwrap();
        let got_set =
            resolve_lasso(&el, &cam, &lasso, Domain::Points, TriangleRule::AllVertices, ResolveMode::Through).unwrap();
        let want_set = IndexSet::from_indices(Domain::Points, &expected_indices, world_pts.len() as u64).unwrap();
        assert_eq!(got_set, want_set, "edge case {} end-to-end resolve_lasso", ec.name);
    }

    // Every point on the shared diagonal between two adjacent triangles must belong to EXACTLY ONE.
    assert_eq!(upper_results.len(), lower_results.len());
    assert!(!upper_results.is_empty());
    for (i, (&u, &l)) in upper_results.iter().zip(&lower_results).enumerate() {
        assert_ne!(u, l, "shared diagonal point {i} belonged to both or neither triangle");
    }
}

// ------------------------------------------------------------------ AC-5

#[test]
fn ac5_error_paths_return_typed_errors() {
    let frame = FrameRef::from_bytes([0x44; 16]);
    let ps = PointSet::from_points(frame, &[[0.0, 0.0, -5.0]]).unwrap();
    let el = GeometryElement::PointSet(ps);
    let ok_lasso = Lasso::new(vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0)]).unwrap();
    let ok_cam = Camera {
        view: Rigid::IDENTITY,
        proj: Projection::Perspective { fov_y: 1.0, aspect: 1.0, near: 0.5, far: 10.0 },
        viewport: (64, 64),
    };

    // near == far -> BadCamera
    let cam_eq = Camera {
        view: Rigid::IDENTITY,
        proj: Projection::Perspective { fov_y: 1.0, aspect: 1.0, near: 5.0, far: 5.0 },
        viewport: (64, 64),
    };
    assert!(matches!(
        resolve_lasso(&el, &cam_eq, &ok_lasso, Domain::Points, TriangleRule::AllVertices, ResolveMode::Through),
        Err(SelError::BadCamera { .. })
    ));

    // near > far -> BadCamera
    let cam_gt = Camera {
        view: Rigid::IDENTITY,
        proj: Projection::Ortho { height: 2.0, aspect: 1.0, near: 8.0, far: 2.0 },
        viewport: (64, 64),
    };
    assert!(matches!(
        resolve_lasso(&el, &cam_gt, &ok_lasso, Domain::Points, TriangleRule::AllVertices, ResolveMode::Through),
        Err(SelError::BadCamera { .. })
    ));

    // Zero viewport -> BadCamera
    let cam_zero_vp = Camera { viewport: (0, 64), ..ok_cam };
    assert!(matches!(
        resolve_lasso(&el, &cam_zero_vp, &ok_lasso, Domain::Points, TriangleRule::AllVertices, ResolveMode::Through),
        Err(SelError::BadCamera { .. })
    ));

    // Non-finite fov_y -> BadCamera
    let cam_nan = Camera {
        view: Rigid::IDENTITY,
        proj: Projection::Perspective { fov_y: f64::NAN, aspect: 1.0, near: 0.5, far: 10.0 },
        viewport: (64, 64),
    };
    assert!(matches!(
        resolve_lasso(&el, &cam_nan, &ok_lasso, Domain::Points, TriangleRule::AllVertices, ResolveMode::Through),
        Err(SelError::BadCamera { .. })
    ));

    // 2-vertex polygon -> BadPolygon
    let poly_2 = Lasso { pixels: vec![(0.0, 0.0), (10.0, 10.0)] };
    assert!(matches!(
        resolve_lasso(&el, &ok_cam, &poly_2, Domain::Points, TriangleRule::AllVertices, ResolveMode::Through),
        Err(SelError::BadPolygon { .. })
    ));
    assert!(matches!(Lasso::new(vec![(0.0, 0.0), (10.0, 10.0)]), Err(SelError::BadPolygon { .. })));

    // 4 097-vertex polygon -> BadPolygon, while 4 096 vertices succeeds
    let poly_4097 = Lasso { pixels: vec![(1.0, 1.0); MAX_POLYGON_VERTICES + 1] };
    assert!(matches!(
        resolve_lasso(&el, &ok_cam, &poly_4097, Domain::Points, TriangleRule::AllVertices, ResolveMode::Through),
        Err(SelError::BadPolygon { .. })
    ));
    let poly_4096 = Lasso::new(vec![(1.0, 1.0); MAX_POLYGON_VERTICES]).unwrap();
    assert!(resolve_lasso(&el, &ok_cam, &poly_4096, Domain::Points, TriangleRule::AllVertices, ResolveMode::Through)
        .is_ok());

    // Non-finite polygon vertex -> BadPolygon
    let poly_nan = Lasso { pixels: vec![(0.0, 0.0), (f64::INFINITY, 0.0), (5.0, 5.0)] };
    assert!(matches!(
        resolve_lasso(&el, &ok_cam, &poly_nan, Domain::Points, TriangleRule::AllVertices, ResolveMode::Through),
        Err(SelError::BadPolygon { .. })
    ));

    // Empty target -> EmptyTarget
    let empty_ps = PointSet::new(frame, PointBuffer::default(), AttributeSet::default()).unwrap();
    let empty_el = GeometryElement::PointSet(empty_ps);
    assert_eq!(
        resolve_lasso(&empty_el, &ok_cam, &ok_lasso, Domain::Points, TriangleRule::AllVertices, ResolveMode::Through),
        Err(SelError::EmptyTarget)
    );

    // Negative depth_tol -> BadTolerance
    assert!(matches!(
        resolve_lasso(
            &el,
            &ok_cam,
            &ok_lasso,
            Domain::Points,
            TriangleRule::AllVertices,
            ResolveMode::Visible { depth_tol: Some(-1.0) },
        ),
        Err(SelError::BadTolerance { .. })
    ));

    // Domain mismatch: Triangles on PointSet
    assert!(matches!(
        resolve_lasso(&el, &ok_cam, &ok_lasso, Domain::Triangles, TriangleRule::AllVertices, ResolveMode::Through),
        Err(SelError::DomainMismatch { .. })
    ));
}

// ------------------------------------------------------------------ AC-6

#[test]
fn ac6_face_domain_lasso_and_pick() {
    let frame = FrameRef::from_bytes([0x55; 16]);
    // Synthetic two-face mesh:
    //   Face 10 (left quad, x in [-0.8, -0.1], y in [-0.4, 0.4], z = -5.0): triangles 0, 1
    //   Face 20 (right quad, x in [0.1, 0.8], y in [-0.4, 0.4], z = -5.0): triangles 2, 3
    let verts = [
        [-0.8, -0.4, -5.0],
        [-0.1, -0.4, -5.0],
        [-0.1, 0.4, -5.0],
        [-0.8, 0.4, -5.0],
        [0.1, -0.4, -5.0],
        [0.8, -0.4, -5.0],
        [0.8, 0.4, -5.0],
        [0.1, 0.4, -5.0],
    ];
    let tris = [[0, 1, 2], [0, 2, 3], [4, 5, 6], [4, 6, 7]];
    let face_ids = [10u64, 10, 20, 20];
    let mesh = Mesh::from_points(frame, &verts, &tris).unwrap();
    let bvh = PrimitiveBvh::build(&mesh, &face_ids, 1).unwrap();

    // Ortho camera: world [-1, 1] x [-1, 1] maps to [0, 100] x [0, 100].
    // Face 10 occupies x in [10, 45], y in [30, 70].
    // Face 20 occupies x in [55, 90], y in [30, 70].
    let cam = Camera {
        view: Rigid::IDENTITY,
        proj: Projection::Ortho { height: 2.0, aspect: 1.0, near: 1.0, far: 10.0 },
        viewport: (100, 100),
    };

    let lasso_left = Lasso::new(vec![(5.0, 20.0), (48.0, 20.0), (48.0, 80.0), (5.0, 80.0)]).unwrap();
    let lasso_right = Lasso::new(vec![(52.0, 20.0), (95.0, 20.0), (95.0, 80.0), (52.0, 80.0)]).unwrap();
    let lasso_both = Lasso::new(vec![(5.0, 20.0), (95.0, 20.0), (95.0, 80.0), (5.0, 80.0)]).unwrap();
    let lasso_none = Lasso::new(vec![(0.0, 0.0), (5.0, 0.0), (5.0, 5.0), (0.0, 5.0)]).unwrap();

    for mode in [ResolveMode::Through, ResolveMode::visible_default()] {
        for rule in TriangleRule::ALL {
            let f_left = resolve_lasso_faces(&mesh, &face_ids, &cam, &lasso_left, rule, mode).unwrap();
            assert_eq!(f_left.domain(), Domain::Faces);
            assert_eq!(f_left.ranges(), &[(10, 11)]);

            let f_right = resolve_lasso_faces(&mesh, &face_ids, &cam, &lasso_right, rule, mode).unwrap();
            assert_eq!(f_right.ranges(), &[(20, 21)]);

            let f_both = resolve_lasso_faces(&mesh, &face_ids, &cam, &lasso_both, rule, mode).unwrap();
            assert_eq!(f_both.ranges(), &[(10, 11), (20, 21)]);

            let f_none = resolve_lasso_faces(&mesh, &face_ids, &cam, &lasso_none, rule, mode).unwrap();
            assert!(f_none.is_empty());
        }
    }

    // Face picks
    let p10 = resolve_pick_faces(&mesh, &face_ids, &bvh, &cam, (25.0, 50.0)).unwrap();
    assert_eq!(p10.domain(), Domain::Faces);
    assert_eq!(p10.ranges(), &[(10, 11)]);

    let p20 = resolve_pick_faces(&mesh, &face_ids, &bvh, &cam, (75.0, 50.0)).unwrap();
    assert_eq!(p20.ranges(), &[(20, 21)]);

    let p_miss = resolve_pick_faces(&mesh, &face_ids, &bvh, &cam, (50.0, 50.0)).unwrap();
    assert!(p_miss.is_empty());

    // Wrong face_ids length fails closed
    assert!(matches!(
        resolve_lasso_faces(&mesh, &[10, 20], &cam, &lasso_both, TriangleRule::AnyVertex, ResolveMode::Through),
        Err(SelError::FaceIdsMismatch { expected: 4, found: 2 })
    ));

    // Occlusion test on two overlapping faces: face 20 in front (z = -3) occludes face 10 (z = -7).
    let occ_verts = [
        [-0.5, -0.5, -7.0],
        [0.5, -0.5, -7.0],
        [0.5, 0.5, -7.0],
        [-0.5, 0.5, -7.0],
        [-0.6, -0.6, -3.0],
        [0.6, -0.6, -3.0],
        [0.6, 0.6, -3.0],
        [-0.6, 0.6, -3.0],
    ];
    let occ_mesh = Mesh::from_points(frame, &occ_verts, &tris).unwrap();
    let f_through =
        resolve_lasso_faces(&occ_mesh, &face_ids, &cam, &lasso_both, TriangleRule::Centroid, ResolveMode::Through)
            .unwrap();
    assert_eq!(f_through.ranges(), &[(10, 11), (20, 21)]);
    let f_vis = resolve_lasso_faces(
        &occ_mesh,
        &face_ids,
        &cam,
        &lasso_both,
        TriangleRule::Centroid,
        ResolveMode::visible_default(),
    )
    .unwrap();
    assert_eq!(f_vis.ranges(), &[(20, 21)]);
}
