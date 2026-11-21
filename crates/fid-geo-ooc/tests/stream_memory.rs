//! GEO.OOC.STREAM memory bound. The first 200 000 points of fixture cloud_5e6 (187 chunks
//! of at most 4 096 points) are streamed in spatial order, and the first 30 000 points
//! (chunked with `max_chunk_points = 256`) in original order, through a chunk cache whose
//! budget is four chunk loads. The original order of this cloud jumps between chunks, so a
//! small cache re-fetches often; the shorter prefix keeps the debug run short. The counting
//! allocator's peak while streaming must stay within the cache budget plus one block, the
//! merge state (24 bytes per chunk) and a fixed slack, so memory does not grow with the
//! cloud. The spatial run is repeated through the `PointSource` view (`next_block`), whose
//! conversion to `f64` channel columns may hold a second block-sized copy, so its bound adds
//! one more block.
//!
//! Own test binary with one test function, because it installs a counting global allocator
//! (see `cache_budget.rs` for the ballast technique).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use cap::Cap;
use common::*;
use fid_geo_ooc::{
    load_cost, to_chunks, ChunkCache, ChunkStream, LayoutParams, MemStore, Provenance, StreamConfig, StreamOrder,
};
use fid_geo_types::{AttributeSet, PointBuffer, PointCloud, PointSource};
use std::alloc::System;

#[global_allocator]
static ALLOC: Cap<System> = Cap::new(System, usize::MAX);

/// Allocator slack for the stream's small fixed parts (pin list, hasher, map nodes).
const SLACK: u64 = 128 * 1024;

#[test]
fn streaming_peak_is_bounded_by_cache_budget_and_block() {
    let e = json("cloud_5e6/expected.json");
    let spec = get(&e, "small");
    let (n, max) = (int(get(spec, "n_points")), int(get(spec, "max_chunk_points")));
    check(&e, StreamOrder::Spatial, n, max);
    check(&e, StreamOrder::Original, 30_000, 256);
}

fn check(e: &fid_canon::CValue, order: StreamOrder, n: u64, max: u64) {
    let g = ScanGen::from_fixture(e);
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
    let cloud = PointCloud::new(frame(get(e, "frame")), PointBuffer::from_points(&pts), attrs).unwrap();
    drop(pts);
    let store = MemStore::new();
    let m = to_chunks(&cloud, &store, &LayoutParams { max_chunk_points: max }).unwrap();
    drop(cloud);
    let load = m.chunks.iter().map(|c| load_cost(m.channels, c).unwrap()).max().unwrap();
    let budget = 4 * load;
    let block = 1000usize;
    let block_bytes = block as u64 * m.channels.record_bytes();
    let merge = m.chunks.len() as u64 * fid_geo_ooc::stream::MERGE_BYTES_PER_CHUNK;
    let cloud_bytes = n * m.channels.record_bytes();
    assert!(budget + block_bytes + merge + SLACK < cloud_bytes / 4, "the bound is far below the cloud");

    {
        let cache = ChunkCache::new(budget);
        let ballast: Vec<u8> = vec![0u8; ALLOC.max_allocated() - ALLOC.allocated()];
        let base = ALLOC.allocated();
        let cfg = StreamConfig { order, block_points: block, channels: m.channels, fan_in: 8 };
        let mut s = ChunkStream::new(&m, &store, &cache, None, cfg, Provenance::TrustedWriter).unwrap();
        let mut count = 0u64;
        let mut xor = 0u64;
        while let Some(b) = s.next_stream_block() {
            let b = b.unwrap();
            count += b.len() as u64;
            xor ^= b.x.iter().fold(0, |a, v| a ^ v.to_bits());
        }
        drop(s);
        let peak = (ALLOC.max_allocated() - base) as u64;
        drop(ballast);
        assert_eq!(count, n);
        assert_ne!(xor, 0);
        let bound = budget + block_bytes + merge + SLACK;
        assert!(peak <= bound, "{order:?}: peak {peak} bytes, bound {bound}");
        assert!(cache.stats().peak_bytes <= budget);
    }
    if order == StreamOrder::Spatial {
        let cache = ChunkCache::new(budget);
        let ballast: Vec<u8> = vec![0u8; ALLOC.max_allocated() - ALLOC.allocated()];
        let base = ALLOC.allocated();
        let cfg = StreamConfig { order, block_points: block, channels: m.channels, fan_in: 8 };
        let mut s = ChunkStream::new(&m, &store, &cache, None, cfg, Provenance::TrustedWriter).unwrap();
        let mut count = 0u64;
        while let Some(b) = PointSource::next_block(&mut s) {
            let b = b.unwrap();
            assert_eq!(b.channels.len(), 4, "intensity and three normal columns");
            count += b.x.len() as u64;
        }
        drop(s);
        let peak = (ALLOC.max_allocated() - base) as u64;
        drop(ballast);
        assert_eq!(count, n);
        let bound = budget + 2 * block_bytes + merge + SLACK;
        assert!(peak <= bound, "PointSource view: peak {peak} bytes, bound {bound}");
    }
}
