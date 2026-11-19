//! The chunk and manifest readers fail closed: unknown versions (AC-5), truncated,
//! oversized and trailing bytes, malformed headers and values, all as typed errors, and no
//! input makes them panic. The chunk bytes come from the Python reference vectors.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use common::*;
use fid_geo_ooc::{
    chunk_header_bytes, decode_chunk, encode_chunk, to_chunks, to_memory, CellCode, Channel, ChannelSet, Chunk,
    ChunkManifest, ChunkStore, Fault, LayoutParams, MemStore, OocError, CHUNK_VERSION,
};
use fid_geo_types::{AttributeSet, PointBuffer, PointCloud};

/// The first chunk of the reference case `tiny_all_channels` (every channel).
fn tiny_chunk() -> Vec<u8> {
    let v = json("layout_vectors.json");
    let c = arr(get(&v, "cases")).iter().find(|c| s(get(c, "name")) == "tiny_all_channels").unwrap().clone();
    bytes_hex(s(&arr(get(get(&c, "expected"), "chunk_hex"))[0]))
}

fn fault(r: Result<Chunk, OocError>) -> Fault {
    match r {
        Err(OocError::CorruptChunk { fault }) => fault,
        other => panic!("expected CorruptChunk, got {other:?}"),
    }
}

#[test]
fn ac5_unknown_version_is_refused() {
    let mut b = tiny_chunk();
    assert_eq!(b[6], CHUNK_VERSION);
    for v in [9u8, 0, 2, 255] {
        b[6] = v;
        assert_eq!(decode_chunk(&b, u64::MAX), Err(OocError::UnsupportedChunkVersion { found: v }));
    }
}

#[test]
fn reference_chunk_decodes_and_reencodes() {
    let b = tiny_chunk();
    let c = decode_chunk(&b, b.len() as u64).unwrap();
    assert_eq!(c.channels().iter().count(), 6);
    assert_eq!(encode_chunk(&c).unwrap(), b);
}

#[test]
fn truncated_trailing_and_oversized_bytes_are_typed() {
    let b = tiny_chunk();
    let n = b.len() as u64;
    for cut in 0..b.len() {
        let r = decode_chunk(&b[..cut], u64::MAX);
        match r {
            Err(OocError::CorruptChunk { fault: Fault::Truncated { found, .. } }) => assert_eq!(found, cut as u64),
            Err(OocError::CorruptChunk { fault: Fault::BadMagic }) => panic!("prefix of a valid tag at {cut}"),
            other => panic!("cut {cut}: {other:?}"),
        }
    }
    let mut longer = b.clone();
    longer.extend_from_slice(&[0, 0, 0]);
    assert_eq!(fault(decode_chunk(&longer, u64::MAX)), Fault::TrailingBytes { extra: 3 });
    assert_eq!(fault(decode_chunk(&b, n - 1)), Fault::Oversized { needed: n, limit: n - 1 });
    // A header declaring 2^40 points within a small limit: refused before allocating.
    let mut huge = b.clone();
    huge[15..23].copy_from_slice(&(1u64 << 40).to_le_bytes());
    match fault(decode_chunk(&huge, 1 << 20)) {
        Fault::Oversized { needed, limit } => assert!(needed > limit),
        other => panic!("{other:?}"),
    }
    // A count whose size overflows u64.
    huge[15..23].copy_from_slice(&u64::MAX.to_le_bytes());
    assert_eq!(fault(decode_chunk(&huge, u64::MAX)), Fault::Oversized { needed: u64::MAX, limit: u64::MAX });
    // An honest header with fewer bytes than it declares.
    huge[15..23].copy_from_slice(&1000u64.to_le_bytes());
    assert!(matches!(fault(decode_chunk(&huge, u64::MAX)), Fault::Truncated { .. }));
}

#[test]
fn malformed_headers_are_typed() {
    let b = tiny_chunk();
    let mut x = b.clone();
    x[0] = b'G';
    assert_eq!(fault(decode_chunk(&x, u64::MAX)), Fault::BadMagic);
    let mut x = b.clone();
    x[7..15].copy_from_slice(&2u64.to_le_bytes());
    assert_eq!(fault(decode_chunk(&x, u64::MAX)), Fault::BadCell { code: 2 });
    let mut x = b.clone();
    x[15..23].copy_from_slice(&0u64.to_le_bytes());
    assert_eq!(fault(decode_chunk(&x, u64::MAX)), Fault::ZeroCount);
    // Channel list: first name "colour" at offset 25; rename it, retype it, reorder.
    let mut x = b.clone();
    x[25] = b'k';
    assert_eq!(fault(decode_chunk(&x, u64::MAX)), Fault::BadChannel);
    let mut x = b.clone();
    x[31] = 9;
    assert_eq!(fault(decode_chunk(&x, u64::MAX)), Fault::BadChannel);
}

/// A one-channel chunk with intensity, built by hand.
fn small_chunk() -> Chunk {
    Chunk {
        cell: CellCode::ROOT,
        index: vec![3, 5, 9],
        points: PointBuffer::new(vec![0.0, -0.0, 1.0], vec![2.0, 3.0, 4.0], vec![5.0, 6.0, 7.0]),
        attrs: AttributeSet { intensity: Some(vec![0.5, -0.0, f32::INFINITY]), ..AttributeSet::default() },
    }
}

#[test]
fn floats_are_raw_bits_and_nan_is_refused() {
    let c = small_chunk();
    let b = encode_chunk(&c).unwrap();
    let back = decode_chunk(&b, u64::MAX).unwrap();
    assert_eq!(back.points.x[1].to_bits(), (-0.0f64).to_bits(), "-0.0 is kept");
    assert_eq!(back.attrs.intensity.as_ref().unwrap()[1].to_bits(), (-0.0f32).to_bits());
    assert_eq!(back, c);
    let h = chunk_header_bytes(ChannelSet::EMPTY.with(Channel::Intensity)) as usize;
    let x_at = h + 3 * 8;
    // NaN in x, refused by the reader and the writer.
    let mut nan = b.clone();
    nan[x_at + 8..x_at + 16].copy_from_slice(&f64::NAN.to_bits().to_le_bytes());
    assert_eq!(fault(decode_chunk(&nan, u64::MAX)), Fault::NaN { column: "x", at: 1 });
    let mut bad = c.clone();
    bad.points.z[2] = f64::NAN;
    assert_eq!(encode_chunk(&bad), Err(OocError::CorruptChunk { fault: Fault::NaN { column: "z", at: 2 } }));
    // Infinite coordinate.
    let mut inf = b.clone();
    inf[x_at..x_at + 8].copy_from_slice(&f64::NEG_INFINITY.to_bits().to_le_bytes());
    assert_eq!(fault(decode_chunk(&inf, u64::MAX)), Fault::NonFinite { column: "x", at: 0 });
    // NaN intensity (a signalling payload too).
    let i_at = h + 3 * 8 + 3 * 24;
    let mut ni = b.clone();
    ni[i_at + 4..i_at + 8].copy_from_slice(&0x7f80_0001u32.to_le_bytes());
    assert_eq!(fault(decode_chunk(&ni, u64::MAX)), Fault::NaN { column: "intensity", at: 1 });
    // Index order.
    let mut io = b.clone();
    io[h + 8..h + 16].copy_from_slice(&3u64.to_le_bytes());
    assert_eq!(fault(decode_chunk(&io, u64::MAX)), Fault::IndexOrder { at: 1 });
}

#[test]
fn encoder_refuses_invalid_chunks() {
    let mut c = small_chunk();
    c.index.clear();
    c.points = PointBuffer::default();
    c.attrs = AttributeSet::default();
    assert_eq!(encode_chunk(&c), Err(OocError::CorruptChunk { fault: Fault::ZeroCount }));
    let mut c = small_chunk();
    c.points.y.pop();
    assert_eq!(encode_chunk(&c), Err(OocError::CorruptChunk { fault: Fault::ColumnLength { column: "y" } }));
    let mut c = small_chunk();
    c.attrs.grid = Some((1, 3));
    assert_eq!(encode_chunk(&c), Err(OocError::CorruptChunk { fault: Fault::CloudAttributeInChunk }));
}

#[test]
fn no_single_byte_corruption_panics() {
    let b = tiny_chunk();
    for i in 0..b.len() {
        for x in [0x01u8, 0x80, 0xff] {
            let mut c = b.clone();
            if let Some(byte) = c.get_mut(i) {
                *byte ^= x;
            }
            let _ = decode_chunk(&c, u64::MAX);
        }
    }
}

fn sample_manifest(store: &MemStore) -> (PointCloud, ChunkManifest) {
    let pts: Vec<[f64; 3]> = (0..200).map(|i| [i as f64, (i * 7 % 13) as f64, -(i as f64)]).collect();
    let cloud = PointCloud::from_points(fid_geo_types::FrameRef::from_bytes([3; 16]), &pts).unwrap();
    let m = to_chunks(&cloud, store, &LayoutParams { max_chunk_points: 16 }).unwrap();
    (cloud, m)
}

#[test]
fn manifest_reader_fails_closed() {
    let store = MemStore::new();
    let (_, m) = sample_manifest(&store);
    let b = m.to_bytes();
    assert_eq!(ChunkManifest::from_bytes(&b, u64::MAX).unwrap(), m);
    let mut v = b.clone();
    v[6] = 9;
    assert_eq!(ChunkManifest::from_bytes(&v, u64::MAX), Err(OocError::UnsupportedManifestVersion { found: 9 }));
    for cut in [0, 5, 7, 40, b.len() / 2, b.len() - 1] {
        assert!(
            matches!(
                ChunkManifest::from_bytes(&b[..cut], u64::MAX),
                Err(OocError::CorruptManifest { fault: Fault::Truncated { .. } })
            ),
            "cut {cut}"
        );
    }
    let mut t = b.clone();
    t.push(0);
    assert_eq!(
        ChunkManifest::from_bytes(&t, u64::MAX),
        Err(OocError::CorruptManifest { fault: Fault::TrailingBytes { extra: 1 } })
    );
    assert!(matches!(
        ChunkManifest::from_bytes(&b, 10),
        Err(OocError::CorruptManifest { fault: Fault::Oversized { .. } })
    ));
    for i in 0..b.len() {
        let mut c = b.clone();
        if let Some(byte) = c.get_mut(i) {
            *byte ^= 0x5a;
        }
        let _ = ChunkManifest::from_bytes(&c, u64::MAX);
    }
}

#[test]
fn manifest_validation_catches_inconsistent_fields() {
    let store = MemStore::new();
    let (_, m) = sample_manifest(&store);
    let mut x = m.clone();
    x.n_points += 1;
    assert_eq!(x.validate(), Err(OocError::CorruptManifest { fault: Fault::CountMismatch }));
    let mut x = m.clone();
    x.manifest_digest[0] ^= 1;
    assert_eq!(x.validate(), Err(OocError::CorruptManifest { fault: Fault::DigestMismatch }));
    let mut x = m.clone();
    x.chunks.swap(0, 1);
    x.seal();
    assert_eq!(x.validate(), Err(OocError::CorruptManifest { fault: Fault::ChunkOrder { at: 1 } }));
    let mut x = m.clone();
    x.max_chunk_points = 0;
    x.seal();
    assert_eq!(x.validate(), Err(OocError::CorruptManifest { fault: Fault::BadParams }));
    let mut x = m.clone();
    x.chunks[0].bytes += 1;
    x.seal();
    assert!(matches!(x.validate(), Err(OocError::CorruptManifest { fault: Fault::WrongSize { .. } })));
    let mut x = m.clone();
    x.grid = Some((3, 3));
    x.seal();
    assert_eq!(x.validate(), Err(OocError::CorruptManifest { fault: Fault::BadGrid }));
}

#[test]
fn to_memory_checks_chunks_against_the_manifest() {
    let store = MemStore::new();
    let (cloud, m) = sample_manifest(&store);
    assert_eq!(to_memory(&m, &store, u64::MAX).unwrap(), cloud);
    // A stored chunk replaced by a different valid chunk under the same address: the
    // store's integrity check catches it.
    let e0 = m.chunks[0];
    let e1 = m.chunks[1];
    let other = store.get_blob(&e1.blob, e1.bytes).unwrap();
    store.replace(e0.blob, other.clone());
    assert!(matches!(to_memory(&m, &store, u64::MAX), Err(OocError::Store { code: "integrity", .. })));
    // A manifest that points chunk 0 at chunk 1's blob: the cell check catches it.
    let store = MemStore::new();
    let (_, m) = sample_manifest(&store);
    let mut x = m.clone();
    if x.chunks[0].bytes == x.chunks[1].bytes {
        x.chunks[0].blob = x.chunks[1].blob;
        x.seal();
        assert!(matches!(
            to_memory(&x, &store, u64::MAX),
            Err(OocError::CorruptChunk { fault: Fault::WrongCell { .. } })
        ));
    }
    // A wrong original digest.
    let mut x = m.clone();
    x.original_digest.0[0] ^= 1;
    x.seal();
    assert_eq!(to_memory(&x, &store, u64::MAX), Err(OocError::DigestMismatch));
}

#[test]
fn layout_params_and_inputs_are_checked() {
    let store = MemStore::new();
    let cloud = PointCloud::from_points(fid_geo_types::FrameRef::from_bytes([0; 16]), &[[0.0; 3]]).unwrap();
    for bad in [0, (1u64 << 32) + 1] {
        assert_eq!(
            to_chunks(&cloud, &store, &LayoutParams { max_chunk_points: bad }),
            Err(OocError::InvalidParams { what: "max_chunk_points" })
        );
    }
    let empty = PointCloud::from_points(fid_geo_types::FrameRef::from_bytes([0; 16]), &[]).unwrap();
    assert_eq!(to_chunks(&empty, &store, &LayoutParams::default()), Err(OocError::EmptyCloud));
    let attrs = AttributeSet { intensity: Some(vec![1.0, f32::NAN]), ..AttributeSet::default() };
    let nan = PointCloud::new(
        fid_geo_types::FrameRef::from_bytes([0; 16]),
        PointBuffer::from_points(&[[0.0; 3], [1.0; 3]]),
        attrs,
    )
    .unwrap();
    assert_eq!(
        to_chunks(&nan, &store, &LayoutParams::default()),
        Err(OocError::NaNValue { channel: "intensity", index: 1 })
    );
    let wide = PointCloud::from_points(
        fid_geo_types::FrameRef::from_bytes([0; 16]),
        &[[-f64::MAX, 0.0, 0.0], [f64::MAX, 0.0, 0.0]],
    )
    .unwrap();
    assert_eq!(to_chunks(&wide, &store, &LayoutParams::default()), Err(OocError::BoundsTooLarge { axis: 0 }));
    let small = fid_geo_types::Aabb::new([0.0; 3], [0.5; 3]).unwrap();
    let two = PointCloud::from_points(fid_geo_types::FrameRef::from_bytes([0; 16]), &[[0.0; 3], [1.0; 3]]).unwrap();
    assert_eq!(
        fid_geo_ooc::to_chunks_in(&two, &small, &store, &LayoutParams::default()),
        Err(OocError::OutOfBounds { index: 1 })
    );
}
