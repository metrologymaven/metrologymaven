//! AC-3 at full size (REQ-GEO.OOC.LAYOUT-004): 10^9 points of the seeded cloud_5e6
//! generator streamed into a `ChunkWriter` with the default 1 GiB spill budget; the
//! counting allocator must report a peak below 1 GiB and the manifest must list 10^9
//! points. Ignored by default: it needs a large runner with about 200 GB of free disk for
//! the spill files and the blob store [unverified runner]. Run it with
//!
//! ```text
//! FID_OOC_WORK_DIR=/big/disk cargo test --release -p fid-geo-ooc --test writer_1e9 -- --ignored
//! ```
//!
//! Own test binary with one test function, because it installs a counting global
//! allocator. The scaled version that CI runs is `writer_memory.rs`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use cap::Cap;
use common::*;
use fid_geo_ooc::{Channel, ChannelSet, ChunkStore, ChunkWriter, CloudHeader, PointAttrs, WriterConfig};
use fid_geo_types::Aabb;
use std::alloc::System;
use std::path::PathBuf;

#[global_allocator]
static ALLOC: Cap<System> = Cap::new(System, usize::MAX);

const N: u64 = 1_000_000_000;
const BUDGET: u64 = fid_geo_ooc::DEFAULT_SPILL_BUDGET;

#[test]
#[ignore = "needs a large runner: 10^9 points, about 200 GB of disk [unverified runner]"]
fn ac3_one_billion_points_under_one_gib() {
    let fixture = json("cloud_5e6/expected.json");
    let g = ScanGen::from_fixture(&fixture);
    let work = std::env::var_os("FID_OOC_WORK_DIR").map(PathBuf::from).unwrap_or_else(|| temp_path("1e9"));
    std::fs::create_dir_all(&work).unwrap();
    // The generator's facets lie in [0, 2] x [0, 2] x [-0.1, 1] with noise below 0.001
    // along unit normals, so these bounds contain every point (the file-header case).
    let bounds = Aabb::new([-0.2; 3], [2.2, 2.2, 1.2]).unwrap();
    #[cfg(feature = "store")]
    let blob = fid_store::BlobStore::open(&work.join("blobs")).unwrap();
    #[cfg(feature = "store")]
    let store: &dyn ChunkStore = &blob;
    #[cfg(not(feature = "store"))]
    let store: &dyn ChunkStore = &NullStore;
    let header = CloudHeader {
        frame: frame(get(&fixture, "frame")),
        bounds,
        channels: ChannelSet::EMPTY.with(Channel::Normal).with(Channel::Intensity),
        scan_origins: Vec::new(),
        grid: None,
    };
    let cfg = WriterConfig { spill_budget: BUDGET, ..WriterConfig::default() };
    let base = ALLOC.allocated();
    let mut w = ChunkWriter::new(store, &work.join("spill"), header, cfg).unwrap();
    for i in 0..N {
        let (p, n, inten) = g.point(i);
        w.push(p, &PointAttrs { normal: Some(n), intensity: Some(inten), ..PointAttrs::default() }).unwrap();
    }
    let (m, _) = w.finish().unwrap();
    let peak = ALLOC.max_allocated() as u64 - base as u64;
    assert!(peak < BUDGET, "peak writer memory {peak} bytes");
    assert_eq!(m.n_points, N);
    assert_eq!(m.chunks.iter().map(|e| e.count).sum::<u64>(), N);
}
