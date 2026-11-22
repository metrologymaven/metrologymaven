//! AC-1 (REQ-GEO.INDEX.BUDGET-001, -002): builds under a budget of `estimate - 1` fail with
//! `BudgetExceeded { needed, budget }` and the counting global allocator records zero index
//! allocation; builds at `estimate` succeed with actual allocated and retained bytes `<= estimate`.
//!
//! Own test binary with a single test function so no other test thread allocates concurrently
//! against the process-wide `Cap<System>` allocator.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::alloc::System;

use cap::Cap;
use fid_geo_index::{
    build_bvh, build_point_index, estimate_bytes, BudgetError, BuildParams, IndexCache, IndexKind, MemoryBudget,
};
use fid_geo_types::digest::{digest_mesh, digest_point_set};
use fid_geo_types::{FrameRef, Mesh, PointBuffer, PointSet};
use fid_kci::kci::{to_hex, Sha256};

#[global_allocator]
static ALLOC: Cap<System> = Cap::new(System, usize::MAX);

const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/geo/index/");

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

fn uniform_1e5() -> PointBuffer {
    let mut g = SplitMix(21);
    let n = 100_000usize;
    let pts: Vec<[f64; 3]> = (0..n).map(|_| [g.unit() * 0.125, g.unit() * 0.125, g.unit() * 0.125]).collect();

    // Verify against fixtures/geo/index/uniform_1e5/expected.txt.
    let expected = std::fs::read_to_string(format!("{ROOT}uniform_1e5/expected.txt")).unwrap();
    let expected_hex = expected
        .lines()
        .find_map(|l| l.strip_prefix("points 100000 "))
        .expect("points line in uniform_1e5/expected.txt");
    let mut h = Sha256::new();
    h.update(&(pts.len() as u64).to_le_bytes());
    for p in &pts {
        for x in p {
            h.update(&x.to_bits().to_le_bytes());
        }
    }
    assert_eq!(to_hex(&h.finish()), expected_hex, "uniform_1e5 digest mismatch");
    PointBuffer::from_points(&pts)
}

fn mesh_sphere_1e5() -> (Mesh, Vec<u64>) {
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
    let n = 71u64;
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
    let expected = std::fs::read_to_string(format!("{ROOT}mesh_sphere_1e5/expected.txt")).unwrap();
    let expected_tris: u64 = expected.lines().find_map(|l| l.strip_prefix("triangles ")).unwrap().parse().unwrap();
    assert_eq!(tris.len() as u64, expected_tris);
    let payload: Vec<u64> = (0..tris.len() as u64).map(|t| t.wrapping_mul(2_654_435_761) & 0xffff_ffff).collect();
    let frame = FrameRef::from_bytes(std::array::from_fn(|i| i as u8));
    let mesh = Mesh::from_points(frame, &verts, &tris).unwrap();
    (mesh, payload)
}

#[test]
fn ac1_zero_allocation_on_refusal_and_upper_bound_on_build() {
    // --- 1. uniform_1e5 (PointIndex / KdTree) ---
    let pts = uniform_1e5();
    let kdtree_params = BuildParams::kdtree();
    let kdtree_est = estimate_bytes(IndexKind::KdTree, pts.len(), kdtree_params);
    assert!(kdtree_est > 400_000, "unexpectedly small kdtree estimate: {kdtree_est}");

    for budget in [0, 1, kdtree_est / 2, kdtree_est - 1] {
        let before = ALLOC.total_allocated();
        let res = build_point_index(&pts, kdtree_params, MemoryBudget(budget));
        let after = ALLOC.total_allocated();
        assert_eq!(after, before, "KdTree refusal at budget={budget} must allocate 0 bytes");
        assert_eq!(res, Err(BudgetError::BudgetExceeded { needed: kdtree_est, budget }));
    }

    // IndexCache refusal before allocation on uniform_1e5.
    let ps = PointSet::new(FrameRef::from_bytes([1; 16]), pts.clone(), fid_geo_types::AttributeSet::default()).unwrap();
    let ps_digest = digest_point_set(&ps);
    let mut cache = IndexCache::new(MemoryBudget(kdtree_est - 1));
    let before = ALLOC.total_allocated();
    let cache_res = cache.get_or_build_point(ps_digest, &pts, kdtree_params);
    let after = ALLOC.total_allocated();
    assert_eq!(after, before, "IndexCache KdTree refusal must allocate 0 bytes");
    assert_eq!(cache_res.unwrap_err(), BudgetError::BudgetExceeded { needed: kdtree_est, budget: kdtree_est - 1 });
    assert_eq!(cache.build_count(), 0);

    // Exact budget succeeds and both actual reported bytes and total heap bytes allocated <= estimate.
    let before = ALLOC.total_allocated();
    let (idx, report) = build_point_index(&pts, kdtree_params, MemoryBudget(kdtree_est)).unwrap();
    let allocated = (ALLOC.total_allocated() - before) as u64;
    assert_eq!(idx.len(), 100_000);
    assert!(report.bytes <= kdtree_est, "reported {} > estimate {}", report.bytes, kdtree_est);
    assert!(allocated <= kdtree_est, "heap allocated {} > estimate {}", allocated, kdtree_est);
    drop(idx);

    // --- 2. mesh_sphere_1e5 (PrimitiveBvh / Bvh) ---
    let (mesh, payload) = mesh_sphere_1e5();
    let bvh_params = BuildParams::bvh();
    let bvh_est = estimate_bytes(IndexKind::Bvh, mesh.triangle_count(), bvh_params);
    assert!(bvh_est > 10_000_000, "unexpectedly small bvh estimate: {bvh_est}");

    for budget in [0, 1, bvh_est / 2, bvh_est - 1] {
        let before = ALLOC.total_allocated();
        let res = build_bvh(&mesh, &payload, bvh_params, MemoryBudget(budget));
        let after = ALLOC.total_allocated();
        assert_eq!(after, before, "BVH refusal at budget={budget} must allocate 0 bytes");
        assert_eq!(res, Err(BudgetError::BudgetExceeded { needed: bvh_est, budget }));
    }

    // IndexCache refusal before allocation on mesh_sphere_1e5.
    let mesh_digest = digest_mesh(&mesh);
    let mut bvh_cache = IndexCache::new(MemoryBudget(bvh_est - 1));
    let before = ALLOC.total_allocated();
    let bvh_cache_res = bvh_cache.get_or_build_bvh(mesh_digest, &mesh, &payload, bvh_params);
    let after = ALLOC.total_allocated();
    assert_eq!(after, before, "IndexCache BVH refusal must allocate 0 bytes");
    assert_eq!(bvh_cache_res.unwrap_err(), BudgetError::BudgetExceeded { needed: bvh_est, budget: bvh_est - 1 });
    assert_eq!(bvh_cache.build_count(), 0);

    // Exact budget succeeds and both retained bytes and total heap bytes allocated during build <= estimate.
    let before = ALLOC.total_allocated();
    let (bvh, report) = build_bvh(&mesh, &payload, bvh_params, MemoryBudget(bvh_est)).unwrap();
    let allocated = (ALLOC.total_allocated() - before) as u64;
    assert_eq!(bvh.triangle_count(), mesh.triangle_count());
    assert!(report.bytes <= bvh_est, "reported {} > estimate {}", report.bytes, bvh_est);
    assert!(allocated <= bvh_est, "heap allocated {} > estimate {}", allocated, bvh_est);
}
