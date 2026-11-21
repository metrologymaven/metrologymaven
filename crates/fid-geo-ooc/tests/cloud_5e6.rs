//! AC-1 and AC-2 on fixture cloud_5e6: 5 x 10^6 points with normals and intensity,
//! regenerated from seed 61 (the data is not committed). Expected digests come from the
//! independent Python reference (`fixtures/geo/ooc/make_layout_fixtures.py` ->
//! `cloud_5e6/expected.json`).
//!
//! Plain `cargo test` runs the first 200 000 points with `max_chunk_points = 4 096` (the
//! `small` block of the expected file, also from the Python reference). The full
//! 5 x 10^6-point run, which takes minutes in a debug build, runs with `FID_OOC_FULL=1`;
//! CI runs it in release on Linux.
//!
//! * AC-1: chunked with `max_chunk_points = 65 536` and rebuilt, the element digest equals
//!   the original.
//! * AC-2: chunked in memory and through `ChunkWriter`, both manifest digests are equal
//!   (and equal the reference).
//!
//! GEO.OOC.STREAM on the same cloud (`stream_ac1_ac2_ac3`, same sizes; sequence digests from
//! the independent Python reference `fixtures/geo/ooc/make_stream_fixtures.py` ->
//! `stream_vectors.json`):
//!
//! * AC-1: streamed in spatial order with block sizes 1 000 and 65 536, both sequences are
//!   identical and hold every original index exactly once.
//! * AC-2: streamed in original order and digested, the digest equals the manifest's
//!   original digest (and the reference element digest).
//! * AC-3: the spatial-order sequence digest equals the reference value, which this test
//!   checks on every CI platform (Linux, Windows, macOS).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use common::*;
use fid_geo_ooc::{
    original_digest, to_chunks, to_memory, Channel, ChannelSet, ChunkCache, ChunkWriter, CloudHeader, LayoutParams,
    PointAttrs, StreamConfig, StreamOrder, WriterConfig, DEFAULT_CACHE_BUDGET,
};
use fid_geo_types::digest::digest_point_cloud;
use fid_geo_types::{AttributeSet, PointBuffer, PointCloud};

#[test]
fn ac1_ac2_cloud_5e6() {
    let e = json("cloud_5e6/expected.json");
    let g = ScanGen::from_fixture(&e);
    assert_eq!(
        (int(get(&e, "n_points")), int(get(&e, "max_chunk_points")), int(get(&e, "seed"))),
        (5_000_000, 65_536, 61)
    );
    let full = std::env::var("FID_OOC_FULL").is_ok_and(|v| v == "1");
    let spec = if full { &e } else { get(&e, "small") };
    let n = int(get(spec, "n_points"));
    let max = int(get(spec, "max_chunk_points"));
    // The writer's working budget is far below the cloud's records (60 bytes per point).
    let budget = if full { 64 << 20 } else { fid_geo_ooc::writer::FIXED_OVERHEAD + (4 << 20) };
    for sample in arr(get(&e, "samples")) {
        let (p, nrm, inten) = g.point(int(get(sample, "index")));
        let want = arr(get(sample, "point"));
        let wn = arr(get(sample, "normal"));
        for ((a, b), (c, d)) in p.iter().zip(want).zip(nrm.iter().zip(wn)) {
            assert_eq!(a.to_bits(), f64_hex(b).to_bits(), "sample point");
            assert_eq!(c.to_bits(), f64_hex(d).to_bits(), "sample normal");
        }
        assert_eq!(inten.to_bits(), f32_hex(get(sample, "intensity")).to_bits(), "sample intensity");
    }

    let un = n as usize;
    let mut pts = Vec::with_capacity(un);
    let (mut nx, mut ny, mut nz, mut it) =
        (Vec::with_capacity(un), Vec::with_capacity(un), Vec::with_capacity(un), Vec::with_capacity(un));
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
    let original = digest_point_cloud(&cloud);
    assert_eq!(original.to_string(), s(get(spec, "element_digest")), "element digest of the regenerated cloud");

    // AC-1: in-memory chunking and back.
    let ts = TestStore::new("5e6");
    let m = to_chunks(&cloud, ts.store(), &LayoutParams { max_chunk_points: max }).unwrap();
    assert_eq!(m.chunks.len() as u64, int(get(spec, "n_chunks")));
    assert_eq!(hex(&m.manifest_digest), s(get(spec, "manifest_digest")), "manifest digest (in memory)");
    assert_eq!(m.original_digest, original);
    // to_memory refuses a rebuilt cloud whose digest differs from the manifest's original
    // digest, which equals the reference value checked above.
    let back = to_memory(&m, ts.store(), u64::MAX).unwrap();
    assert_eq!(back.len(), n, "AC-1: round trip");
    drop(back);

    // AC-2: the streaming writer.
    let ts2 = TestStore::new("5e6-writer");
    let header = CloudHeader {
        frame: cloud.frame(),
        bounds: cloud.aabb().unwrap(),
        channels: ChannelSet::EMPTY.with(Channel::Normal).with(Channel::Intensity),
        scan_origins: Vec::new(),
        grid: None,
    };
    let dir = temp_path("5e6-spill");
    let cfg = WriterConfig { max_chunk_points: max, spill_budget: budget };
    let mut w = ChunkWriter::new(ts2.store(), &dir, header, cfg).unwrap();
    let a = cloud.attributes();
    let (nrm, inten) = (a.normal.as_ref().unwrap(), a.intensity.as_ref().unwrap());
    for (i, &it) in inten.iter().enumerate() {
        let pa = PointAttrs {
            normal: Some([nrm[0][i], nrm[1][i], nrm[2][i]]),
            intensity: Some(it),
            ..PointAttrs::default()
        };
        w.push(cloud.point(i as u64).unwrap(), &pa).unwrap();
    }
    let (wm, stats) = w.finish().unwrap();
    assert!(stats.spilled_bytes > budget - fid_geo_ooc::writer::FIXED_OVERHEAD);
    assert_eq!(wm.manifest_digest, m.manifest_digest, "AC-2: writer and in-memory manifests");
    assert_eq!(wm, m);
}

/// GEO.OOC.STREAM AC-1, AC-2 and AC-3 (see the file header).
#[test]
fn stream_ac1_ac2_ac3() {
    let e = json("cloud_5e6/expected.json");
    let sv = json("stream_vectors.json");
    let g = ScanGen::from_fixture(&e);
    let full = std::env::var("FID_OOC_FULL").is_ok_and(|v| v == "1");
    let (spec, want) = if full { (&e, get(&sv, "cloud_5e6")) } else { (get(&e, "small"), get(&sv, "cloud_5e6_small")) };
    let n = int(get(spec, "n_points"));
    let max = int(get(spec, "max_chunk_points"));
    assert_eq!((int(get(want, "n_points")), int(get(want, "max_chunk_points"))), (n, max));

    let un = n as usize;
    let mut pts = Vec::with_capacity(un);
    let (mut nx, mut ny, mut nz, mut it) =
        (Vec::with_capacity(un), Vec::with_capacity(un), Vec::with_capacity(un), Vec::with_capacity(un));
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
    let ts = TestStore::new("5e6-stream");
    let m = to_chunks(&cloud, ts.store(), &LayoutParams { max_chunk_points: max }).unwrap();
    drop(cloud);
    assert_eq!(hex(&m.manifest_digest), s(get(spec, "manifest_digest")), "the chunked cloud of the reference");
    assert_eq!(m.chunks.len() as u64, int(get(want, "n_chunks")));
    let cache = ChunkCache::new(DEFAULT_CACHE_BUDGET);
    let cfg = |order, block_points| StreamConfig { order, block_points, channels: m.channels, fan_in: 8 };

    // AC-1: two block sizes, one sequence, every index once.
    let (a, da, la) = stream_all(&m, ts.store(), &cache, cfg(StreamOrder::Spatial, 1000));
    let (b, db, lb) = stream_all(&m, ts.store(), &cache, cfg(StreamOrder::Spatial, 65_536));
    assert_eq!((la.len(), lb.len()), (un.div_ceil(1000), un.div_ceil(65_536)));
    assert!(a == b, "AC-1: the spatial sequence does not depend on the block size");
    assert_eq!(da, db);
    assert_eq!(a.len(), un);
    let mut seen = vec![false; un];
    for &i in &a {
        assert!(!std::mem::replace(&mut seen[i as usize], true), "AC-1: index {i} twice");
    }
    assert!(seen.iter().all(|v| *v), "AC-1: every index");
    drop((a, b, seen));

    // AC-3: the sequence digest is the reference's, on every platform CI runs.
    assert_eq!(da, s(get(want, "spatial_digest")), "AC-3: spatial-order sequence digest");

    // AC-2: the original-order element digest is the manifest's original digest.
    let d = original_digest(&m, ts.store(), &cache, None, cfg(StreamOrder::Original, 65_536)).unwrap();
    assert_eq!(d, m.original_digest, "AC-2");
    assert_eq!(d.to_string(), s(get(spec, "element_digest")), "AC-2: reference element digest");
    let (idx, dorig, _) = stream_all(&m, ts.store(), &cache, cfg(StreamOrder::Original, 1000));
    assert!(idx.iter().copied().eq(0..n), "original order");
    assert_eq!(dorig, s(get(want, "original_digest")), "original-order sequence digest");
    assert_eq!(cache.stats().pinned_chunks, 0);
}
