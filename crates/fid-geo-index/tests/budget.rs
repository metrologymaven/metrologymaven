//! Acceptance criteria of spec node `GEO.INDEX.BUDGET` (AC-1 to AC-4, REQ-001 to REQ-005).
//!
//! AC-1's zero-allocation proof using a process-wide counting `#[global_allocator]` also runs in
//! the dedicated single-test binary `tests/budget_alloc.rs` (avoiding concurrent allocation noise).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::sync::Arc;
use std::time::Instant;

use fid_geo_index::{
    build_bvh, build_bvh_triangles, build_point_index, build_point_tree, bvh_build_bytes, checked_estimate_bytes,
    estimate_bytes, overhead_bytes, BudgetError, BuildParams, BuildReport, BvhError, CachedIndex, IndexCache, IndexKey,
    IndexKind, MemoryBudget, PointIndex, PointIndexError, PointTree, PrimitiveBvh, BUCKET, LEAF_SIZE, MAX_POINTS,
    MAX_TRIANGLES,
};
use fid_geo_types::digest::{digest_mesh, digest_point_set, ElementDigest};
use fid_geo_types::{Aabb, AttributeSet, FrameRef, Mesh, PointSet};

fn make_cloud(seed: u64, n: usize) -> (PointSet, ElementDigest) {
    let mut s = seed.wrapping_add(1);
    let mut next = || -> f64 {
        s = s.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = s;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        ((z ^ (z >> 31)) >> 11) as f64 * (1.0 / 9_007_199_254_740_992.0)
    };
    let pts: Vec<[f64; 3]> = (0..n).map(|_| [next(), next(), next()]).collect();
    let frame = FrameRef::from_bytes([0x11; 16]);
    let ps = PointSet::from_points(frame, &pts).unwrap();
    let digest = digest_point_set(&ps);
    (ps, digest)
}

fn make_mesh(offset: f64, n_tris: usize) -> (Mesh, Vec<u64>, ElementDigest) {
    let mut verts = Vec::with_capacity(n_tris * 3);
    let mut tris = Vec::with_capacity(n_tris);
    for i in 0..n_tris {
        let base = (i * 3) as u64;
        let x = offset + (i as f64) * 0.01;
        verts.push([x, 0.0, 0.0]);
        verts.push([x + 0.005, 0.01, 0.0]);
        verts.push([x, 0.01, 0.005]);
        tris.push([base, base + 1, base + 2]);
    }
    let frame = FrameRef::from_bytes([0x22; 16]);
    let mesh = Mesh::from_points(frame, &verts, &tris).unwrap();
    let payload: Vec<u64> = (0..n_tris as u64).collect();
    let digest = digest_mesh(&mesh);
    (mesh, payload, digest)
}

#[test]
fn ac1_estimates_bound_actual_across_edge_sizes() {
    let kd_params = BuildParams::kdtree();
    for n in [0usize, 1, 2, 3, 7, 8, 9, 15, 16, 17, 31, 32, 33, 100, 1_024] {
        let (ps, _) = make_cloud(n as u64, n);
        let est = estimate_bytes(IndexKind::KdTree, n as u64, kd_params);
        assert_eq!(Some(est), overhead_bytes(n as u64));

        if est > 0 {
            let err = build_point_index(ps.buffer(), kd_params, MemoryBudget(est - 1)).unwrap_err();
            assert_eq!(err, BudgetError::BudgetExceeded { needed: est, budget: est - 1 });
        }
        let (idx, report) = build_point_index(ps.buffer(), kd_params, MemoryBudget(est)).unwrap();
        assert_eq!(report.bytes, idx.memory_bytes());
        assert!(report.bytes <= est, "n={n}: {} > {est}", report.bytes);

        let (tree, tree_report) = build_point_tree(ps.buffer(), kd_params, MemoryBudget(est)).unwrap();
        assert_eq!(tree_report.bytes, tree.memory_bytes());
        assert!(tree_report.bytes <= est, "n={n}: tree {} > {est}", tree_report.bytes);
    }

    let bvh_params = BuildParams::bvh();
    for n in [1usize, 2, 3, 4, 7, 8, 9, 15, 16, 17, 32, 33, 100, 500] {
        let (mesh, payload, _) = make_mesh(0.0, n);
        let est = estimate_bytes(IndexKind::Bvh, n as u64, bvh_params);
        assert_eq!(Some(est), bvh_build_bytes(n as u64));

        let err = build_bvh(&mesh, &payload, bvh_params, MemoryBudget(est - 1)).unwrap_err();
        assert_eq!(err, BudgetError::BudgetExceeded { needed: est, budget: est - 1 });

        let (bvh, report) = build_bvh(&mesh, &payload, bvh_params, MemoryBudget(est)).unwrap();
        assert_eq!(report.bytes, bvh.memory_bytes());
        assert!(report.bytes <= est, "n={n}: bvh {} > {est}", report.bytes);
    }

    // Corner-triple BVH builder respects the same budget.
    let tris = vec![[[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]]];
    let est1 = estimate_bytes(IndexKind::Bvh, 1, bvh_params);
    assert_eq!(
        build_bvh_triangles(FrameRef::from_bytes([0; 16]), &tris, &[42], bvh_params, MemoryBudget(est1 - 1)),
        Err(BudgetError::BudgetExceeded { needed: est1, budget: est1 - 1 })
    );
    let (bvh1, rep1) =
        build_bvh_triangles(FrameRef::from_bytes([0; 16]), &tris, &[42], bvh_params, MemoryBudget(est1)).unwrap();
    assert_eq!(bvh1.payload(0), Some(42));
    assert!(rep1.bytes <= est1);
}

#[test]
fn ac2_cache_hit_reuses_same_object_without_rebuild() {
    let mut cache = IndexCache::new(MemoryBudget::UNLIMITED);
    let kd_params = BuildParams::kdtree();
    let (ps, ps_digest) = make_cloud(7, 256);

    let t0 = Instant::now();
    let mut first = cache.get_or_build_point(ps_digest, ps.buffer(), kd_params).unwrap();
    first.report = first.report.with_elapsed_ns(t0.elapsed().as_nanos().min(u64::MAX as u128) as u64);
    assert!(!first.hit);
    assert_eq!(cache.build_count(), 1);
    assert_eq!(cache.hit_count(), 0);

    let second = cache.get_or_build_point(ps_digest, ps.buffer(), kd_params).unwrap();
    assert!(second.hit);
    assert_eq!(cache.build_count(), 1, "build counter must not change on cache hit");
    assert_eq!(cache.hit_count(), 1);
    assert!(first.index.ptr_eq(&second.index), "cache hit must return the exact same object");
    assert_eq!(second.report.bytes, first.report.bytes);
    assert_eq!(second.report.elapsed_ns, 0, "kernel never stores elapsed_ns");

    // REQ-005: cached index produces bit-identical query results to a freshly built PointIndex.
    let direct = PointIndex::build(ps.buffer(), 1).unwrap();
    let cached_tree = second.index.as_point().unwrap();
    let q = [0.25, 0.5, 0.75];
    assert_eq!(cached_tree.knn(ps.buffer(), q, 8).unwrap(), direct.knn(q, 8).unwrap());
    assert_eq!(cached_tree.radius(ps.buffer(), q, 0.35).unwrap(), direct.radius(q, 0.35).unwrap());
    let bx = Aabb::new([0.1, 0.1, 0.1], [0.6, 0.6, 0.6]).unwrap();
    assert_eq!(cached_tree.in_box(ps.buffer(), &bx).unwrap(), direct.in_box(&bx).unwrap());

    // Same for PrimitiveBvh.
    let bvh_params = BuildParams::bvh();
    let (mesh, payload, mesh_digest) = make_mesh(1.0, 64);
    let b1 = cache.get_or_build_bvh(mesh_digest, &mesh, &payload, bvh_params).unwrap();
    let b2 = cache.get_or_build_bvh(mesh_digest, &mesh, &payload, bvh_params).unwrap();
    assert!(!b1.hit);
    assert!(b2.hit);
    assert_eq!(cache.build_count(), 2);
    assert!(b1.index.ptr_eq(&b2.index));

    let direct_bvh = PrimitiveBvh::build(&mesh, &payload, 1).unwrap();
    let cached_bvh = b2.index.as_primitive().unwrap();
    assert_eq!(cached_bvh.to_bytes().unwrap(), direct_bvh.to_bytes().unwrap());
    assert_eq!(cached_bvh.nearest_box([1.2, 0.0, 0.0]).unwrap(), direct_bvh.nearest_box([1.2, 0.0, 0.0]).unwrap());
}

#[test]
fn ac3_lru_evicts_b_for_sequence_a_b_a_c() {
    let params = BuildParams::kdtree();
    let (ps_a, dig_a) = make_cloud(10, 500);
    let (ps_b, dig_b) = make_cloud(20, 500);
    let (ps_c, dig_c) = make_cloud(30, 500);
    assert_ne!(dig_a, dig_b);
    assert_ne!(dig_b, dig_c);
    assert_ne!(dig_a, dig_c);

    let key_a = IndexKey::new(dig_a, IndexKind::KdTree, params);
    let key_b = IndexKey::new(dig_b, IndexKind::KdTree, params);
    let key_c = IndexKey::new(dig_c, IndexKind::KdTree, params);

    // Budget fits exactly 2 of the 3 indexes (not 3).
    let one = estimate_bytes(IndexKind::KdTree, 500, params);
    let mut cache = IndexCache::new(MemoryBudget(one * 2));

    // Sequence: A, B, A, C.
    let res_a1 = cache.get_or_build_point(dig_a, ps_a.buffer(), params).unwrap();
    assert!(!res_a1.hit);
    let res_b = cache.get_or_build_point(dig_b, ps_b.buffer(), params).unwrap();
    assert!(!res_b.hit);
    assert_eq!(cache.len(), 2);

    // Access A again -> A becomes MRU, B becomes LRU.
    let res_a2 = cache.get_or_build_point(dig_a, ps_a.buffer(), params).unwrap();
    assert!(res_a2.hit);
    assert!(res_a1.index.ptr_eq(&res_a2.index));

    // Insert C -> exceeds 2*one, so B must be evicted and A & C remain.
    let res_c = cache.get_or_build_point(dig_c, ps_c.buffer(), params).unwrap();
    assert!(!res_c.hit);
    assert_eq!(cache.len(), 2);
    assert_eq!(cache.eviction_count(), 1);
    assert!(cache.contains(&key_a), "A was recently used and must stay");
    assert!(!cache.contains(&key_b), "B was least recently used and must be evicted");
    assert!(cache.contains(&key_c), "C was just inserted and must stay");
    assert!(cache.used_bytes() <= cache.budget().bytes());

    // Re-requesting B now misses and rebuilds.
    let res_b2 = cache.get_or_build_point(dig_b, ps_b.buffer(), params).unwrap();
    assert!(!res_b2.hit);
    assert!(!res_b.index.ptr_eq(&res_b2.index));
    assert_eq!(cache.build_count(), 4);
    assert_eq!(cache.eviction_count(), 2);
    assert!(!cache.contains(&key_a), "A was now older than C and gets evicted");
    assert!(cache.contains(&key_c));
    assert!(cache.contains(&key_b));

    // Same A, B, A, C eviction test for BVH.
    let bvh_params = BuildParams::bvh();
    let (m_a, p_a, d_a) = make_mesh(0.0, 120);
    let (m_b, p_b, d_b) = make_mesh(10.0, 120);
    let (m_c, p_c, d_c) = make_mesh(20.0, 120);
    let sample = PrimitiveBvh::build(&m_a, &p_a, 1).unwrap();
    let retained_one = sample.memory_bytes();
    let build_one = estimate_bytes(IndexKind::Bvh, 120, bvh_params);
    // Budget >= build_one so each build is allowed, and >= 2*retained_one but < 3*retained_one so
    // two cached BVHs fit while the third forces eviction.
    let bvh_budget = build_one.max(retained_one * 2);
    assert!(bvh_budget < retained_one * 3, "budget {bvh_budget} must fit 2 but not 3 BVHs (retained={retained_one})");

    let mut bvh_cache = IndexCache::new(MemoryBudget(bvh_budget));
    bvh_cache.get_or_build_bvh(d_a, &m_a, &p_a, bvh_params).unwrap();
    bvh_cache.get_or_build_bvh(d_b, &m_b, &p_b, bvh_params).unwrap();
    let hit_a = bvh_cache.get_or_build_bvh(d_a, &m_a, &p_a, bvh_params).unwrap();
    assert!(hit_a.hit);
    bvh_cache.get_or_build_bvh(d_c, &m_c, &p_c, bvh_params).unwrap();

    assert!(bvh_cache.contains(&IndexKey::new(d_a, IndexKind::Bvh, bvh_params)));
    assert!(!bvh_cache.contains(&IndexKey::new(d_b, IndexKind::Bvh, bvh_params)));
    assert!(bvh_cache.contains(&IndexKey::new(d_c, IndexKind::Bvh, bvh_params)));
}

#[test]
fn ac4_different_digest_or_params_forces_new_build() {
    let mut cache = IndexCache::new(MemoryBudget::UNLIMITED);
    let params = BuildParams::kdtree();

    let (ps1, dig1) = make_cloud(100, 128);
    // Perturb a single coordinate so the element has a different digest.
    let mut pts2 = ps1.buffer().clone();
    pts2.x[0] += 1e-6;
    let ps2 = PointSet::new(ps1.frame(), pts2, AttributeSet::default()).unwrap();
    let dig2 = digest_point_set(&ps2);
    assert_ne!(dig1, dig2);

    let r1 = cache.get_or_build_point(dig1, ps1.buffer(), params).unwrap();
    assert!(!r1.hit);
    assert_eq!(cache.build_count(), 1);

    let r2 = cache.get_or_build_point(dig2, ps2.buffer(), params).unwrap();
    assert!(!r2.hit, "different digest must trigger a new build");
    assert_eq!(cache.build_count(), 2);
    assert!(!r1.index.ptr_eq(&r2.index));

    // Different BuildParams on the same digest also produce a distinct cache key.
    let params2 = params.with_threads(4);
    let r3 = cache.get_or_build_point(dig1, ps1.buffer(), params2).unwrap();
    assert!(!r3.hit, "different build params must produce a distinct key");
    assert_eq!(cache.build_count(), 3);
    assert!(!r1.index.ptr_eq(&r3.index));
}

#[test]
fn error_paths_and_edge_cases() {
    // Overflow & invalid counts in checked_estimate_bytes / estimate_bytes.
    assert_eq!(checked_estimate_bytes(IndexKind::KdTree, MAX_POINTS + 1, BuildParams::kdtree()), None);
    assert_eq!(estimate_bytes(IndexKind::KdTree, MAX_POINTS + 1, BuildParams::kdtree()), u64::MAX);
    assert_eq!(checked_estimate_bytes(IndexKind::Bvh, MAX_TRIANGLES + 1, BuildParams::bvh()), None);
    assert_eq!(estimate_bytes(IndexKind::Bvh, MAX_TRIANGLES + 1, BuildParams::bvh()), u64::MAX);
    assert_eq!(checked_estimate_bytes(IndexKind::KdTree, 10, BuildParams { leaf_size: 0, threads: 1 }), None);
    assert_eq!(estimate_bytes(IndexKind::Bvh, 10, BuildParams { leaf_size: 0, threads: 1 }), u64::MAX);

    // Unsupported leaf size rejected before build.
    let (ps, dig) = make_cloud(1, 32);
    let bad_kd = BuildParams { leaf_size: (BUCKET as u16) + 1, threads: 1 };
    assert_eq!(
        build_point_index(ps.buffer(), bad_kd, MemoryBudget::UNLIMITED).unwrap_err(),
        BudgetError::UnsupportedLeafSize { kind: IndexKind::KdTree, requested: 17, supported: 16 }
    );
    let (mesh, payload, _) = make_mesh(0.0, 4);
    let bad_bvh = BuildParams { leaf_size: (LEAF_SIZE as u16) - 1, threads: 1 };
    assert_eq!(
        build_bvh(&mesh, &payload, bad_bvh, MemoryBudget::UNLIMITED).unwrap_err(),
        BudgetError::UnsupportedLeafSize { kind: IndexKind::Bvh, requested: 7, supported: 8 }
    );

    // Underlying build errors propagate cleanly and leave cache state untouched.
    let mut cache = IndexCache::new(MemoryBudget::UNLIMITED);
    let err = cache.get_or_build_point(dig, ps.buffer(), BuildParams::kdtree().with_threads(0)).unwrap_err();
    assert_eq!(err, BudgetError::PointIndex(PointIndexError::ZeroThreads));
    assert_eq!(err.code(), "zero_threads");
    assert!(cache.is_empty());
    assert_eq!(cache.build_count(), 0);

    let empty_tris: Vec<[[f64; 3]; 3]> = Vec::new();
    let bvh_err = cache
        .get_or_build_bvh_triangles(
            ElementDigest([0x99; 32]),
            FrameRef::from_bytes([0; 16]),
            &empty_tris,
            &[],
            BuildParams::bvh(),
        )
        .unwrap_err();
    assert_eq!(bvh_err, BudgetError::Bvh(BvhError::EmptyMesh));
    assert_eq!(bvh_err.code(), "empty_mesh");
    assert!(cache.is_empty());

    // KindMismatch guard on generic get_or_build.
    let key = IndexKey::new(dig, IndexKind::Bvh, BuildParams::bvh());
    let mismatch = cache
        .get_or_build(key, 32, || {
            let tree = PointTree::build(ps.buffer(), 1)?;
            Ok(CachedIndex::Point(Arc::new(tree)))
        })
        .unwrap_err();
    assert_eq!(mismatch, BudgetError::KindMismatch { expected: IndexKind::Bvh, found: IndexKind::KdTree });
    assert_eq!(mismatch.code(), "kind_mismatch");
    assert!(cache.is_empty());

    // set_budget shrinks cache deterministically by LRU order.
    let one = estimate_bytes(IndexKind::KdTree, 32, BuildParams::kdtree());
    let (ps_a, dig_a) = make_cloud(1, 32);
    let (ps_b, dig_b) = make_cloud(2, 32);
    cache.get_or_build_point(dig_a, ps_a.buffer(), BuildParams::kdtree()).unwrap();
    cache.get_or_build_point(dig_b, ps_b.buffer(), BuildParams::kdtree()).unwrap();
    assert_eq!(cache.len(), 2);
    cache.set_budget(MemoryBudget(one));
    assert_eq!(cache.len(), 1);
    assert!(!cache.contains(&IndexKey::new(dig_a, IndexKind::KdTree, BuildParams::kdtree())));
    assert!(cache.contains(&IndexKey::new(dig_b, IndexKind::KdTree, BuildParams::kdtree())));

    // BuildReport observational helper.
    let r = BuildReport::new(1024).with_elapsed_ns(42_000);
    assert_eq!(r, BuildReport { bytes: 1024, elapsed_ns: 42_000 });
}
