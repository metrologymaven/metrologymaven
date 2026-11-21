//! Acceptance criteria of spec node GEO.DIST.BATCH (AC-1 to AC-5) and the error paths.
//!
//! Expected values come from `fixtures/geo/dist/scan_1e6_on_mesh/expected.txt` and
//! `fixtures/geo/dist/directional_cases.json`. Both are written by the independent stdlib-Python
//! reference `tools/fixtures/geo_dist_batch.py` (closest points from the GEO.DIST.MESH model;
//! line intersections in exact rational arithmetic). None come from this crate. The sphere mesh
//! and the point stream are regenerated here from the pinned generators and checked against
//! the fixture's digests before use. The determinism, cancellation, error and watertightness
//! cases are hand-labelled from the leaf's Design.
//!
//! AC-1 runs on the first `FID_BATCH_POINTS` points of the stream (default 50 000; CI's
//! release step sets 1 000 000).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use common::{parse_json, Json};
use fid_geo_dist::batch::{BLOCK_QUANTUM, MAX_BLOCK, MAX_THREADS, RECORD_BYTES};
use fid_geo_dist::{
    encode, project_batch, summary, BatchConfig, BatchError, HitSign, MeshTarget, ProjectError, Projection,
    ProjectionOptions, ProjectionTarget, SignTarget, TriangleSubset,
};
use fid_geo_index::PrimitiveBvh;
use fid_geo_types::{FrameRef, Mesh, PointSet};
use fid_kci::kci::{to_hex, Sha256};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/geo/dist/");

fn read(rel: &str) -> String {
    std::fs::read_to_string(format!("{ROOT}{rel}")).unwrap()
}

fn bits(s: &str) -> f64 {
    f64::from_bits(u64::from_str_radix(s, 16).unwrap())
}

fn frame() -> FrameRef {
    FrameRef::from_bytes([0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15])
}

fn target_of(verts: &[[f64; 3]], tris: &[[u64; 3]]) -> (Mesh, PrimitiveBvh) {
    let mesh = Mesh::from_points(frame(), verts, tris).unwrap();
    let payload: Vec<u64> = (0..tris.len() as u64).collect();
    let bvh = PrimitiveBvh::build(&mesh, &payload, 1).unwrap();
    (mesh, bvh)
}

fn never() -> AtomicBool {
    AtomicBool::new(false)
}

fn cfg(threads: usize, block: usize) -> BatchConfig {
    BatchConfig { threads, block }
}

// ------------------------------------------------------------------ sphere and stream

const N_SUB: u64 = 71;

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
}

fn sqrt(x: f64) -> f64 {
    fid_num::f::sqrt(x).unwrap()
}

/// The GEO.INDEX sphere, as `tools/fixtures/geo_dist.py` `sphere_mesh()` builds it.
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

/// `scan_points()` of the reference: SplitMix64(51), direction from three `uni(-1, 1)`,
/// radius `uni(0.97, 1.03)`.
fn scan_points(n: usize) -> Vec<[f64; 3]> {
    let mut g = SplitMix(51);
    (0..n)
        .map(|_| {
            let d = [g.uni(-1.0, 1.0), g.uni(-1.0, 1.0), g.uni(-1.0, 1.0)];
            let r = g.uni(0.97, 1.03);
            let len = sqrt((d[0] * d[0] + d[1] * d[1]) + d[2] * d[2]);
            let s = r / len;
            [d[0] * s, d[1] * s, d[2] * s]
        })
        .collect()
}

struct Scan {
    header: std::collections::BTreeMap<String, String>,
    samples: Vec<(usize, Vec<String>)>,
}

fn scan_fixture() -> Scan {
    let text = read("scan_1e6_on_mesh/expected.txt");
    let mut header = std::collections::BTreeMap::new();
    let mut samples = Vec::new();
    for ln in text.lines().filter(|l| !l.starts_with('#')) {
        let (k, v) = ln.split_once(' ').unwrap();
        if k == "r" {
            let parts: Vec<String> = v.split(' ').map(str::to_string).collect();
            samples.push((parts[0].parse().unwrap(), parts[1..].to_vec()));
        } else {
            header.insert(k.to_string(), v.to_string());
        }
    }
    Scan { header, samples }
}

fn points_under_test() -> usize {
    std::env::var("FID_BATCH_POINTS").ok().and_then(|s| s.parse().ok()).unwrap_or(50_000).min(1_000_000)
}

// ------------------------------------------------------------------ AC-1, AC-4

#[test]
fn ac1_ac4_results_and_summaries_do_not_depend_on_threads_or_blocks() {
    let scan = scan_fixture();
    let (verts, tris) = sphere_mesh();
    assert_eq!(mesh_digest(&verts, &tris), scan.header["mesh_digest"]);
    let all = scan_points(1_000_000);
    let mut h = Sha256::new();
    for p in &all {
        for x in p {
            h.update(&x.to_bits().to_le_bytes());
        }
    }
    assert_eq!(to_hex(&h.finish()), scan.header["point_digest"], "the stream equals the reference's");

    let n = points_under_test();
    let points = PointSet::from_points(frame(), &all[..n]).unwrap();
    let (mesh, bvh) = target_of(&verts, &tris);
    let target = SignTarget::new(MeshTarget::new(&mesh, &bvh).unwrap()).unwrap();
    let opts = ProjectionOptions::default();

    let reference = project_batch(&points, &target, &opts, &cfg(1, 4096), &never()).unwrap();
    assert_eq!(reference.len(), n);
    let ref_bytes = encode(&reference).unwrap();
    assert_eq!(ref_bytes.len(), n * RECORD_BYTES);
    let ref_summary = summary(&reference).unwrap();

    // Closest points of the sampled indexes equal the GEO.DIST.MESH model's.
    let mut checked = 0;
    for (i, want) in scan.samples.iter().filter(|(i, _)| *i < n) {
        let Projection::Hit(hit) = reference[*i] else { panic!("point {i}: no hit") };
        assert_eq!(hit.entity_id.to_string(), want[0], "point {i}: triangle");
        for k in 0..3 {
            assert_eq!(hit.foot[k].to_bits(), bits(&want[1 + k]).to_bits(), "point {i}: foot {k}");
        }
        assert_eq!(hit.distance.to_bits(), bits(&want[4]).to_bits(), "point {i}: distance");
        assert!(matches!(hit.sign, HitSign::Mesh(_)));
        checked += 1;
    }
    assert!(checked >= n / 10_000);

    for (threads, block) in [(1, 64), (3, 64), (3, 4096), (16, 64), (16, 4096), (7, 192)] {
        let r = project_batch(&points, &target, &opts, &cfg(threads, block), &never()).unwrap();
        assert!(encode(&r).unwrap() == ref_bytes, "threads {threads}, block {block}: results differ");
        let s = summary(&r).unwrap();
        assert_eq!(format!("{s:?}"), format!("{ref_summary:?}"));
        for (a, b) in [(s.min_signed, ref_summary.min_signed), (s.max_signed, ref_summary.max_signed)] {
            assert_eq!(a.map(f64::to_bits), b.map(f64::to_bits));
        }
        assert_eq!(s.mean_signed.map(f64::to_bits), ref_summary.mean_signed.map(f64::to_bits));
    }
    assert_eq!(ref_summary.points, n as u64);
    assert_eq!(ref_summary.hits + ref_summary.no_hits, n as u64);
}

#[test]
fn summary_reduces_in_input_order_with_total_order_extremes() {
    let hit = |d: f64, sign: HitSign| {
        Projection::Hit(fid_geo_dist::ProjectionHit {
            foot: [0.0; 3],
            distance: d.abs(),
            signed_distance: d,
            sign,
            entity_id: 0,
            params: None,
        })
    };
    use fid_geo_dist::SignStatus;
    let rs = [
        hit(0.0, HitSign::Mesh(SignStatus::OnSurface)),
        hit(-0.0, HitSign::Along),
        Projection::NoHit,
        hit(2.0, HitSign::Mesh(SignStatus::Undecidable)),
        hit(-3.0, HitSign::Mesh(SignStatus::Inside)),
        hit(1.0, HitSign::Mesh(SignStatus::Outside)),
    ];
    let s = summary(&rs).unwrap();
    assert_eq!((s.points, s.hits, s.no_hits, s.sign_undefined), (6, 5, 1, 1));
    assert_eq!(s.min_signed, Some(-3.0));
    assert_eq!(s.max_signed.map(f64::to_bits), Some(1.0f64.to_bits()));
    // (0 + -0 + -3 + 1) / 4
    assert_eq!(s.mean_signed, Some(-0.5));
    let e = summary(&[]).unwrap();
    assert_eq!((e.points, e.min_signed, e.mean_signed), (0, None, None));
    let z = summary(&[hit(0.0, HitSign::Along), hit(-0.0, HitSign::Along)]).unwrap();
    assert_eq!(z.min_signed.map(f64::to_bits), Some((-0.0f64).to_bits()));
    assert_eq!(z.max_signed.map(f64::to_bits), Some(0.0f64.to_bits()));
}

// ------------------------------------------------------------------ small meshes

fn cube() -> (Vec<[f64; 3]>, Vec<[u64; 3]>) {
    let json = parse_json(&read("directional_cases.json"));
    mesh_of(json.get("meshes").get("cube"))
}

fn mesh_of(j: &Json) -> (Vec<[f64; 3]>, Vec<[u64; 3]>) {
    let verts = j.get("vertices").arr().iter().map(|v| v3(v)).collect();
    let tris = j
        .get("triangles")
        .arr()
        .iter()
        .map(|t| {
            let a = t.arr();
            [a[0].num() as u64, a[1].num() as u64, a[2].num() as u64]
        })
        .collect();
    (verts, tris)
}

fn v3(j: &Json) -> [f64; 3] {
    let a = j.arr();
    [bits(a[0].str()), bits(a[1].str()), bits(a[2].str())]
}

/// Counts the projections a wrapped target performs, and can fail or cancel on demand.
struct Probe<'t, 'm> {
    inner: &'t SignTarget<'m>,
    calls: AtomicU64,
    fail_at: Vec<[f64; 3]>,
    cancel_after: Option<(&'t AtomicBool, u64)>,
}

impl<'t, 'm> Probe<'t, 'm> {
    fn new(inner: &'t SignTarget<'m>) -> Self {
        Probe { inner, calls: AtomicU64::new(0), fail_at: Vec::new(), cancel_after: None }
    }
}

impl ProjectionTarget for Probe<'_, '_> {
    fn frame(&self) -> FrameRef {
        self.inner.frame()
    }
    fn project(&self, p: [f64; 3], opts: &ProjectionOptions<'_, '_>) -> Result<Projection, ProjectError> {
        let k = self.calls.fetch_add(1, Ordering::SeqCst) + 1;
        if let Some((flag, after)) = self.cancel_after {
            if k >= after {
                flag.store(true, Ordering::SeqCst);
            }
        }
        if self.fail_at.contains(&p) {
            return Err(ProjectError::InvalidDirection);
        }
        self.inner.project(p, opts)
    }
    fn project_along(
        &self,
        p: [f64; 3],
        v: [f64; 3],
        opts: &ProjectionOptions<'_, '_>,
    ) -> Result<Projection, ProjectError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.inner.project_along(p, v, opts)
    }
}

fn grid_points(n: usize) -> Vec<[f64; 3]> {
    (0..n).map(|i| [(i % 17) as f64 * 0.25 - 2.0, (i % 13) as f64 * 0.3 - 1.9, (i % 7) as f64 * 0.5 - 1.5]).collect()
}

// ------------------------------------------------------------------ AC-2

#[test]
fn ac2_frame_mismatch_before_any_point() {
    let (verts, tris) = cube();
    let (mesh, bvh) = target_of(&verts, &tris);
    let target = SignTarget::new(MeshTarget::new(&mesh, &bvh).unwrap()).unwrap();
    let probe = Probe::new(&target);
    let other = FrameRef::from_bytes([7; 16]);
    let points = PointSet::from_points(other, &grid_points(1000)).unwrap();
    // Even an invalid configuration reports the frame first.
    for c in [cfg(4, 4096), cfg(0, 0)] {
        let e = project_batch(&points, &probe, &ProjectionOptions::default(), &c, &never()).unwrap_err();
        match &e {
            BatchError::FrameMismatch(fid_geo_frame::GeoError::FrameMismatch { left, right }) => {
                assert_eq!((*left, *right), (other, frame()));
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(e.code(), "frame_mismatch");
    }
    assert_eq!(probe.calls.load(Ordering::SeqCst), 0, "no point was processed");
}

// ------------------------------------------------------------------ AC-3

#[test]
fn ac3_directional_cases_match_the_exact_reference() {
    let json = parse_json(&read("directional_cases.json"));
    let mut built = std::collections::BTreeMap::new();
    for (name, m) in match json.get("meshes") {
        Json::Obj(v) => v.clone(),
        _ => panic!(),
    } {
        let (verts, tris) = mesh_of(&m);
        built.insert(name, target_of(&verts, &tris));
    }
    let cases = json.get("cases").arr();
    assert!(cases.len() >= 100);
    let (mut hits, mut misses) = (0, 0);
    for (ci, c) in cases.iter().enumerate() {
        let (mesh, bvh) = &built[c.get("mesh").str()];
        let target = SignTarget::new(MeshTarget::new(mesh, bvh).unwrap()).unwrap();
        let p = v3(c.get("p"));
        let v = v3(c.get("v"));
        let max_distance = if c.get("max_distance").is_null() { None } else { Some(bits(c.get("max_distance").str())) };
        let ids: Vec<u64> = if c.get("subset").is_null() {
            Vec::new()
        } else {
            c.get("subset").arr().iter().map(|x| x.num() as u64).collect()
        };
        let subset = TriangleSubset::new(&ids, mesh.triangle_count()).unwrap();
        let opts = ProjectionOptions {
            max_distance,
            subset: if c.get("subset").is_null() { None } else { Some(&subset) },
            direction: Some(v),
        };
        let points = PointSet::from_points(frame(), &[p]).unwrap();
        let got = project_batch(&points, &target, &opts, &cfg(1, 64), &never()).unwrap()[0];
        let brute = target.project_along_brute(p, v, &opts).unwrap();
        assert!(encode(&[got]).unwrap() == encode(&[brute]).unwrap(), "case {ci}: hierarchy and brute force differ");
        let want_hit = matches!(c.get("hit"), Json::Bool(true));
        match got {
            Projection::NoHit => {
                assert!(!want_hit, "case {ci}: expected a hit");
                misses += 1;
            }
            Projection::Hit(h) => {
                assert!(want_hit, "case {ci}: expected no hit, got {h:?}");
                hits += 1;
                let foot = v3(c.get("foot"));
                let e: Vec<f64> = (0..3).map(|k| h.foot[k] - foot[k]).collect();
                let err = sqrt((e[0] * e[0] + e[1] * e[1]) + e[2] * e[2]);
                assert!(err <= 1e-12, "case {ci}: foot off by {err}");
                assert!((h.distance - bits(c.get("distance").str())).abs() <= 1e-12, "case {ci}: distance");
                assert!((h.signed_distance - bits(c.get("signed").str())).abs() <= 1e-12, "case {ci}: signed");
                assert_eq!(h.sign, HitSign::Along);
                if !c.get("tri").is_null() {
                    assert_eq!(h.entity_id, c.get("tri").num() as u64, "case {ci}: triangle");
                }
            }
        }
    }
    assert!(hits >= 50 && misses >= 20, "{hits} hits, {misses} misses");
}

#[test]
fn directional_lines_from_inside_a_closed_mesh_never_slip_through() {
    // Watertightness: every line through an interior point of a closed mesh meets it, also
    // when it passes exactly through shared edges and vertices.
    let (verts, tris) = cube();
    let (mesh, bvh) = target_of(&verts, &tris);
    let target = SignTarget::new(MeshTarget::new(&mesh, &bvh).unwrap()).unwrap();
    let mut g = SplitMix(7);
    let opts = ProjectionOptions::default();
    let mut pts = Vec::new();
    let mut dirs = Vec::new();
    for i in 0..4000 {
        // Dyadic points and directions aimed exactly at cube edges and vertices.
        let p = [g.uni(-0.875, 0.875), g.uni(-0.875, 0.875), g.uni(-0.875, 0.875)];
        let corner = [if i % 2 == 0 { 1.0 } else { -1.0 }, if i % 3 == 0 { 1.0 } else { -1.0 }, g.uni(-1.0, 1.0)];
        let aim = match i % 4 {
            0 => corner,
            1 => [corner[0], corner[1], if i % 8 == 1 { 1.0 } else { -1.0 }],
            _ => [g.uni(-1.0, 1.0), g.uni(-1.0, 1.0), g.uni(-1.0, 1.0)],
        };
        pts.push(p);
        dirs.push([aim[0] - p[0], aim[1] - p[1], aim[2] - p[2]]);
    }
    for (p, v) in pts.iter().zip(&dirs) {
        if v.iter().all(|&x| x == 0.0) {
            continue;
        }
        match target.project_along(*p, *v, &opts).unwrap() {
            Projection::Hit(h) => assert!(h.distance <= 3.5, "{p:?} {v:?}"),
            Projection::NoHit => panic!("line through {p:?} along {v:?} slipped through"),
        }
    }
    // The same on the sphere, through the hierarchy.
    let (sv, st) = sphere_mesh();
    let (smesh, sbvh) = target_of(&sv, &st);
    let sphere = SignTarget::new(MeshTarget::new(&smesh, &sbvh).unwrap()).unwrap();
    for _ in 0..1000 {
        let p = [g.uni(-0.5, 0.5), g.uni(-0.5, 0.5), g.uni(-0.5, 0.5)];
        let v = [g.uni(-1.0, 1.0), g.uni(-1.0, 1.0), g.uni(-1.0, 1.0)];
        match sphere.project_along(p, v, &opts).unwrap() {
            Projection::Hit(h) => assert!(h.distance > 0.1 && h.distance < 1.9, "{p:?} {v:?}: {}", h.distance),
            Projection::NoHit => panic!("line through {p:?} along {v:?} slipped through the sphere"),
        }
    }
}

// ------------------------------------------------------------------ AC-5

#[test]
fn ac5_cancellation_after_the_first_block_returns_no_results() {
    let (verts, tris) = cube();
    let (mesh, bvh) = target_of(&verts, &tris);
    let target = SignTarget::new(MeshTarget::new(&mesh, &bvh).unwrap()).unwrap();
    let points = PointSet::from_points(frame(), &grid_points(64 * 40)).unwrap();
    for threads in [1, 4] {
        let flag = AtomicBool::new(false);
        let mut probe = Probe::new(&target);
        probe.cancel_after = Some((&flag, 64));
        let e = project_batch(&points, &probe, &ProjectionOptions::default(), &cfg(threads, 64), &flag).unwrap_err();
        assert_eq!(e, BatchError::Cancelled);
        assert!(probe.calls.load(Ordering::SeqCst) < 64 * 40, "work stopped between blocks");
    }
    // A flag set before the call: nothing runs.
    let flag = AtomicBool::new(true);
    let probe = Probe::new(&target);
    let e = project_batch(&points, &probe, &ProjectionOptions::default(), &cfg(2, 64), &flag).unwrap_err();
    assert_eq!(e.code(), "cancelled");
    assert_eq!(probe.calls.load(Ordering::SeqCst), 0);
}

// ------------------------------------------------------------------ errors and options

#[test]
fn the_lowest_failing_index_is_reported_whatever_the_schedule() {
    let (verts, tris) = cube();
    let (mesh, bvh) = target_of(&verts, &tris);
    let target = SignTarget::new(MeshTarget::new(&mesh, &bvh).unwrap()).unwrap();
    let mut pts = grid_points(5000);
    for (i, p) in pts.iter_mut().enumerate() {
        p[0] += i as f64 * 1e-6; // all distinct
    }
    let mut probe = Probe::new(&target);
    probe.fail_at = vec![pts[4321], pts[777], pts[3000]];
    let points = PointSet::from_points(frame(), &pts).unwrap();
    for (t, b) in [(1, 64), (5, 64), (16, 128), (3, 4096)] {
        let e = project_batch(&points, &probe, &ProjectionOptions::default(), &cfg(t, b), &never()).unwrap_err();
        assert_eq!(e, BatchError::Point { index: 777, error: ProjectError::InvalidDirection }, "{t} {b}");
    }
}

#[test]
fn configuration_options_and_points_are_checked_before_work() {
    let (verts, tris) = cube();
    let (mesh, bvh) = target_of(&verts, &tris);
    let target = SignTarget::new(MeshTarget::new(&mesh, &bvh).unwrap()).unwrap();
    let probe = Probe::new(&target);
    let points = PointSet::from_points(frame(), &grid_points(100)).unwrap();
    let d = ProjectionOptions::default();
    let run = |o: &ProjectionOptions<'_, '_>, c: BatchConfig| project_batch(&points, &probe, o, &c, &never());
    assert_eq!(run(&d, cfg(0, 64)).unwrap_err(), BatchError::InvalidThreads { threads: 0 });
    assert_eq!(run(&d, cfg(MAX_THREADS + 1, 64)).unwrap_err(), BatchError::InvalidThreads { threads: MAX_THREADS + 1 });
    for b in [0, 1, 63, 100, MAX_BLOCK + BLOCK_QUANTUM] {
        assert_eq!(run(&d, cfg(1, b)).unwrap_err(), BatchError::InvalidBlockSize { block: b });
    }
    for m in [f64::NAN, -1.0, f64::INFINITY] {
        let o = ProjectionOptions { max_distance: Some(m), ..d };
        assert_eq!(run(&o, cfg(1, 64)).unwrap_err(), BatchError::InvalidMaxDistance);
    }
    for v in [[0.0; 3], [f64::NAN, 1.0, 0.0], [0.0, f64::INFINITY, 0.0], [2f64.powi(251), 0.0, 0.0]] {
        let o = ProjectionOptions { direction: Some(v), ..d };
        assert_eq!(run(&o, cfg(1, 64)).unwrap_err(), BatchError::InvalidDirection);
    }
    assert_eq!(probe.calls.load(Ordering::SeqCst), 0);
    let far = PointSet::from_points(frame(), &[[0.0; 3], [0.0, 2f64.powi(251), 0.0]]).unwrap();
    let e = project_batch(&far, &probe, &d, &cfg(1, 64), &never()).unwrap_err();
    assert_eq!(e, BatchError::QueryOutOfRange { index: 1 });
    assert_eq!(probe.calls.load(Ordering::SeqCst), 0);
    // An empty point set gives an empty result.
    let empty = PointSet::from_points(frame(), &[]).unwrap();
    assert!(project_batch(&empty, &target, &d, &cfg(4, 64), &never()).unwrap().is_empty());
}

#[test]
fn max_distance_and_subset_apply_to_closest_point_projection() {
    let (verts, tris) = cube();
    let (mesh, bvh) = target_of(&verts, &tris);
    let target = SignTarget::new(MeshTarget::new(&mesh, &bvh).unwrap()).unwrap();
    let pts = [[0.5, 0.25, 3.0], [0.0, 0.0, 1.5], [0.25, 0.5, -1.25]];
    let points = PointSet::from_points(frame(), &pts).unwrap();
    // Hand-labelled: distances to the cube are 2, 0.5 and 0.25.
    let o = ProjectionOptions { max_distance: Some(1.0), ..ProjectionOptions::default() };
    let r = project_batch(&points, &target, &o, &cfg(2, 64), &never()).unwrap();
    assert_eq!(r[0], Projection::NoHit);
    let Projection::Hit(h) = r[1] else { panic!() };
    assert_eq!((h.distance, h.signed_distance), (0.5, 0.5));
    assert_eq!(h.sign, HitSign::Mesh(fid_geo_dist::SignStatus::Outside));
    // Only the top face (triangles 0 and 1): the point below the cube is 2.25 from it.
    let ids = [0u64, 1];
    let subset = TriangleSubset::new(&ids, 12).unwrap();
    let o = ProjectionOptions { subset: Some(&subset), ..ProjectionOptions::default() };
    let r = project_batch(&points, &target, &o, &cfg(1, 64), &never()).unwrap();
    let Projection::Hit(h) = r[2] else { panic!() };
    assert_eq!(h.distance, 2.25);
    assert!(h.entity_id <= 1);
    // A subset built for another triangle count is refused per point.
    let wrong = TriangleSubset::new(&ids, 13).unwrap();
    let o = ProjectionOptions { subset: Some(&wrong), ..ProjectionOptions::default() };
    let e = project_batch(&points, &target, &o, &cfg(1, 64), &never()).unwrap_err();
    assert_eq!(e.code(), "subset_mismatch");
}

#[test]
fn records_have_the_pinned_layout() {
    let mut out = Vec::new();
    Projection::NoHit.write_bytes(&mut out);
    assert_eq!(out, vec![0u8; RECORD_BYTES]);
    out.clear();
    let h = fid_geo_dist::ProjectionHit {
        foot: [1.0, 2.0, 3.0],
        distance: 0.5,
        signed_distance: -0.5,
        sign: HitSign::Along,
        entity_id: 0x0102,
        params: Some((0.25, 0.75)),
    };
    Projection::Hit(h).write_bytes(&mut out);
    assert_eq!(out.len(), RECORD_BYTES);
    assert_eq!(out[0], 1);
    assert_eq!(&out[1..9], &0x0102u64.to_le_bytes());
    assert_eq!(&out[9..17], &1.0f64.to_bits().to_le_bytes());
    assert_eq!(&out[41..49], &(-0.5f64).to_bits().to_le_bytes());
    assert_eq!(out[49], fid_geo_dist::batch::ALONG_SIGN_CODE);
    assert_eq!(out[50], 1);
    assert_eq!(&out[59..67], &0.75f64.to_bits().to_le_bytes());
}
