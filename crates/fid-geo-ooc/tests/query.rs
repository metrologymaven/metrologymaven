//! GEO.OOC.QUERY: cross-chunk k-nearest, radius and box queries.
//!
//! * AC-2 on fixture `boundary_ties` (points mirrored across the partition's split planes at
//!   equal distance; expected results from the independent brute-force reference
//!   `fixtures/geo/ooc/make_query_fixtures.py`): the tie order by original index matches the
//!   reference and the in-memory GEO.INDEX result, although the tied points lie in
//!   different chunks.
//! * Identity with the in-memory GEO.INDEX query on awkward clouds (duplicates at the depth
//!   limit, signed zeros, non-dyadic bounds, queries far outside the cloud).
//! * The exact chunk bounds hold every point of their chunk.
//! * Error paths: query argument errors equal GEO.INDEX's; an unverified manifest is refused
//!   until verified; a tampered chunk is a `chunk_integrity` error; the index budget refuses
//!   a tree that does not fit, and a budget that holds one tree still gives identical results.
//! * A cloud beyond the GEO.INDEX coordinate limit is refused when the index is made; a
//!   store whose reads fail makes every query kind return that error.
//! * AC-4 on a small cloud: the per-chunk build counter does not exceed the distinct chunks
//!   touched, and repeated queries build nothing more.
//!
//! AC-1 and AC-3 (fixture cloud_5e6) are in `tests/cloud_5e6.rs`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::needless_range_loop)]

mod common;

use common::*;
use fid_geo_index::{BudgetError, PointIndex, PointIndexError};
use fid_geo_ooc::{
    to_chunks, ChunkCache, ChunkManifest, ChunkStore, ChunkedIndex, LayoutParams, OocError, Provenance, QueryConfig,
    QueryError, DEFAULT_CACHE_BUDGET,
};
use fid_geo_types::{Aabb, FrameRef, PointCloud};

fn frame() -> FrameRef {
    FrameRef::from_bytes([7u8; 16])
}

fn chunked(cloud: &PointCloud, max: u64) -> (SyncStore, ChunkManifest) {
    let store = SyncStore::new();
    let m = to_chunks(cloud, &store, &LayoutParams { max_chunk_points: max }).unwrap();
    (store, m)
}

/// Original index -> manifest position of its chunk.
fn chunk_of(m: &ChunkManifest, store: &SyncStore, cache: &ChunkCache) -> Vec<usize> {
    let mut out = vec![usize::MAX; m.n_points as usize];
    for pos in 0..m.chunks.len() {
        let h = cache.get(store, m, pos).unwrap();
        for &i in &h.index {
            out[i as usize] = pos;
        }
    }
    out
}

#[test]
fn ac2_boundary_ties() {
    let f = json("boundary_ties/cases.json");
    let pts: Vec<[f64; 3]> = arr(get(&f, "points")).iter().map(f3).collect();
    let cloud = PointCloud::from_points(frame(), &pts).unwrap();
    let (store, m) = chunked(&cloud, int(get(&f, "max_chunk_points")));
    assert!(m.chunks.len() >= 8, "the root is split ({} chunks)", m.chunks.len());
    let cache = ChunkCache::new(DEFAULT_CACHE_BUDGET);
    let where_ = chunk_of(&m, &store, &cache);
    let ix = ChunkedIndex::new(&m, &store, &cache, QueryConfig::default(), Provenance::TrustedWriter).unwrap();
    let mem = PointIndex::build(cloud.buffer(), 1).unwrap();

    let (mut ties, mut against_chunk_order) = (0, 0);
    for (n, case) in arr(get(&f, "cases")).iter().enumerate() {
        let kind = s(get(case, "kind"));
        if kind == "box" {
            let b = Aabb::new(f3(get(case, "lo")), f3(get(case, "hi"))).unwrap();
            let want: Vec<u64> = arr(get(case, "expected")).iter().map(int).collect();
            assert_eq!(ix.in_box(&b).unwrap(), want, "case {n}: box against the reference");
            assert_eq!(mem.in_box(&b).unwrap(), want, "case {n}: in-memory box");
            continue;
        }
        let q = f3(get(case, "point"));
        let want: Vec<(u64, u64)> = arr(get(case, "expected"))
            .iter()
            .map(|e| {
                let e = arr(e);
                (int(&e[0]), f64_hex(&e[1]).to_bits())
            })
            .collect();
        let (got, inmem) = if kind == "knn" {
            let k = int(get(case, "k")) as usize;
            (ix.knn(q, k).unwrap(), mem.knn(q, k).unwrap())
        } else {
            let r = f64_hex(get(case, "r"));
            (ix.radius(q, r).unwrap(), mem.radius(q, r).unwrap())
        };
        assert_eq!(bits(&got), want, "case {n} ({kind}): against the reference");
        assert_eq!(bits(&inmem), want, "case {n} ({kind}): in-memory");
        // The first two results tie; they must come from different chunks for the case to
        // test the merge.
        if want.len() >= 2 && want[0].1 == want[1].1 {
            let (a, b) = (where_[want[0].0 as usize], where_[want[1].0 as usize]);
            if a != b {
                ties += 1;
                if m.chunks[a].cell.raw() > m.chunks[b].cell.raw() {
                    against_chunk_order += 1;
                }
            }
        }
    }
    assert!(ties >= 20, "tied results across chunks: {ties}");
    assert!(against_chunk_order >= 5, "ties whose index order is not the chunk order: {against_chunk_order}");
}

/// A cloud with duplicates (a cell at the depth limit above the chunk size), signed zeros
/// and non-dyadic bounds.
fn awkward_cloud() -> PointCloud {
    let mut g = SplitMix(0x5eed_0062);
    let mut pts = Vec::new();
    for _ in 0..3000 {
        pts.push([g.unit() * 0.7 - 0.3, g.unit() * (1.0 / 3.0), g.unit() * 0.01 + 7.0]);
    }
    for _ in 0..200 {
        pts.push([0.1, 0.1, 7.005]);
    }
    for i in 0..50 {
        let z = if i % 2 == 0 { -0.0 } else { 0.0 };
        pts.push([z, 0.2, 7.0 + (i as f64) * 1e-4]);
    }
    // Points on the max faces.
    pts.push([0.4, 1.0 / 3.0, 7.01]);
    pts.push([-0.3, 0.0, 7.0]);
    PointCloud::from_points(frame(), &pts).unwrap()
}

fn query_point(g: &mut SplitMix, cloud: &PointCloud) -> [f64; 3] {
    let n = cloud.len();
    match g.next() % 4 {
        // Far outside the cloud.
        0 => [g.unit() * 20.0 - 10.0, g.unit() * 20.0 - 10.0, g.unit() * 20.0 - 3.0],
        // On a cloud point.
        1 => cloud.point(g.next() % n).unwrap(),
        _ => {
            let p = cloud.point(g.next() % n).unwrap();
            [p[0] + (g.unit() - 0.5) * 0.01, p[1] + (g.unit() - 0.5) * 0.01, p[2] + (g.unit() - 0.5) * 0.001]
        }
    }
}

#[test]
fn identical_to_in_memory_on_awkward_clouds() {
    let cloud = awkward_cloud();
    let mem = PointIndex::build(cloud.buffer(), 1).unwrap();
    for max in [1u64, 32, 500, 1 << 20] {
        let (store, m) = chunked(&cloud, max);
        let cache = ChunkCache::new(DEFAULT_CACHE_BUDGET);
        let ix = ChunkedIndex::new(&m, &store, &cache, QueryConfig::default(), Provenance::TrustedWriter).unwrap();
        let mut g = SplitMix(max);
        let rounds = if max == 1 { 60 } else { 400 };
        for i in 0..rounds {
            let q = query_point(&mut g, &cloud);
            for k in [1usize, 2, 16, 250, 1024] {
                assert_eq!(bits(&ix.knn(q, k).unwrap()), bits(&mem.knn(q, k).unwrap()), "max {max} q{i} k {k}");
            }
            for r in [0.0, 1e-4, 0.003, 0.05, 30.0] {
                assert_eq!(bits(&ix.radius(q, r).unwrap()), bits(&mem.radius(q, r).unwrap()), "max {max} q{i} r {r}");
            }
            let h = [g.unit() * 0.02, g.unit() * 0.02, g.unit() * 0.002];
            let b =
                Aabb::new([q[0] - h[0], q[1] - h[1], q[2] - h[2]], [q[0] + h[0], q[1] + h[1], q[2] + h[2]]).unwrap();
            assert_eq!(ix.in_box(&b).unwrap(), mem.in_box(&b).unwrap(), "max {max} q{i} box");
        }
        // The duplicate cluster: 200 exact ties, ordered by index.
        let dup = ix.knn([0.1, 0.1, 7.005], 1024).unwrap();
        assert_eq!(bits(&dup), bits(&mem.knn([0.1, 0.1, 7.005], 1024).unwrap()));
        assert!(dup[..200].iter().all(|n| n.dist2 == 0.0));
    }
}

#[test]
fn chunk_bounds_hold_every_point() {
    let cloud = awkward_cloud();
    for max in [1u64, 7, 64] {
        let (store, m) = chunked(&cloud, max);
        let cache = ChunkCache::new(DEFAULT_CACHE_BUDGET);
        let ix = ChunkedIndex::new(&m, &store, &cache, QueryConfig::default(), Provenance::TrustedWriter).unwrap();
        let mut tight = 0usize;
        for pos in 0..m.chunks.len() {
            let (lo, hi) = ix.chunk_bounds(pos).unwrap();
            let h = cache.get(&store, &m, pos).unwrap();
            let mut seen_lo = [f64::INFINITY; 3];
            for j in 0..h.index.len() {
                let p = [h.points.x[j], h.points.y[j], h.points.z[j]];
                for k in 0..3 {
                    assert!(lo[k] <= p[k] && p[k] <= hi[k], "max {max} chunk {pos} point {j} axis {k}");
                    if p[k] < seen_lo[k] {
                        seen_lo[k] = p[k];
                    }
                }
            }
            tight += (0..3).filter(|&k| seen_lo[k] == lo[k]).count();
        }
        assert!(ix.chunk_bounds(m.chunks.len()).is_none());
        // Bounds are cell bounds, not point boxes, but many cells have a point on an edge.
        assert!(tight > 0, "max {max}");
    }
}

#[test]
fn query_argument_errors_are_those_of_geo_index() {
    let cloud = awkward_cloud();
    let (store, m) = chunked(&cloud, 64);
    let cache = ChunkCache::new(DEFAULT_CACHE_BUDGET);
    let ix = ChunkedIndex::new(&m, &store, &cache, QueryConfig::default(), Provenance::TrustedWriter).unwrap();
    let mem = PointIndex::build(cloud.buffer(), 1).unwrap();
    let big = 2f64.powi(501);
    for q in [[f64::NAN, 0.0, 0.0], [0.0, f64::INFINITY, f64::NAN], [0.0, 0.0, -big], [big, f64::NAN, 0.0]] {
        for k in [0usize, 1, 1025] {
            let want = mem.knn(q, k).unwrap_err();
            assert_eq!(ix.knn(q, k).unwrap_err(), QueryError::Index(want));
        }
        assert_eq!(ix.radius(q, 1.0).unwrap_err(), QueryError::Index(mem.radius(q, 1.0).unwrap_err()));
    }
    for k in [0usize, 1025, usize::MAX] {
        let e = ix.knn([0.0; 3], k).unwrap_err();
        assert_eq!(e, QueryError::Index(mem.knn([0.0; 3], k).unwrap_err()));
        assert_eq!(e.code(), "bad_k");
    }
    for r in [-1.0, f64::NAN, f64::INFINITY, big, -0.5e-300] {
        assert_eq!(ix.radius([0.0; 3], r).unwrap_err(), QueryError::Index(PointIndexError::BadRadius), "{r}");
    }
    // -0.0 is a valid radius, as in GEO.INDEX.
    assert_eq!(bits(&ix.radius([0.1, 0.1, 7.005], -0.0).unwrap()).len(), 200);
    // Batches: zero and too many threads.
    let qs = [[0.0; 3]];
    assert_eq!(ix.batch_knn(&qs, 1, 0).unwrap_err(), QueryError::Index(PointIndexError::ZeroThreads));
    assert_eq!(ix.batch_knn(&qs, 1, 257).unwrap_err().code(), "invalid_params");
    // A failing batch reports the first failing query in query order, whatever the threads.
    let mixed = [[0.0; 3], [0.0, f64::NAN, 0.0], [f64::INFINITY, 0.0, 0.0], [0.0; 3]];
    for t in [1, 2, 4] {
        assert_eq!(
            ix.batch_knn(&mixed, 3, t).unwrap_err(),
            QueryError::Index(PointIndexError::NonFiniteQuery { axis: 1 }),
            "threads {t}"
        );
    }
}

#[test]
fn unverified_manifest_is_refused_until_verified() {
    let cloud = awkward_cloud();
    let (store, m) = chunked(&cloud, 64);
    let cache = ChunkCache::new(DEFAULT_CACHE_BUDGET);
    let mut ix = ChunkedIndex::new(&m, &store, &cache, QueryConfig::default(), Provenance::Untrusted).unwrap();
    assert!(!ix.verified());
    let b = Aabb::new([0.0; 3], [1.0, 1.0, 8.0]).unwrap();
    let refused = QueryError::Ooc(OocError::UnverifiedManifest);
    assert_eq!(ix.knn([0.0; 3], 1).unwrap_err(), refused);
    assert_eq!(ix.radius([0.0; 3], 1.0).unwrap_err(), refused);
    assert_eq!(ix.in_box(&b).unwrap_err(), refused);
    assert_eq!(ix.stats().chunk_visits, 0, "nothing loaded");
    ix.verify().unwrap();
    assert!(ix.verified());
    let mem = PointIndex::build(cloud.buffer(), 1).unwrap();
    assert_eq!(bits(&ix.knn([0.0; 3], 5).unwrap()), bits(&mem.knn([0.0; 3], 5).unwrap()));
}

#[test]
fn tampered_chunk_fails_closed() {
    let cloud = awkward_cloud();
    let (store, m) = chunked(&cloud, 64);
    // Swap the bytes of chunk 0 for those of chunk 1: a valid chunk, the wrong one.
    let other = store.get_blob_for_test(&m, 1);
    store.replace(m.chunks[0].blob, other);
    let cache = ChunkCache::new(DEFAULT_CACHE_BUDGET);
    let ix = ChunkedIndex::new(&m, &store, &cache, QueryConfig::default(), Provenance::TrustedWriter).unwrap();
    let (lo, hi) = ix.chunk_bounds(0).unwrap();
    let q = [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0, (lo[2] + hi[2]) / 2.0];
    let e = ix.knn(q, 1).unwrap_err();
    assert_eq!(e.code(), "chunk_integrity", "{e}");
    let b = Aabb::new(lo, hi).unwrap();
    assert_eq!(ix.in_box(&b).unwrap_err().code(), "chunk_integrity");
    let mut un = ChunkedIndex::new(&m, &store, &cache, QueryConfig::default(), Provenance::Untrusted).unwrap();
    assert_eq!(un.verify().unwrap_err().code(), "chunk_integrity");
    assert!(!un.verified());
}

#[test]
fn index_budget_is_enforced() {
    let cloud = awkward_cloud();
    let (store, m) = chunked(&cloud, 256);
    let cache = ChunkCache::new(DEFAULT_CACHE_BUDGET);
    let tiny =
        ChunkedIndex::new(&m, &store, &cache, QueryConfig { index_budget: 64 }, Provenance::TrustedWriter).unwrap();
    match tiny.knn([0.0; 3], 1).unwrap_err() {
        QueryError::Budget(BudgetError::BudgetExceeded { budget, .. }) => assert_eq!(budget, 64),
        other => panic!("{other:?}"),
    }
    assert_eq!(tiny.stats().index_builds, 0);

    // A budget for about one tree: trees are evicted and rebuilt, results do not change.
    let largest = m.chunks.iter().map(|e| e.count).max().unwrap();
    let one =
        fid_geo_index::estimate_bytes(fid_geo_index::IndexKind::KdTree, largest, fid_geo_index::BuildParams::kdtree());
    let ix =
        ChunkedIndex::new(&m, &store, &cache, QueryConfig { index_budget: one }, Provenance::TrustedWriter).unwrap();
    let mem = PointIndex::build(cloud.buffer(), 1).unwrap();
    let mut g = SplitMix(9);
    for _ in 0..200 {
        let q = query_point(&mut g, &cloud);
        assert_eq!(bits(&ix.knn(q, 300).unwrap()), bits(&mem.knn(q, 300).unwrap()));
    }
    let st = ix.stats();
    assert!(st.index_evictions > 0 && st.index_bytes <= one, "{st:?}");
}

#[test]
fn ac4_build_counter_bound() {
    let cloud = awkward_cloud();
    let (store, m) = chunked(&cloud, 32);
    let cache = ChunkCache::new(DEFAULT_CACHE_BUDGET);
    let ix = ChunkedIndex::new(&m, &store, &cache, QueryConfig::default(), Provenance::TrustedWriter).unwrap();
    let mut g = SplitMix(4);
    let qs: Vec<[f64; 3]> = (0..300).map(|_| query_point(&mut g, &cloud)).collect();
    let first = ix.batch_knn(&qs, 16, 4).unwrap();
    let st = ix.stats();
    assert!(st.chunks_touched > 1, "{st:?}");
    assert!(st.index_builds <= st.chunks_touched, "AC-4: {st:?}");
    for _ in 0..3 {
        assert_eq!(ix.batch_knn(&qs, 16, 2).unwrap(), first);
        ix.batch_radius(&qs, 0.002, 3).unwrap();
    }
    let st2 = ix.stats();
    assert!(st2.index_builds <= st2.chunks_touched, "AC-4: {st2:?}");
    assert!(st2.index_hits > st.index_hits);
    // Repeating the same queries builds no tree again.
    let again = ix.stats().index_builds;
    ix.batch_knn(&qs, 16, 1).unwrap();
    assert_eq!(ix.stats().index_builds, again);
}

#[test]
fn cloud_beyond_the_coordinate_limit_is_refused_up_front() {
    let big = 2f64.powi(501);
    let pts = [[0.0, 0.0, 0.0], [1.0, 2.0, 3.0], [0.5, -big, 0.0]];
    let cloud = PointCloud::from_points(frame(), &pts).unwrap();
    let mem = PointIndex::build(cloud.buffer(), 1).unwrap_err();
    assert_eq!(mem, PointIndexError::PointOutOfRange { index: 2, axis: 1 });
    let (store, m) = chunked(&cloud, 1);
    let cache = ChunkCache::new(DEFAULT_CACHE_BUDGET);
    let e = ChunkedIndex::new(&m, &store, &cache, QueryConfig::default(), Provenance::TrustedWriter).unwrap_err();
    assert_eq!(e, QueryError::Index(PointIndexError::PointOutOfRange { index: u64::MAX, axis: 1 }));
    assert_eq!(e.code(), "point_out_of_range");
    // At the limit itself the cloud is accepted, as by GEO.INDEX.
    let lim = fid_geo_index::COORD_LIMIT;
    let pts = [[0.0, 0.0, 0.0], [lim, -lim, 1.0]];
    let cloud = PointCloud::from_points(frame(), &pts).unwrap();
    let mem = PointIndex::build(cloud.buffer(), 1).unwrap();
    let (store, m) = chunked(&cloud, 1);
    let ix = ChunkedIndex::new(&m, &store, &cache, QueryConfig::default(), Provenance::TrustedWriter).unwrap();
    assert_eq!(bits(&ix.knn([1.0; 3], 2).unwrap()), bits(&mem.knn([1.0; 3], 2).unwrap()));
}

/// A store whose writes work and whose reads always fail.
struct FailingReads(SyncStore);

const READ_ERROR: &str = "unavailable";

impl ChunkStore for FailingReads {
    fn put_blob(&self, bytes: &[u8]) -> Result<[u8; 32], OocError> {
        self.0.put_blob(bytes)
    }

    fn get_blob(&self, _: &[u8; 32], _: u64) -> Result<Vec<u8>, OocError> {
        Err(OocError::Store { code: READ_ERROR, detail: "injected read failure".to_string() })
    }
}

#[test]
fn failing_store_reads_fail_every_query() {
    let cloud = awkward_cloud();
    let store = FailingReads(SyncStore::new());
    let m = to_chunks(&cloud, &store, &LayoutParams { max_chunk_points: 64 }).unwrap();
    let cache = ChunkCache::new(DEFAULT_CACHE_BUDGET);
    let ix = ChunkedIndex::new(&m, &store, &cache, QueryConfig::default(), Provenance::TrustedWriter).unwrap();
    let want = QueryError::Ooc(OocError::Store { code: READ_ERROR, detail: "injected read failure".to_string() });
    let q = cloud.point(0).unwrap();
    assert_eq!(ix.knn(q, 4).unwrap_err(), want);
    assert_eq!(ix.radius(q, 0.01).unwrap_err(), want);
    let b = Aabb::new([-1.0, -1.0, 6.0], [1.0, 1.0, 8.0]).unwrap();
    assert_eq!(ix.in_box(&b).unwrap_err(), want);
    assert_eq!(ix.batch_knn(&[q, q], 4, 2).unwrap_err(), want);
    assert_eq!(ix.stats().index_builds, 0, "nothing was built");
    assert_eq!(cache.stats().pinned_chunks, 0);
    // Verification of an untrusted manifest fails with the same error.
    let mut un = ChunkedIndex::new(&m, &store, &cache, QueryConfig::default(), Provenance::Untrusted).unwrap();
    assert_eq!(un.verify().unwrap_err(), want);
}
