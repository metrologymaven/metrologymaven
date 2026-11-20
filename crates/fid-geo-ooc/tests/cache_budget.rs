//! GEO.OOC.CACHE AC-1 (REQ-GEO.OOC.CACHE-001, -004): the first 200 000 points of fixture
//! cloud_5e6 (the prefix plain `cargo test` uses for GEO.OOC.LAYOUT), chunked with
//! `max_chunk_points = 6 144` into 75 chunks, are streamed forward and then in reverse,
//! one handle at a time, through a cache whose budget is 8 times the largest resident
//! chunk cost. The counting allocator's peak while streaming must stay within the budget,
//! and the eviction sequence, hits and misses must equal `fixtures/geo/ooc/evict_seq.json`
//! from the independent Python model `make_cache_fixtures.py`.
//!
//! Own test binary with one test function, because it installs a counting global
//! allocator. `cap` cannot reset its high-water mark, so before streaming the test holds a
//! ballast allocation that lifts the current allocation to the setup's peak: after that,
//! any new high-water mark is the streaming phase's own.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use cap::Cap;
use common::*;
use fid_geo_ooc::{resident_cost, to_chunks, CacheConfig, ChunkCache, LayoutParams, MemStore, ENTRY_OVERHEAD};
use fid_geo_types::{AttributeSet, PointBuffer, PointCloud};
use fid_kci::kci::Sha256;
use std::alloc::System;

#[global_allocator]
static ALLOC: Cap<System> = Cap::new(System, usize::MAX);

#[test]
fn ac1_peak_within_budget_and_eviction_sequence() {
    let fx = json("evict_seq.json");
    let e = json("cloud_5e6/expected.json");
    let g = ScanGen::from_fixture(&e);
    let n = int(get(&fx, "n_points"));
    let max = int(get(&fx, "max_chunk_points"));
    assert_eq!((n, max), (200_000, 6144));
    assert_eq!(int(get(&fx, "entry_overhead")), ENTRY_OVERHEAD);

    let un = n as usize;
    let (mut pts, mut nx, mut ny, mut nz, mut it) =
        (Vec::with_capacity(un), Vec::with_capacity(un), Vec::with_capacity(un), Vec::with_capacity(un), Vec::new());
    for i in 0..n {
        let (p, nrm, inten) = g.point(i);
        pts.push(p);
        nx.push(nrm[0]);
        ny.push(nrm[1]);
        nz.push(nrm[2]);
        it.push(inten);
    }
    let attrs = AttributeSet { normal: Some([nx, ny, nz]), intensity: Some(it), ..AttributeSet::default() };
    let cloud = PointCloud::new(frame(get(&e, "frame")), PointBuffer::from_points(&pts), attrs).unwrap();
    drop(pts);
    let store = MemStore::new();
    let m = to_chunks(&cloud, &store, &LayoutParams { max_chunk_points: max }).unwrap();
    drop(cloud);
    assert_eq!(m.chunks.len() as u64, int(get(&fx, "n_chunks")));
    assert_eq!(hex(&m.manifest_digest), s(get(&fx, "manifest_digest")));
    assert_eq!(m.channels.record_bytes(), int(get(&fx, "record_bytes")));
    let budget = 8 * m.chunks.iter().map(|c| resident_cost(m.channels, c.count).unwrap()).max().unwrap();
    assert_eq!(budget, int(get(&fx, "budget")));

    let want: Vec<(String, u64)> =
        arr(get(&fx, "evictions")).iter().map(|v| (s(get(v, "blob")).to_string(), int(get(v, "cell")))).collect();
    let nch = m.chunks.len();
    let pattern: Vec<usize> = (0..nch).chain((0..nch).rev()).collect();
    assert_eq!(pattern.len() as u64, int(get(&fx, "accesses")));
    let cache = ChunkCache::with_config(CacheConfig { budget, eviction_log: 4096 });

    // Lift the current allocation to the setup's high-water mark.
    let ballast: Vec<u8> = vec![0u8; ALLOC.max_allocated() - ALLOC.allocated()];
    let base = ALLOC.allocated();
    assert_eq!(ALLOC.max_allocated(), base);

    let mut h = Sha256::new();
    for &i in &pattern {
        let c = cache.get(&store, &m, i).unwrap();
        for v in &c.points.x {
            h.update(&v.to_bits().to_le_bytes());
        }
    }
    let peak = (ALLOC.max_allocated() - base) as u64;
    let digest = h.finish();
    drop(ballast);

    assert!(peak <= budget, "peak chunk memory {peak} bytes, budget {budget}");
    let stats = cache.stats();
    assert!(stats.peak_bytes <= budget);
    // The cache really held most of its budget at some point (the test measures something).
    assert!(peak > budget / 2, "peak {peak}, budget {budget}");
    assert_eq!(stats.hits, int(get(&fx, "hits")));
    assert_eq!(stats.misses, int(get(&fx, "misses")));
    assert_eq!(stats.resident_chunks, arr(get(&fx, "final_resident")).len() as u64);
    let got: Vec<(String, u64)> = cache.eviction_log().iter().map(|v| (hex(&v.digest), v.cell.raw())).collect();
    assert_eq!(got.len(), want.len());
    assert_eq!(got, want, "eviction sequence");
    assert_eq!(stats.evictions, want.len() as u64);
    assert_ne!(digest, [0; 32]);
}
