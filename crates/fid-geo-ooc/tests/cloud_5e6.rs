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
//!
//! GEO.OOC.QUERY on the same cloud (`query_ac1_ac3_ac4`, same sizes: the 200 000-point
//! prefix by default, the full cloud with `FID_OOC_FULL=1`, which CI runs in release on
//! Linux):
//!
//! * AC-1: chunked with `max_chunk_points = 65 536` (and 4 096 on the prefix, for many more
//!   chunk boundaries), 10^4 seeded queries (2 000 each of k = 1, 16, 1024, radius 1 mm and
//!   boxes; half of them moved exactly onto, or one ulp beside, a chunk's cell edge) return
//!   exactly the in-memory GEO.INDEX results. On the prefix, 60 further queries are checked
//!   against the independent brute-force reference `fixtures/geo/ooc/make_query_fixtures.py`
//!   (`query_vectors.json`), both chunked and in memory.
//! * AC-3: the batch queries with 1 and 8 threads give byte-identical results.
//! * AC-4: the per-chunk index build counter does not exceed the distinct chunks touched,
//!   and repeating the queries builds nothing.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use common::*;
use fid_geo_index::{Neighbour, PointIndex};
use fid_geo_ooc::{
    original_digest, to_chunks, to_memory, Channel, ChannelSet, ChunkCache, ChunkWriter, ChunkedIndex, CloudHeader,
    LayoutParams, PointAttrs, Provenance, QueryConfig, StreamConfig, StreamOrder, WriterConfig, DEFAULT_CACHE_BUDGET,
};
use fid_geo_types::digest::digest_point_cloud;
use fid_geo_types::{Aabb, AttributeSet, PointBuffer, PointCloud};

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

/// Serialises neighbour lists for a byte comparison (AC-3).
fn ser(v: &[Vec<Neighbour>]) -> Vec<u8> {
    let mut out = Vec::new();
    for r in v {
        out.extend_from_slice(&(r.len() as u64).to_le_bytes());
        for n in r {
            out.extend_from_slice(&n.index.to_le_bytes());
            out.extend_from_slice(&n.dist2.to_bits().to_le_bytes());
        }
    }
    out
}

fn ser_hits(v: &[Vec<u64>]) -> Vec<u8> {
    let mut out = Vec::new();
    for r in v {
        out.extend_from_slice(&(r.len() as u64).to_le_bytes());
        for i in r {
            out.extend_from_slice(&i.to_le_bytes());
        }
    }
    out
}

/// 2 000 query points near the cloud's surface; every other one moved exactly onto a chunk
/// cell edge (or one ulp beside it) on one axis.
fn query_points<S: fid_geo_ooc::ChunkStore>(
    g: &mut SplitMix,
    cloud: &PointCloud,
    ix: &ChunkedIndex<'_, S>,
) -> Vec<[f64; 3]> {
    let chunks = ix.manifest().chunks.len() as u64;
    (0..2000)
        .map(|i| {
            let p = cloud.point(g.next() % cloud.len()).unwrap();
            let mut q: [f64; 3] = core::array::from_fn(|k| p[k] + (g.unit() - 0.5) * 0.002);
            if i % 2 == 1 {
                let (lo, hi) = ix.chunk_bounds((g.next() % chunks) as usize).unwrap();
                let k = (g.next() % 3) as usize;
                let edge = if g.next() % 2 == 0 { lo[k] } else { hi[k] };
                q[k] = match g.next() % 3 {
                    0 => edge,
                    1 => f64::from_bits(edge.to_bits() + 1),
                    _ => f64::from_bits(edge.to_bits().wrapping_sub(1)),
                };
                if !q[k].is_finite() {
                    q[k] = edge;
                }
            }
            q
        })
        .collect()
}

/// GEO.OOC.QUERY AC-1, AC-3 and AC-4 (see the file header).
#[test]
fn query_ac1_ac3_ac4() {
    let e = json("cloud_5e6/expected.json");
    let qv = json("query_vectors.json");
    let g = ScanGen::from_fixture(&e);
    let full = std::env::var("FID_OOC_FULL").is_ok_and(|v| v == "1");
    let n = if full { int(get(&e, "n_points")) } else { int(get(&qv, "n_points")) };
    let pts: Vec<[f64; 3]> = (0..n).map(|i| g.point(i).0).collect();
    let cloud = PointCloud::from_points(frame(get(&e, "frame")), &pts).unwrap();
    drop(pts);
    let mem = PointIndex::build(cloud.buffer(), 1).unwrap();

    // The pinned subset from the independent reference (the prefix only).
    let oracle: Vec<&fid_canon::CValue> = if full { Vec::new() } else { arr(get(&qv, "queries")).iter().collect() };
    if !full {
        let b = cloud.aabb().unwrap();
        assert_eq!((b.min(), b.max()), (f3(get(&qv, "aabb_min")), f3(get(&qv, "aabb_max"))), "reference cloud");
    }
    let check_oracle = |knn: &dyn Fn([f64; 3], usize) -> Vec<Neighbour>,
                        radius: &dyn Fn([f64; 3], f64) -> Vec<Neighbour>,
                        in_box: &dyn Fn(&Aabb) -> Vec<u64>,
                        what: &str| {
        for (i, q) in oracle.iter().enumerate() {
            let kind = s(get(q, "kind"));
            let (count, digest) = if kind.starts_with("knn") {
                let r = knn(f3(get(q, "point")), int(get(q, "k")) as usize);
                let first = arr(get(q, "first"));
                assert_eq!((r[0].index, r[0].dist2.to_bits()), (int(&first[0]), f64_hex(&first[1]).to_bits()));
                (r.len(), neighbour_digest(&r))
            } else if kind == "radius" {
                let r = radius(f3(get(q, "point")), f64_hex(get(q, "r")));
                (r.len(), neighbour_digest(&r))
            } else {
                let b = Aabb::new(f3(get(q, "lo")), f3(get(q, "hi"))).unwrap();
                let r = in_box(&b);
                (r.len(), hits_digest(&r))
            };
            assert_eq!(count as u64, int(get(q, "count")), "{what}: reference query {i} ({kind}) count");
            assert_eq!(digest, s(get(q, "digest")), "{what}: reference query {i} ({kind})");
        }
    };
    check_oracle(
        &|q, k| mem.knn(q, k).unwrap(),
        &|q, r| mem.radius(q, r).unwrap(),
        &|b| mem.in_box(b).unwrap(),
        "in memory",
    );

    let sizes: &[u64] = if full { &[65_536] } else { &[65_536, 4_096] };
    for &max in sizes {
        let store = SyncStore::new();
        let m = to_chunks(&cloud, &store, &LayoutParams { max_chunk_points: max }).unwrap();
        let cache = ChunkCache::new(DEFAULT_CACHE_BUDGET);
        let ix = ChunkedIndex::new(&m, &store, &cache, QueryConfig::default(), Provenance::TrustedWriter).unwrap();
        assert!(m.chunks.len() > 1, "max {max}: {} chunks", m.chunks.len());
        check_oracle(
            &|q, k| ix.knn(q, k).unwrap(),
            &|q, r| ix.radius(q, r).unwrap(),
            &|b| ix.in_box(b).unwrap(),
            &format!("chunked {max}"),
        );

        // AC-1: 10^4 seeded queries, all identical to the in-memory index.
        let mut rng = SplitMix(0x0c00_0063 ^ max);
        let mut sets: Vec<(&str, Vec<[f64; 3]>)> = Vec::new();
        for kind in ["knn1", "knn16", "knn1024", "radius", "box"] {
            sets.push((kind, query_points(&mut rng, &cloud, &ix)));
        }
        let boxes: Vec<Aabb> = sets[4]
            .1
            .iter()
            .enumerate()
            .map(|(i, q)| {
                let h = if i % 2 == 0 { 0.002 } else { 0.01 };
                Aabb::new([q[0] - h, q[1] - h, q[2] - h], [q[0] + h, q[1] + h, q[2] + h]).unwrap()
            })
            .collect();
        let mut one_thread: Vec<Vec<u8>> = Vec::new();
        let mut found = 0usize;
        for (kind, qs) in &sets {
            let (got, want) = match *kind {
                "radius" => {
                    let got = ix.batch_radius(qs, 0.001, 1).unwrap();
                    let want: Vec<Vec<Neighbour>> = qs.iter().map(|q| mem.radius(*q, 0.001).unwrap()).collect();
                    (got, want)
                }
                "box" => {
                    let got = ix.batch_in_box(&boxes, 1).unwrap();
                    let want: Vec<Vec<u64>> = boxes.iter().map(|b| mem.in_box(b).unwrap()).collect();
                    assert!(got == want, "AC-1 max {max}: box queries differ from in memory");
                    found += got.iter().map(Vec::len).sum::<usize>();
                    one_thread.push(ser_hits(&got));
                    continue;
                }
                _ => {
                    let k: usize = kind[3..].parse().unwrap();
                    let got = ix.batch_knn(qs, k, 1).unwrap();
                    let want: Vec<Vec<Neighbour>> = qs.iter().map(|q| mem.knn(*q, k).unwrap()).collect();
                    (got, want)
                }
            };
            for (i, (a, b)) in got.iter().zip(&want).enumerate() {
                assert_eq!(bits(a), bits(b), "AC-1 max {max}: {kind} query {i} at {:?}", qs[i]);
            }
            assert_eq!(got.len(), 2000);
            found += got.iter().map(Vec::len).sum::<usize>();
            one_thread.push(ser(&got));
        }
        assert!(found > 2000 * (1 + 16 + 1024), "max {max}: the radius and box queries found points");

        // AC-4: one build per distinct chunk touched at most.
        let st = ix.stats();
        assert!(st.index_builds <= st.chunks_touched, "AC-4 max {max}: {st:?}");

        // AC-3: 8 threads give the same bytes as 1.
        for (n_set, (kind, qs)) in sets.iter().enumerate() {
            let bytes = match *kind {
                "radius" => ser(&ix.batch_radius(qs, 0.001, 8).unwrap()),
                "box" => ser_hits(&ix.batch_in_box(&boxes, 8).unwrap()),
                _ => ser(&ix.batch_knn(qs, kind[3..].parse().unwrap(), 8).unwrap()),
            };
            assert!(bytes == one_thread[n_set], "AC-3 max {max}: {kind} with 8 threads");
        }
        // AC-4: the repeated queries built nothing.
        let st2 = ix.stats();
        assert_eq!(st2.index_builds, st.index_builds, "AC-4 max {max}: {st2:?}");
        assert!(st2.index_builds <= st2.chunks_touched);
        assert_eq!(cache.stats().pinned_chunks, 0);
    }
}
