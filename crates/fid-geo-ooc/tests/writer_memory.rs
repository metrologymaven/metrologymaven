//! AC-3 (REQ-GEO.OOC.LAYOUT-004), scaled for every CI runner: 400 000 points of the
//! cloud_5e6 generator (normals and intensity, 24 MB of records) streamed into a
//! `ChunkWriter` with a 16 MiB spill budget. The counting allocator must report a peak
//! below the budget while the writer runs (store included), and the manifest must list
//! every point and equal the in-memory layout of the same points. The full-size run
//! (10^9 points, 1 GiB) is `writer_1e9.rs`, ignored by default. Own test binary with one
//! test function, because it installs a counting global allocator.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use cap::Cap;
use common::*;
use fid_geo_ooc::{
    to_chunks, Channel, ChannelSet, ChunkStore, ChunkWriter, CloudHeader, LayoutParams, PointAttrs, WriterConfig,
};
use fid_geo_types::digest::digest_point_cloud;
use fid_geo_types::{Aabb, AttributeSet, PointBuffer, PointCloud};
use std::alloc::System;

#[global_allocator]
static ALLOC: Cap<System> = Cap::new(System, usize::MAX);

const N: u64 = 400_000;
const BUDGET: u64 = 16 << 20;
const MAX_CHUNK: u64 = 16_384;

/// IEEE 754 totalOrder bounding box of the first `n` generated points (IO's first pass).
fn bounds(g: &ScanGen, n: u64) -> Aabb {
    let (mut lo, mut hi) = ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]);
    for i in 0..n {
        let (p, _, _) = g.point(i);
        for ((v, l), h) in p.iter().zip(lo.iter_mut()).zip(hi.iter_mut()) {
            if v.total_cmp(l).is_lt() {
                *l = *v;
            }
            if v.total_cmp(h).is_gt() {
                *h = *v;
            }
        }
    }
    Aabb::new(lo, hi).unwrap()
}

#[test]
fn ac3_writer_peak_memory_stays_below_budget() {
    let g = ScanGen::from_fixture(&json("cloud_5e6/expected.json"));
    let frame = frame(get(&json("cloud_5e6/expected.json"), "frame"));
    let b = bounds(&g, N);
    #[cfg(feature = "store")]
    let ts = TestStore::new("mem");
    #[cfg(feature = "store")]
    let store: &dyn ChunkStore = ts.store();
    #[cfg(not(feature = "store"))]
    let store: &dyn ChunkStore = &NullStore;
    let dir = temp_path("mem-spill");
    let header = CloudHeader {
        frame,
        bounds: b,
        channels: ChannelSet::EMPTY.with(Channel::Normal).with(Channel::Intensity),
        scan_origins: Vec::new(),
        grid: None,
    };
    let cfg = WriterConfig { max_chunk_points: MAX_CHUNK, spill_budget: BUDGET };

    let base = ALLOC.allocated();
    let mut w = ChunkWriter::new(store, &dir, header, cfg).unwrap();
    for i in 0..N {
        let (p, n, inten) = g.point(i);
        w.push(p, &PointAttrs { normal: Some(n), intensity: Some(inten), ..PointAttrs::default() }).unwrap();
    }
    let (m, stats) = w.finish().unwrap();
    let peak = ALLOC.max_allocated() as u64 - base as u64;

    assert!(peak < BUDGET, "peak writer memory {peak} bytes, budget {BUDGET}");
    assert!(stats.spilled_bytes > BUDGET, "the input ({} bytes) is larger than the budget", stats.spilled_bytes);
    assert_eq!(m.n_points, N);
    assert_eq!(m.chunks.iter().map(|e| e.count).sum::<u64>(), N);
    assert!(!dir.exists());

    // Not measured: the in-memory layout of the same points gives the same manifest.
    let n = N as usize;
    let (mut pts, mut nx, mut ny, mut nz, mut it) =
        (Vec::with_capacity(n), Vec::with_capacity(n), Vec::with_capacity(n), Vec::with_capacity(n), Vec::new());
    for i in 0..N {
        let (p, nn, inten) = g.point(i);
        pts.push(p);
        nx.push(nn[0]);
        ny.push(nn[1]);
        nz.push(nn[2]);
        it.push(inten);
    }
    let attrs = AttributeSet { normal: Some([nx, ny, nz]), intensity: Some(it), ..AttributeSet::default() };
    let cloud = PointCloud::new(frame, PointBuffer::from_points(&pts), attrs).unwrap();
    assert_eq!(cloud.aabb().unwrap(), b);
    assert_eq!(m.original_digest, digest_point_cloud(&cloud));
    let mem = to_chunks(&cloud, store, &LayoutParams { max_chunk_points: MAX_CHUNK }).unwrap();
    assert_eq!(mem.manifest_digest, m.manifest_digest);
}
