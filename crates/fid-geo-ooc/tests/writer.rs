//! ChunkWriter error paths, budget rules and spill-directory hygiene, and the store
//! address rule.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use common::*;
use fid_geo_ooc::writer::{ENTRY_COST, FIXED_OVERHEAD};
use fid_geo_ooc::{
    blob_address, Channel, ChannelSet, ChunkEntry, ChunkStore, ChunkWriter, CloudHeader, MemStore, OocError,
    PointAttrs, WriterConfig, STORE_CHUNK_SIZE,
};
use fid_geo_types::{Aabb, FrameRef, GeoTypeError};

fn header(channels: ChannelSet) -> CloudHeader {
    CloudHeader {
        frame: FrameRef::from_bytes([9; 16]),
        bounds: Aabb::new([0.0; 3], [1.0; 3]).unwrap(),
        channels,
        scan_origins: Vec::new(),
        grid: None,
    }
}

fn cfg() -> WriterConfig {
    WriterConfig { max_chunk_points: 64, spill_budget: FIXED_OVERHEAD + (256 << 10) }
}

#[test]
fn entry_cost_covers_the_struct() {
    assert!(ENTRY_COST >= core::mem::size_of::<ChunkEntry>() as u64);
}

#[test]
fn point_validation_errors_keep_the_writer_usable() {
    let store = MemStore::new();
    let dir = temp_path("val");
    let ch = ChannelSet::EMPTY.with(Channel::Normal).with(Channel::Intensity);
    let mut w = ChunkWriter::new(&store, &dir, header(ch), cfg()).unwrap();
    let good = PointAttrs { normal: Some([0.0, 0.0, 1.0]), intensity: Some(0.5), ..PointAttrs::default() };
    w.push([0.5; 3], &good).unwrap();
    assert_eq!(w.push([0.5; 3], &PointAttrs::default()), Err(OocError::ChannelMismatch { index: 1 }));
    let extra = PointAttrs { quality: Some(1), ..good };
    assert_eq!(w.push([0.5; 3], &extra), Err(OocError::ChannelMismatch { index: 1 }));
    assert_eq!(w.push([f64::NAN, 0.0, 0.0], &good), Err(OocError::Geometry(GeoTypeError::NonFinite { index: 1 })));
    let tilted = PointAttrs { normal: Some([0.0, 0.0, 1.0 + 1e-6]), ..good };
    assert_eq!(w.push([0.5; 3], &tilted), Err(OocError::Geometry(GeoTypeError::BadNormal { index: 1 })));
    let nan = PointAttrs { intensity: Some(f32::NAN), ..good };
    assert_eq!(w.push([0.5; 3], &nan), Err(OocError::NaNValue { channel: "intensity", index: 1 }));
    assert_eq!(w.push([1.5, 0.5, 0.5], &good), Err(OocError::OutOfBounds { index: 1 }));
    w.push([-0.0, 1.0, 0.0], &good).unwrap();
    let (m, stats) = w.finish().unwrap();
    assert_eq!((m.n_points, stats.points), (2, 2));
    assert!(!dir.exists());
}

#[test]
fn configuration_errors() {
    let store = MemStore::new();
    let dir = temp_path("cfg");
    let small = WriterConfig { max_chunk_points: 1 << 16, spill_budget: FIXED_OVERHEAD + (1 << 20) };
    match ChunkWriter::new(&store, &dir, header(ChannelSet::EMPTY), small) {
        Err(OocError::BudgetTooSmall { needed }) => {
            assert!(needed > small.spill_budget);
            let ok = WriterConfig { spill_budget: needed, ..small };
            let w = ChunkWriter::new(&store, &dir, header(ChannelSet::EMPTY), ok);
            assert!(w.is_ok(), "the reported minimum is enough");
        }
        other => panic!("{:?}", other.err()),
    }
    assert!(!dir.exists(), "dropped writer removed its directory");
    let zero = WriterConfig { max_chunk_points: 0, ..cfg() };
    assert!(matches!(
        ChunkWriter::new(&store, &dir, header(ChannelSet::EMPTY), zero),
        Err(OocError::InvalidParams { .. })
    ));
    let mut h = header(ChannelSet::EMPTY);
    h.scan_origins = vec![(2, [0.0; 3]), (1, [0.0; 3])];
    assert_eq!(
        ChunkWriter::new(&store, &dir, h, cfg()).err(),
        Some(OocError::Geometry(GeoTypeError::ScanOrigin { scanner_id: 1 }))
    );
    let mut h = header(ChannelSet::EMPTY);
    h.bounds = Aabb::new([-f64::MAX, 0.0, 0.0], [f64::MAX, 1.0, 1.0]).unwrap();
    assert_eq!(ChunkWriter::new(&store, &dir, h, cfg()).err(), Some(OocError::BoundsTooLarge { axis: 0 }));
    // The spill directory must not exist yet.
    std::fs::create_dir_all(&dir).unwrap();
    assert!(matches!(ChunkWriter::new(&store, &dir, header(ChannelSet::EMPTY), cfg()), Err(OocError::Spill { .. })));
    assert!(dir.exists(), "a directory the writer did not create is left alone");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn finish_errors() {
    let store = MemStore::new();
    let dir = temp_path("fin");
    let w = ChunkWriter::new(&store, &dir, header(ChannelSet::EMPTY), cfg()).unwrap();
    assert_eq!(w.finish().err(), Some(OocError::EmptyCloud));
    let mut h = header(ChannelSet::EMPTY);
    h.grid = Some((2, 2));
    let mut w = ChunkWriter::new(&store, &dir, h, cfg()).unwrap();
    for _ in 0..3 {
        w.push([0.25; 3], &PointAttrs::default()).unwrap();
    }
    assert_eq!(w.finish().err(), Some(OocError::Geometry(GeoTypeError::GridShape { rows: 2, cols: 2, count: 3 })));
    assert!(!dir.exists());
}

#[test]
fn too_many_chunks_for_the_entry_budget() {
    // max_chunk_points 1 over 2 000 distinct points needs 2 000 entries; the smallest
    // budget holds fewer.
    let store = MemStore::new();
    let dir = temp_path("entries");
    let c = WriterConfig { max_chunk_points: 1, spill_budget: FIXED_OVERHEAD + 32 * 1024 };
    let mut w = ChunkWriter::new(&store, &dir, header(ChannelSet::EMPTY), c).unwrap();
    for i in 0..2000 {
        let t = i as f64 / 2000.0;
        w.push([t, t, t], &PointAttrs::default()).unwrap();
    }
    assert!(matches!(w.finish(), Err(OocError::TooManyChunks { .. })));
    assert!(!dir.exists());
}

#[test]
fn store_addresses_follow_the_cas_rule() {
    let small = vec![7u8; 1000];
    let exact = vec![1u8; STORE_CHUNK_SIZE as usize];
    let big = vec![3u8; STORE_CHUNK_SIZE as usize + 5];
    let store = MemStore::new();
    for b in [&small, &exact, &big] {
        let a = store.put_blob(b).unwrap();
        assert_eq!(a, blob_address(b));
        assert_eq!(&store.get_blob(&a, b.len() as u64).unwrap(), b);
    }
    // The CORE.STO blob store gives the same addresses (feature `store`).
    #[cfg(feature = "store")]
    {
        assert_eq!(STORE_CHUNK_SIZE, fid_store::cas::CHUNK_SIZE);
        let ts = TestStore::new("cas");
        for b in [&small, &exact, &big] {
            let a = ts.store().put_blob(b).unwrap();
            assert_eq!(a, blob_address(b), "{} bytes", b.len());
            assert_eq!(fid_store::cas::address_of(b).unwrap().digest().to_bytes(), a);
            assert_eq!(&ts.store().get_blob(&a, b.len() as u64).unwrap(), b);
        }
    }
    // Missing and corrupt blobs are store errors.
    let a = blob_address(&small);
    assert!(matches!(MemStore::new().get_blob(&a, 1000), Err(OocError::Store { code: "not_found", .. })));
    store.replace(a, vec![8u8; 1000]);
    assert!(matches!(store.get_blob(&a, 1000), Err(OocError::Store { code: "integrity", .. })));
}
