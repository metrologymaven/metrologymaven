//! GEO.OOC.LAYOUT against the independent Python reference
//! (`fixtures/geo/ooc/make_layout_fixtures.py` -> `layout_vectors.json`): partition, chunk
//! bytes and store addresses, manifest bytes and digests, original digests. Every case is
//! also round-tripped (chunks -> cloud -> chunks, byte-identical), rebuilt through the
//! streaming writer (same manifest digest, REQ-GEO.OOC.LAYOUT-003), and checked for input
//! order independence.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use common::*;
use fid_canon::CValue;
use fid_geo_ooc::writer::FIXED_OVERHEAD;
use fid_geo_ooc::{
    decode_chunk, get_manifest, put_manifest, to_chunks, to_chunks_in, to_memory, CellCode, ChunkManifest, ChunkWriter,
    CloudHeader, LayoutParams, PointAttrs, WriterConfig,
};
use fid_geo_types::digest::digest_point_cloud;
use fid_geo_types::{Aabb, AttributeSet, PointBuffer, PointCloud};

fn cases() -> Vec<CValue> {
    arr(get(&json("layout_vectors.json"), "cases")).to_vec()
}

fn case(name: &str) -> CValue {
    cases().into_iter().find(|c| s(get(c, "name")) == name).unwrap()
}

fn bounds_of(c: &CValue) -> Option<Aabb> {
    let b = get(c, "bounds");
    if is_null(b) {
        return None;
    }
    let lo = arr(get(b, "min"));
    let hi = arr(get(b, "max"));
    Some(
        Aabb::new(
            [f64_hex(&lo[0]), f64_hex(&lo[1]), f64_hex(&lo[2])],
            [f64_hex(&hi[0]), f64_hex(&hi[1]), f64_hex(&hi[2])],
        )
        .unwrap(),
    )
}

fn chunk_case(c: &CValue, cloud: &PointCloud, store: &dyn fid_geo_ooc::ChunkStore) -> ChunkManifest {
    let params = LayoutParams { max_chunk_points: int(get(c, "max_chunk_points")) };
    match bounds_of(c) {
        Some(b) => to_chunks_in(cloud, &b, store, &params).unwrap(),
        None => to_chunks(cloud, store, &params).unwrap(),
    }
}

/// The manifest equals the vector's expected values.
fn check_expected(name: &str, exp: &CValue, m: &ChunkManifest) {
    let lo = arr(get(exp, "aabb_min"));
    let hi = arr(get(exp, "aabb_max"));
    for (k, (l, h)) in lo.iter().zip(hi).enumerate() {
        assert_eq!(m.aabb.min()[k].to_bits(), f64_hex(l).to_bits(), "{name} min {k}");
        assert_eq!(m.aabb.max()[k].to_bits(), f64_hex(h).to_bits(), "{name} max {k}");
    }
    let chunks = arr(get(exp, "chunks"));
    assert_eq!(m.chunks.len(), chunks.len(), "{name}: chunk count");
    for (i, (e, x)) in m.chunks.iter().zip(chunks).enumerate() {
        assert_eq!(e.cell.raw(), int(get(x, "cell")), "{name} chunk {i} cell");
        assert_eq!(e.count, int(get(x, "count")), "{name} chunk {i} count");
        assert_eq!(e.bytes, int(get(x, "bytes")), "{name} chunk {i} bytes");
        assert_eq!(hex(&e.blob), s(get(x, "blob")), "{name} chunk {i} blob");
    }
    assert_eq!(m.original_digest.to_string(), s(get(exp, "original_digest")), "{name}: original digest");
    assert_eq!(hex(&m.manifest_digest), s(get(exp, "manifest_digest")), "{name}: manifest digest");
    assert_eq!(m.to_bytes().len() as u64, int(get(exp, "manifest_bytes")), "{name}: manifest length");
    if let Some(h) = obj(exp).get("manifest_hex") {
        assert_eq!(hex(&m.to_bytes()), s(h), "{name}: manifest bytes");
    }
}

/// Bit-identical columns (so -0.0 survives the round trip).
fn assert_same_cloud(a: &PointCloud, b: &PointCloud) {
    assert_eq!(a.frame(), b.frame());
    let bits = |v: &[f64]| v.iter().map(|x| x.to_bits()).collect::<Vec<_>>();
    assert_eq!(bits(a.x()), bits(b.x()));
    assert_eq!(bits(a.y()), bits(b.y()));
    assert_eq!(bits(a.z()), bits(b.z()));
    let (p, q) = (a.attributes(), b.attributes());
    match (&p.normal, &q.normal) {
        (Some(n), Some(m)) => (0..3).for_each(|k| assert_eq!(bits(&n[k]), bits(&m[k]))),
        (None, None) => {}
        _ => panic!("normal presence differs"),
    }
    // A NaN intensity comes back as the canonical NaN (pinned rule).
    let f32s = |v: &Option<Vec<f32>>| {
        v.as_ref().map(|v| v.iter().map(|x| fid_geo_ooc::intensity_bits(*x)).collect::<Vec<_>>())
    };
    assert_eq!(f32s(&p.intensity), f32s(&q.intensity));
    assert_eq!(p.colour, q.colour);
    assert_eq!(p.scanner_id, q.scanner_id);
    assert_eq!(p.timestamp_ns, q.timestamp_ns);
    assert_eq!(p.quality, q.quality);
    assert_eq!(p.grid, q.grid);
    assert_eq!(p.scan_origins.len(), q.scan_origins.len());
    for (x, y) in p.scan_origins.iter().zip(&q.scan_origins) {
        assert_eq!(x.0, y.0);
        assert_eq!(bits(&x.1), bits(&y.1));
    }
}

/// Streams `cloud` through the writer and returns its manifest.
fn write_through(cloud: &PointCloud, bounds: Aabb, max: u64, budget: u64) -> (ChunkManifest, fid_geo_ooc::WriterStats) {
    let ts = TestStore::new("writer");
    let a = cloud.attributes();
    let header = CloudHeader {
        frame: cloud.frame(),
        bounds,
        channels: fid_geo_ooc::ChannelSet::of(a),
        scan_origins: a.scan_origins.clone(),
        grid: a.grid,
    };
    let dir = temp_path("spill");
    let mut w =
        ChunkWriter::new(ts.store(), &dir, header, WriterConfig { max_chunk_points: max, spill_budget: budget })
            .unwrap();
    for i in 0..cloud.len() {
        let u = i as usize;
        let pa = PointAttrs {
            normal: a.normal.as_ref().map(|n| [n[0][u], n[1][u], n[2][u]]),
            intensity: a.intensity.as_ref().map(|v| v[u]),
            colour: a.colour.as_ref().map(|v| v[u]),
            scanner_id: a.scanner_id.as_ref().map(|v| v[u]),
            timestamp_ns: a.timestamp_ns.as_ref().map(|v| v[u]),
            quality: a.quality.as_ref().map(|v| v[u]),
        };
        w.push(cloud.point(i).unwrap(), &pa).unwrap();
    }
    let out = w.finish().unwrap();
    assert!(!dir.exists(), "spill directory removed");
    // The writer's chunks are in its store; read every one back.
    for e in &out.0.chunks {
        let bytes = ts.store().get_blob(&e.blob, e.bytes).unwrap();
        assert_eq!(decode_chunk(&bytes, e.bytes).unwrap().cell, e.cell);
    }
    out
}

#[test]
fn vectors_match_python_reference() {
    for c in cases() {
        let name = s(get(&c, "name")).to_string();
        let cloud = case_cloud(&c);
        let ts = TestStore::new("vec");
        let m = chunk_case(&c, &cloud, ts.store());
        let exp = get(&c, "expected");
        check_expected(&name, exp, &m);
        assert_eq!(m.original_digest, digest_point_cloud(&cloud), "{name}: GEO.TYPES digest");
        if let Some(hexes) = obj(exp).get("chunk_hex") {
            for (e, h) in m.chunks.iter().zip(arr(hexes)) {
                assert_eq!(hex(&ts.store().get_blob(&e.blob, e.bytes).unwrap()), s(h), "{name}: chunk bytes");
            }
        }
        m.validate().unwrap();
    }
}

#[test]
fn round_trip_is_lossless_and_rechunks_identically() {
    for c in cases() {
        let name = s(get(&c, "name")).to_string();
        let cloud = case_cloud(&c);
        let ts = TestStore::new("rt");
        let m = chunk_case(&c, &cloud, ts.store());
        // REQ-002: same element digest, and every column bit-identical (-0.0 included).
        let back = to_memory(&m, ts.store(), u64::MAX).unwrap();
        assert_eq!(digest_point_cloud(&back), m.original_digest, "{name}");
        assert_same_cloud(&cloud, &back);
        // chunk -> points -> chunk: identical manifest, hence identical chunk bytes.
        let ts2 = TestStore::new("rt2");
        let m2 = chunk_case(&c, &back, ts2.store());
        assert_eq!(m2, m, "{name}: re-chunked manifest");
        for e in &m.chunks {
            let a = ts.store().get_blob(&e.blob, e.bytes).unwrap();
            let b = ts2.store().get_blob(&e.blob, e.bytes).unwrap();
            assert_eq!(a, b, "{name}: chunk bytes");
            let decoded = decode_chunk(&a, e.bytes).unwrap();
            assert_eq!(fid_geo_ooc::encode_chunk(&decoded).unwrap(), a, "{name}: decode -> encode");
        }
        // Manifest bytes round trip through the store.
        let (addr, size) = put_manifest(ts.store(), &m).unwrap();
        assert_eq!(addr, m.manifest_digest, "{name}: a small manifest's address is its digest");
        assert_eq!(get_manifest(ts.store(), &addr, size).unwrap(), m);
    }
}

#[test]
fn writer_matches_in_memory_layout() {
    for c in cases() {
        let name = s(get(&c, "name")).to_string();
        let cloud = case_cloud(&c);
        let max = int(get(&c, "max_chunk_points"));
        let bounds = bounds_of(&c).unwrap_or_else(|| cloud.aabb().unwrap());
        let exp = get(&c, "expected");
        let budget = if name == "duplicates_object" { 64 << 20 } else { FIXED_OVERHEAD + (512 << 10) };
        let (m, stats) = write_through(&cloud, bounds, max, budget);
        check_expected(&name, exp, &m);
        assert_eq!(stats.points, cloud.len());
        assert_eq!(stats.chunks, m.chunks.len() as u64);
        if name == "clustered_20000" {
            assert!(stats.external_splits > 0, "the small budget forces streaming splits");
            assert!(stats.spilled_bytes > 512 << 10, "the cloud is larger than the buffer");
        }
    }
}

#[test]
fn partition_is_independent_of_input_order() {
    let c = case("seeded_2000");
    let cloud = case_cloud(&c);
    let n = cloud.len() as usize;
    // A fixed permutation: i -> i * 7 mod n (n = 2000 is coprime with 7).
    let perm: Vec<usize> = (0..n).map(|i| i * 7 % n).collect();
    let pts: Vec<[f64; 3]> = perm.iter().map(|&i| cloud.point(i as u64).unwrap()).collect();
    let shuffled = PointCloud::new(cloud.frame(), PointBuffer::from_points(&pts), AttributeSet::default()).unwrap();
    let plain = PointCloud::new(cloud.frame(), cloud.buffer().clone(), AttributeSet::default()).unwrap();
    let ts = TestStore::new("perm");
    let params = LayoutParams { max_chunk_points: 64 };
    let a = to_chunks(&plain, ts.store(), &params).unwrap();
    let b = to_chunks(&shuffled, ts.store(), &params).unwrap();
    let cells = |m: &ChunkManifest| m.chunks.iter().map(|e| (e.cell, e.count)).collect::<Vec<_>>();
    assert_eq!(cells(&a), cells(&b), "same cells and counts in any input order");
    assert_eq!(a.aabb, b.aabb);
    for (ea, eb) in a.chunks.iter().zip(&b.chunks) {
        let ca = decode_chunk(&ts.store().get_blob(&ea.blob, ea.bytes).unwrap(), ea.bytes).unwrap();
        let cb = decode_chunk(&ts.store().get_blob(&eb.blob, eb.bytes).unwrap(), eb.bytes).unwrap();
        let key = |c: &fid_geo_ooc::Chunk| {
            let mut v: Vec<[u64; 3]> = (0..c.index.len())
                .map(|j| [c.points.x[j].to_bits(), c.points.y[j].to_bits(), c.points.z[j].to_bits()])
                .collect();
            v.sort_unstable();
            v
        };
        assert_eq!(key(&ca), key(&cb), "same points per cell");
        // Original indexes map through the permutation.
        let mut mapped: Vec<u64> = cb.index.iter().map(|&j| perm[j as usize] as u64).collect();
        mapped.sort_unstable();
        assert_eq!(mapped, ca.index);
    }
    // Same cloud, different writer budgets: identical manifest.
    let bounds = plain.aabb().unwrap();
    let (m1, _) = write_through(&plain, bounds, 64, FIXED_OVERHEAD + (64 << 10));
    let (m2, _) = write_through(&plain, bounds, 64, 64 << 20);
    assert_eq!(m1.manifest_digest, a.manifest_digest);
    assert_eq!(m2.manifest_digest, a.manifest_digest);
}

#[test]
fn chunks_are_in_morton_order_and_respect_the_split_rule() {
    for c in cases() {
        let cloud = case_cloud(&c);
        let ts = TestStore::new("order");
        let m = chunk_case(&c, &cloud, ts.store());
        for w in m.chunks.windows(2) {
            assert!(w[0].cell.range_end() <= w[1].cell.range_start());
        }
        for e in &m.chunks {
            assert!(e.count <= m.max_chunk_points || e.cell.depth() == fid_geo_ooc::MAX_DEPTH);
            // The parent of every leaf held more than max_chunk_points (else it would be the leaf).
            if e.cell.depth() > 0 {
                let parent = CellCode::from_raw(e.cell.raw() >> 3).unwrap();
                let in_parent: u64 = m
                    .chunks
                    .iter()
                    .filter(|x| {
                        x.cell.range_start() >= parent.range_start() && x.cell.range_end() <= parent.range_end()
                    })
                    .map(|x| x.count)
                    .sum();
                assert!(in_parent > m.max_chunk_points);
            }
        }
    }
}

#[test]
fn duplicates_reach_the_depth_limit_and_use_object_addresses() {
    let c = case("duplicates_object");
    let cloud = case_cloud(&c);
    let ts = TestStore::new("dup");
    let m = chunk_case(&c, &cloud, ts.store());
    let big = m.chunks.iter().find(|e| e.count == 300_000).unwrap();
    assert_eq!(big.cell.depth(), fid_geo_ooc::MAX_DEPTH);
    assert!(big.bytes > fid_geo_ooc::STORE_CHUNK_SIZE);
    // The writer cannot hold that leaf within a small budget.
    let dir = temp_path("dup-spill");
    let header = CloudHeader {
        frame: cloud.frame(),
        bounds: cloud.aabb().unwrap(),
        channels: fid_geo_ooc::ChannelSet::EMPTY,
        scan_origins: Vec::new(),
        grid: None,
    };
    let mut w = ChunkWriter::new(
        ts.store(),
        &dir,
        header,
        WriterConfig { max_chunk_points: 1000, spill_budget: FIXED_OVERHEAD + (4 << 20) },
    )
    .unwrap();
    for i in 0..cloud.len() {
        w.push(cloud.point(i).unwrap(), &PointAttrs::default()).unwrap();
    }
    assert!(matches!(w.finish(), Err(fid_geo_ooc::OocError::BudgetExceeded { .. })));
    assert!(!dir.exists(), "spill directory removed on error");
}
