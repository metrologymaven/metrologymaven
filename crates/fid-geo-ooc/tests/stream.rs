//! GEO.OOC.STREAM on the small clouds of the layout vectors, and its refusals.
//!
//! Expected sequences come from the independent Python reference
//! `fixtures/geo/ooc/make_stream_fixtures.py` (`stream_vectors.json`); the cloud_5e6
//! acceptance tests (AC-1 to AC-3) are in `tests/cloud_5e6.rs`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use common::*;
use core::sync::atomic::{AtomicBool, Ordering};
use fid_canon::CValue;
use fid_geo_ooc::{
    encode_chunk, load_cost, original_digest, to_chunks, to_chunks_in, verify_original_digest, CellCode, ChannelSet,
    Chunk, ChunkCache, ChunkEntry, ChunkManifest, ChunkStore, ChunkStream, Fault, LayoutParams, MemStore, OocError,
    StreamConfig, StreamOrder, MAX_BLOCK_POINTS, MAX_FAN_IN,
};
use fid_geo_types::digest::digest_point_cloud;
use fid_geo_types::{Aabb, AttributeSet, ElementDigest, FrameRef, GeoTypeError, PointBuffer, PointSource};

fn layout_case(name: &str) -> CValue {
    let v = json("layout_vectors.json");
    arr(get(&v, "cases")).iter().find(|c| s(get(c, "name")) == name).unwrap().clone()
}

fn bounds_of(c: &CValue) -> Option<Aabb> {
    let b = get(c, "bounds");
    if is_null(b) {
        return None;
    }
    let (lo, hi) = (arr(get(b, "min")), arr(get(b, "max")));
    Some(
        Aabb::new(
            [f64_hex(&lo[0]), f64_hex(&lo[1]), f64_hex(&lo[2])],
            [f64_hex(&hi[0]), f64_hex(&hi[1]), f64_hex(&hi[2])],
        )
        .unwrap(),
    )
}

/// The chunked cloud of layout vector `name`, checked against the vector's manifest digest
/// (so the stream vectors and the layout vectors describe the same chunks).
fn chunked(name: &str, store: &dyn ChunkStore) -> ChunkManifest {
    let c = layout_case(name);
    let cloud = case_cloud(&c);
    let params = LayoutParams { max_chunk_points: int(get(&c, "max_chunk_points")) };
    let m = match bounds_of(&c) {
        Some(b) => to_chunks_in(&cloud, &b, store, &params).unwrap(),
        None => to_chunks(&cloud, store, &params).unwrap(),
    };
    assert_eq!(hex(&m.manifest_digest), s(get(get(&c, "expected"), "manifest_digest")), "{name}");
    assert_eq!(m.original_digest, digest_point_cloud(&cloud));
    m
}

fn cfg(order: StreamOrder, block: usize, fan_in: usize, channels: ChannelSet) -> StreamConfig {
    StreamConfig { order, block_points: block, channels, fan_in }
}

/// REQ-001, -002, -003, -005 on every vector: both orders, several block sizes and fan-ins,
/// equal to the reference; the original-order element digest equals the manifest's.
#[test]
fn vectors_both_orders_any_block_size() {
    let v = json("stream_vectors.json");
    for case in arr(get(&v, "cases")) {
        let name = s(get(case, "name"));
        let ts = TestStore::new("stream-vec");
        let m = chunked(name, ts.store());
        let n = int(get(case, "n_points"));
        assert_eq!(m.n_points, n);
        assert_eq!(m.chunks.len() as u64, int(get(case, "n_chunks")), "{name}");
        let cache = ChunkCache::new(1 << 30);
        let all = m.channels;
        let mut spatial = None;
        for block in [1usize, 3, 7, 1000, 65_536] {
            let (idx, d, lens) = stream_all(&m, ts.store(), &cache, cfg(StreamOrder::Spatial, block, 8, all));
            assert_eq!(d, s(get(case, "spatial_digest")), "{name} spatial, block {block}");
            assert!(lens.iter().rev().skip(1).all(|&l| l == block), "{name}: only the last block is short");
            let mut sorted = idx.clone();
            sorted.sort_unstable();
            assert_eq!(sorted, (0..n).collect::<Vec<_>>(), "{name}: every index once");
            if let Some(o) = obj(case).get("spatial_order") {
                let want: Vec<u64> = arr(o).iter().map(int).collect();
                assert_eq!(idx, want, "{name} spatial order");
            }
            if let Some(prev) = &spatial {
                assert_eq!(&idx, prev, "{name}: block size changes nothing");
            }
            spatial = Some(idx);
            for fan_in in [1usize, 2, 8] {
                let (idx, d, _) = stream_all(&m, ts.store(), &cache, cfg(StreamOrder::Original, block, fan_in, all));
                assert_eq!(d, s(get(case, "original_digest")), "{name} original, block {block}, fan-in {fan_in}");
                assert_eq!(idx, (0..n).collect::<Vec<_>>());
            }
        }
        let c = cfg(StreamOrder::Original, 5, 2, ChannelSet::EMPTY);
        assert_eq!(original_digest(&m, ts.store(), &cache, None, c).unwrap(), m.original_digest, "{name}");
        verify_original_digest(&m, ts.store(), &cache, None, c).unwrap();
        assert_eq!(cache.stats().pinned_chunks, 0, "{name}: finished streams pin nothing");
    }
}

fn sink(src: &mut impl PointSource) -> (Vec<u64>, Vec<String>, Result<(), GeoTypeError>) {
    let (mut idx, mut names) = (Vec::new(), Vec::new());
    while let Some(b) = src.next_block() {
        match b {
            Ok(b) => {
                for c in &b.channels {
                    assert_eq!(c.values.len(), b.x.len());
                }
                names = b.channels.iter().map(|c| c.name.clone()).collect();
                idx.extend_from_slice(&b.original_index);
            }
            Err(e) => return (idx, names, Err(e)),
        }
    }
    (idx, names, Ok(()))
}

/// The `PointSource` view: length, frame, channel columns and the same sequence.
#[test]
fn point_source_view() {
    let store = MemStore::new();
    let m = chunked("tiny_all_channels", &store);
    let cache = ChunkCache::new(1 << 24);
    let mut st = ChunkStream::new(&m, &store, &cache, None, cfg(StreamOrder::Spatial, 5, 8, m.channels)).unwrap();
    assert_eq!(PointSource::len(&st), 12);
    assert!(!st.is_empty());
    assert_eq!(st.frame(), m.frame);
    let (idx, names, r) = sink(&mut st);
    r.unwrap();
    let v = json("stream_vectors.json");
    let case = arr(get(&v, "cases")).iter().find(|c| s(get(c, "name")) == "tiny_all_channels").unwrap();
    assert_eq!(idx, arr(get(case, "spatial_order")).iter().map(int).collect::<Vec<_>>());
    let want = [
        "colour_r",
        "colour_g",
        "colour_b",
        "intensity",
        "normal_x",
        "normal_y",
        "normal_z",
        "quality",
        "scanner_id",
        "timestamp_ns",
    ];
    assert_eq!(names, want);

    // Values equal the typed columns.
    let mut a = ChunkStream::new(&m, &store, &cache, None, cfg(StreamOrder::Original, 12, 8, m.channels)).unwrap();
    let mut b = ChunkStream::new(&m, &store, &cache, None, cfg(StreamOrder::Original, 12, 8, m.channels)).unwrap();
    let t = a.next_stream_block().unwrap().unwrap();
    let p = b.next_block().unwrap().unwrap();
    assert_eq!(
        p.x.iter().map(|v| v.to_bits()).collect::<Vec<_>>(),
        t.x.iter().map(|v| v.to_bits()).collect::<Vec<_>>()
    );
    let col = |name: &str| p.channels.iter().find(|c| c.name == name).unwrap().values.clone();
    let ta = &t.attrs;
    assert_eq!(col("colour_g"), ta.colour.as_ref().unwrap().iter().map(|c| f64::from(c[1])).collect::<Vec<_>>());
    let inten: Vec<u64> = col("intensity").iter().map(|v| v.to_bits()).collect();
    let want: Vec<u64> = ta.intensity.as_ref().unwrap().iter().map(|v| f64::from(*v).to_bits()).collect();
    assert_eq!(inten, want);
    assert_eq!(col("normal_z"), ta.normal.as_ref().unwrap()[2]);
    assert_eq!(col("timestamp_ns"), ta.timestamp_ns.as_ref().unwrap().iter().map(|t| *t as f64).collect::<Vec<_>>());
    assert!(b.next_block().is_none());

    // Without requested channels a block carries coordinates only.
    let mut st =
        ChunkStream::new(&m, &store, &cache, None, cfg(StreamOrder::Spatial, 100, 8, ChannelSet::EMPTY)).unwrap();
    let blk = st.next_block().unwrap().unwrap();
    assert!(blk.channels.is_empty());
    assert_eq!(blk.original_index.len(), 12);
}

/// Cancellation: checked before every block; the stream then stays failed and returns no
/// partial block; digests return no value.
#[test]
fn cancellation_is_sticky_and_returns_nothing_partial() {
    let store = MemStore::new();
    let m = chunked("seeded_2000", &store);
    let cache = ChunkCache::new(1 << 26);
    for order in [StreamOrder::Spatial, StreamOrder::Original] {
        let flag = AtomicBool::new(true);
        let mut s = ChunkStream::new(&m, &store, &cache, Some(&flag), cfg(order, 100, 4, m.channels)).unwrap();
        assert_eq!(s.next_stream_block(), Some(Err(OocError::Cancelled)));
        flag.store(false, Ordering::Release);
        assert_eq!(s.next_stream_block(), Some(Err(OocError::Cancelled)), "sticky");
        assert_eq!(s.emitted(), 0);

        let flag = AtomicBool::new(false);
        let mut s = ChunkStream::new(&m, &store, &cache, Some(&flag), cfg(order, 100, 4, m.channels)).unwrap();
        assert_eq!(s.next_stream_block().unwrap().unwrap().len(), 100);
        flag.store(true, Ordering::Release);
        assert_eq!(s.next_stream_block(), Some(Err(OocError::Cancelled)));
        assert_eq!(s.emitted(), 100);
        assert!(matches!(s.next_block(), Some(Err(GeoTypeError::Cancelled))));
        assert_eq!(cache.stats().pinned_chunks, 0, "a failed stream releases its pins");

        let c = cfg(order, 100, 4, ChannelSet::EMPTY);
        assert_eq!(original_digest(&m, &store, &cache, Some(&flag), c), Err(OocError::Cancelled));
    }
}

#[test]
fn configuration_refusals() {
    let store = MemStore::new();
    let m = chunked("boundaries", &store);
    let cache = ChunkCache::new(1 << 20);
    let bad = |c: StreamConfig| ChunkStream::new(&m, &store, &cache, None, c).unwrap_err();
    let ok = StreamConfig::default();
    assert_eq!(bad(StreamConfig { block_points: 0, ..ok }), OocError::InvalidParams { what: "block_points" });
    assert_eq!(
        bad(StreamConfig { block_points: MAX_BLOCK_POINTS + 1, ..ok }),
        OocError::InvalidParams { what: "block_points" }
    );
    assert_eq!(bad(StreamConfig { fan_in: 0, ..ok }), OocError::InvalidParams { what: "fan_in" });
    assert_eq!(bad(StreamConfig { fan_in: MAX_FAN_IN + 1, ..ok }), OocError::InvalidParams { what: "fan_in" });
    let normal = ChannelSet::EMPTY.with(fid_geo_ooc::Channel::Normal);
    assert_eq!(bad(StreamConfig { channels: normal, ..ok }), OocError::InvalidParams { what: "channels" });
    // A manifest that fails validation (here its digest) is refused before any I/O.
    let mut forged = m.clone();
    forged.manifest_digest[0] ^= 1;
    assert_eq!(
        ChunkStream::new(&forged, &store, &cache, None, ok).unwrap_err(),
        OocError::CorruptManifest { fault: Fault::DigestMismatch }
    );
    assert_eq!(StreamConfig::new(StreamOrder::Original).order, StreamOrder::Original);
}

/// A hand-built chunked cloud over [0, 1]^3 with `max_chunk_points = 2`: chunks at the
/// depth-1 octants given, each with (index, point) pairs.
fn forged(store: &MemStore, n_points: u64, chunks: &[(u8, &[(u64, [f64; 3])])]) -> ChunkManifest {
    let mut entries = Vec::new();
    for (octant, pts) in chunks {
        let cell = CellCode::ROOT.child(*octant).unwrap();
        let p: Vec<[f64; 3]> = pts.iter().map(|(_, p)| *p).collect();
        let c = Chunk {
            cell,
            index: pts.iter().map(|(i, _)| *i).collect(),
            points: PointBuffer::from_points(&p),
            attrs: AttributeSet::default(),
        };
        let bytes = encode_chunk(&c).unwrap();
        let blob = store.put_blob(&bytes).unwrap();
        entries.push(ChunkEntry { cell, count: pts.len() as u64, bytes: bytes.len() as u64, blob });
    }
    let mut m = ChunkManifest {
        frame: FrameRef::from_bytes([7; 16]),
        aabb: Aabb::new([0.0; 3], [1.0; 3]).unwrap(),
        max_chunk_points: 2,
        n_points,
        channels: ChannelSet::EMPTY,
        scan_origins: Vec::new(),
        grid: None,
        chunks: entries,
        original_digest: ElementDigest([0; 32]),
        manifest_digest: [0; 32],
    };
    m.seal();
    m.validate().unwrap();
    m
}

fn first_error(m: &ChunkManifest, store: &MemStore, order: StreamOrder) -> OocError {
    let cache = ChunkCache::new(1 << 20);
    let mut s = ChunkStream::new(m, store, &cache, None, cfg(order, 1, 4, ChannelSet::EMPTY)).unwrap();
    loop {
        match s.next_stream_block() {
            Some(Ok(_)) => {}
            Some(Err(e)) => {
                assert_eq!(s.next_stream_block(), Some(Err(e.clone())), "sticky");
                return e;
            }
            None => panic!("no error"),
        }
    }
}

const LO: [f64; 3] = [0.1, 0.1, 0.1];
const HI: [f64; 3] = [0.9, 0.9, 0.9];

/// Forged manifests that validate but break the index partition (REQ-001).
#[test]
fn forged_partitions_are_refused() {
    let store = MemStore::new();
    // Index 0 in two chunks: the original-order merge finds the duplicate.
    let m = forged(&store, 2, &[(0, &[(0, LO)]), (7, &[(0, HI)])]);
    let e = first_error(&m, &store, StreamOrder::Original);
    assert_eq!(e, OocError::CorruptChunk { fault: Fault::DuplicateIndex { index: 0 } });
    let cache = ChunkCache::new(1 << 20);
    let c = cfg(StreamOrder::Original, 64, 4, ChannelSet::EMPTY);
    assert_eq!(original_digest(&m, &store, &cache, None, c), Err(e));

    // Indexes 0, 2, 2 of 3 points: index 1 is missing.
    let m = forged(&store, 3, &[(0, &[(0, LO), (2, [0.2, 0.2, 0.2])]), (7, &[(2, HI)])]);
    let e = first_error(&m, &store, StreamOrder::Original);
    assert_eq!(e, OocError::CorruptChunk { fault: Fault::MissingIndex { index: 1 } });

    // An index past the point count, in both orders.
    let m = forged(&store, 2, &[(0, &[(0, LO)]), (7, &[(5, HI)])]);
    for order in [StreamOrder::Spatial, StreamOrder::Original] {
        assert_eq!(
            first_error(&m, &store, order),
            OocError::CorruptChunk { fault: Fault::IndexOutOfRange { index: 5 } }
        );
    }

    // A point outside its chunk's cell, in both orders.
    let m = forged(&store, 2, &[(0, &[(0, LO)]), (7, &[(1, LO)])]);
    for order in [StreamOrder::Spatial, StreamOrder::Original] {
        assert_eq!(
            first_error(&m, &store, order),
            OocError::CorruptChunk { fault: Fault::PointOutsideCell { index: 1 } }
        );
    }

    // A valid forged cloud streams in both orders.
    let m = forged(&store, 3, &[(0, &[(0, LO), (2, [0.2, 0.2, 0.2])]), (7, &[(1, HI)])]);
    let cache = ChunkCache::new(1 << 20);
    let (idx, _, _) = stream_all(&m, &store, &cache, cfg(StreamOrder::Spatial, 2, 1, ChannelSet::EMPTY));
    assert_eq!(idx, [0, 2, 1]);
    let (idx, _, _) = stream_all(&m, &store, &cache, cfg(StreamOrder::Original, 2, 1, ChannelSet::EMPTY));
    assert_eq!(idx, [0, 1, 2]);
}

/// Store errors and a chunk whose bytes do not match the manifest surface as errors of the
/// stream, and through `PointSource` as `GeoTypeError::Source` with the stream's code.
#[test]
fn store_and_integrity_errors_propagate() {
    let store = MemStore::new();
    let m = chunked("seeded_2000", &store);
    let cache = ChunkCache::new(1 << 26);
    let victim = m.chunks[3];
    store.replace(victim.blob, vec![0u8; victim.bytes as usize]);
    for order in [StreamOrder::Spatial, StreamOrder::Original] {
        let mut s = ChunkStream::new(&m, &store, &cache, None, cfg(order, 4096, 8, m.channels)).unwrap();
        let e = loop {
            match s.next_block() {
                Some(Ok(_)) => {}
                Some(Err(e)) => break e,
                None => panic!("no error"),
            }
        };
        match &e {
            GeoTypeError::Source { code, .. } => assert_eq!(*code, "chunk_integrity"),
            other => panic!("{other:?}"),
        }
        assert_eq!(e.code(), "source");
        assert!(matches!(s.next_block(), Some(Err(GeoTypeError::Source { .. }))), "sticky");
    }
    let empty = MemStore::new();
    let mut s = ChunkStream::new(&m, &empty, &cache, None, cfg(StreamOrder::Spatial, 10, 8, m.channels)).unwrap();
    // Chunk 0 may still be resident from the runs above; stream until a missing one.
    let e = loop {
        match s.next_stream_block() {
            Some(Ok(_)) => {}
            Some(Err(e)) => break e,
            None => panic!("no error"),
        }
    };
    assert!(matches!(e, OocError::Store { code: "not_found", .. } | OocError::ChunkIntegrity { .. }), "{e:?}");
}

/// A cache that holds only one chunk at a time still streams original order with a fan-in
/// above one: a pin that does not fit drops the held pins and retries.
#[test]
fn tiny_cache_budget_same_sequence() {
    let store = MemStore::new();
    let m = chunked("seeded_2000", &store);
    let budget = m.chunks.iter().map(|e| load_cost(m.channels, e).unwrap()).max().unwrap();
    let v = json("stream_vectors.json");
    let case = arr(get(&v, "cases")).iter().find(|c| s(get(c, "name")) == "seeded_2000").unwrap();
    for (order, key) in [(StreamOrder::Original, "original_digest"), (StreamOrder::Spatial, "spatial_digest")] {
        let cache = ChunkCache::new(budget);
        let (_, d, _) = stream_all(&m, &store, &cache, cfg(order, 333, 8, m.channels));
        assert_eq!(d, s(get(case, key)));
        let st = cache.stats();
        assert!(st.peak_bytes <= budget);
        assert_eq!(st.pinned_chunks, 0);
    }
}

/// Timestamps beyond +-2^53 have no exact f64: the `PointSource` view refuses them, the
/// typed view carries them.
#[test]
fn inexact_timestamps_are_refused_by_the_point_source_view() {
    let store = MemStore::new();
    let m = chunked("seeded_2000", &store);
    let cache = ChunkCache::new(1 << 26);
    let ts = ChannelSet::EMPTY.with(fid_geo_ooc::Channel::TimestampNs);
    let mut s = ChunkStream::new(&m, &store, &cache, None, cfg(StreamOrder::Original, 2000, 8, ts)).unwrap();
    let e = s.next_point_block().unwrap().unwrap_err();
    assert!(matches!(e, OocError::InexactChannel { channel: "timestamp_ns", .. }), "{e:?}");
    assert_eq!(s.next_point_block(), Some(Err(e)), "sticky");
    let (idx, _, _) = stream_all(&m, &store, &cache, cfg(StreamOrder::Original, 2000, 8, ts));
    assert_eq!(idx.len(), 2000);
}
