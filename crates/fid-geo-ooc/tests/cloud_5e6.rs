//! AC-1 and AC-2 on fixture cloud_5e6: 5 x 10^6 points with normals and intensity,
//! regenerated from seed 61 (the data is not committed). Expected digests come from the
//! independent Python reference (`fixtures/geo/ooc/make_layout_fixtures.py` ->
//! `cloud_5e6/expected.json`).
//!
//! Slow in debug builds (several minutes; the SHA-256 passes over 300 MB dominate).
//!
//! * AC-1: chunked with `max_chunk_points = 65 536` and rebuilt, the element digest equals
//!   the original.
//! * AC-2: chunked in memory and through `ChunkWriter`, both manifest digests are equal
//!   (and equal the reference).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use common::*;
use fid_geo_ooc::{
    to_chunks, to_memory, Channel, ChannelSet, ChunkWriter, CloudHeader, LayoutParams, PointAttrs, WriterConfig,
};
use fid_geo_types::digest::digest_point_cloud;
use fid_geo_types::{AttributeSet, PointBuffer, PointCloud};

#[test]
fn ac1_ac2_cloud_5e6() {
    let e = json("cloud_5e6/expected.json");
    let g = ScanGen::from_fixture(&e);
    let n = int(get(&e, "n_points"));
    let max = int(get(&e, "max_chunk_points"));
    assert_eq!((n, max, int(get(&e, "seed"))), (5_000_000, 65_536, 61));
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
    assert_eq!(original.to_string(), s(get(&e, "element_digest")), "element digest of the regenerated cloud");

    // AC-1: in-memory chunking and back.
    let ts = TestStore::new("5e6");
    let m = to_chunks(&cloud, ts.store(), &LayoutParams { max_chunk_points: max }).unwrap();
    assert_eq!(m.chunks.len() as u64, int(get(&e, "n_chunks")));
    assert_eq!(hex(&m.manifest_digest), s(get(&e, "manifest_digest")), "manifest digest (in memory)");
    assert_eq!(m.original_digest, original);
    // to_memory refuses a rebuilt cloud whose digest differs from the manifest's original
    // digest, which equals the reference value checked above.
    let back = to_memory(&m, ts.store(), u64::MAX).unwrap();
    assert_eq!(back.len(), n, "AC-1: round trip");
    drop(back);

    // AC-2: the streaming writer, with a budget far below the cloud's 300 MB of records.
    let ts2 = TestStore::new("5e6-writer");
    let header = CloudHeader {
        frame: cloud.frame(),
        bounds: cloud.aabb().unwrap(),
        channels: ChannelSet::EMPTY.with(Channel::Normal).with(Channel::Intensity),
        scan_origins: Vec::new(),
        grid: None,
    };
    let dir = temp_path("5e6-spill");
    let cfg = WriterConfig { max_chunk_points: max, spill_budget: 64 << 20 };
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
    assert!(stats.spilled_bytes > 64 << 20);
    assert_eq!(wm.manifest_digest, m.manifest_digest, "AC-2: writer and in-memory manifests");
    assert_eq!(wm, m);
}
