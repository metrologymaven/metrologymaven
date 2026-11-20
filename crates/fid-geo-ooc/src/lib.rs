//! Out-of-core point clouds of Metrology Maven: chunk layout, manifest and conversion (spec
//! node `GEO.OOC.LAYOUT`). Outside the kernel graph because it does storage I/O; kernel code
//! reads chunked clouds through the GEO.OOC.STREAM source, never through this crate.
//!
//! A very large cloud is cut into the leaves of an octree over its bounding box. Each leaf
//! is one **chunk**, a versioned byte blob stored in the CORE.STO content-addressed store,
//! and a **manifest** names the whole chunked cloud. [`to_chunks`] and [`to_memory`]
//! convert in both directions; [`ChunkWriter`] builds the same chunks from points streamed
//! in original order with bounded memory, for files larger than memory. [`ChunkCache`]
//! (spec node `GEO.OOC.CACHE`) loads chunks on demand under a memory budget, verifying
//! each against its manifest digest.
//!
//! Everything below is pinned for format version 1 and reproduced by the independent
//! Python reference `fixtures/geo/ooc/make_layout_fixtures.py`.
//!
//! # Partition (D0)
//!
//! * **Bounds.** The partition quantises a bounding box `[min, max]`: the cloud's own box
//!   (IEEE 754 totalOrder min/max of the coordinates) for [`to_chunks`], or the bounds given
//!   to [`to_chunks_in`] or the writer, which must contain every point (numeric comparison,
//!   so `-0.0 == +0.0`). An extent `max - min` that overflows is refused.
//! * **Point to cell.** Per axis, `s = ((v - min) / (max - min)) * 2^21` (three IEEE 754
//!   operations, round to nearest even, no fused multiply-add); the quantised coordinate is
//!   0 if `!(s > 0)` (also for a zero extent), else `min(floor(s), 2^21 - 1)`. A point on
//!   an interior split plane therefore goes to the upper cell (half-open intervals), and a
//!   point on the maximum face to the last cell. `-0.0` and `+0.0` quantise the same.
//!   The 63-bit Morton code has bit `3b + k` = bit `b` of axis `k` (x 0, y 1, z 2).
//! * **Cells.** [`CellCode`] is `(1 << 3 depth) | prefix`; the prefix is the top
//!   `3 depth` bits of the Morton code; child octant bit 1 = upper x, 2 = upper y,
//!   4 = upper z. The depth limit is [`MAX_DEPTH`] = 21.
//! * **Split rule.** From the root, a cell holding more than `max_chunk_points` points
//!   (default 2^20, range 1..=2^32) is split into its eight children; empty children are
//!   dropped; a cell at depth 21 is never split (it may hold more than `max_chunk_points`
//!   points, e.g. duplicates). The leaves, in Morton order (ascending
//!   [`CellCode::range_start`]), are the chunks. The result depends only on the bounds,
//!   the parameter and the point coordinates, never on input or processing order.
//! * **Inside a chunk** points are ordered by original index (REQ-GEO.OOC.LAYOUT-001).
//!
//! # Chunk format v1
//!
//! All integers little-endian.
//!
//! ```text
//! "FIDCHK" | version u8 = 1 | cell u64 | count u64 (>= 1)
//! n_channels u8 | per channel, strictly ascending name: name_len u8 | name | type u8
//! index[count] u64 (strictly ascending) | x[count] f64 | y[count] f64 | z[count] f64
//! channel columns in list order:
//!   colour u8x3 (3) [r g b per point]   intensity f32 (2)   normal f64x3col (8) [all x, all y, all z]
//!   quality u16 (4)   scanner_id u32 (5)   timestamp_ns i64 (6)
//! ```
//!
//! Names and type bytes are those of GEO.TYPES.DIGEST. Floats are stored as their raw IEEE
//! 754 bits: the chunk is lossless, so `-0.0` stays `-0.0` (and makes a chunk, hence a
//! manifest digest, different from `+0.0`, although the element digest normalises it).
//! Coordinates and normals must be finite (no NaN, no infinity). Intensity may be
//! infinite or NaN, as in GEO.TYPES; any NaN intensity is stored as the canonical f32 NaN
//! `0x7fc00000` (the element digest's canonicalisation, so the round trip keeps the element
//! digest), and a reader refuses every other NaN bit pattern. [`decode_chunk`] checks, in order: the
//! input against its limit (`Oversized`), the tag, the version
//! ([`OocError::UnsupportedChunkVersion`]), the cell code, a non-zero count, the channel
//! list, the declared size against the limit (`Oversized`) and the bytes present
//! (`Truncated`, `TrailingBytes`), then the values. Nothing count-sized is allocated
//! before the declared size is known to fit the bytes present.
//!
//! # Manifest format v1
//!
//! ```text
//! "FIDCMF" | version u8 = 1 | frame [16] | min x y z f64 | max x y z f64
//! max_chunk_points u64 | max_depth u8 = 21 | n_points u64 | channel list (as in a chunk)
//! n_scan_origins u64 | per origin: id u32 | x y z f64
//! grid flag u8 (0 none, 1 present) [| rows u64 | cols u64]
//! original_digest [32] | n_chunks u64 | per chunk: cell u64 | count u64 | bytes u64 | blob [32]
//! ```
//!
//! The manifest digest is the SHA-256 of these bytes (REQ-GEO.OOC.LAYOUT-003): frame,
//! partition, cloud attributes and chunk contents (through the blob addresses). Stored as
//! a blob of at most 8 MiB, its store address equals the manifest digest. `blob` is the
//! chunk's CORE.STO.CAS address ([`blob_address`]); `bytes` its length.
//!
//! # Original digest
//!
//! `original_digest` is the GEO.TYPES.DIGEST element digest of the cloud.
//! [`to_memory`] rebuilds the cloud and checks it reproduces this digest
//! (REQ-GEO.OOC.LAYOUT-002); the writer computes it from spilled columns.

pub mod cache;
mod cell;
mod error;
mod format;
mod layout;
mod store;
pub mod writer;

pub use cache::{
    load_cost, resident_cost, CacheConfig, CacheStats, ChunkCache, ChunkHandle, Eviction, DEFAULT_CACHE_BUDGET,
    ENTRY_OVERHEAD,
};
pub use cell::{CellCode, Grid, GRID_CELLS, MAX_DEPTH};
pub use error::{Fault, OocError};
pub use format::{
    chunk_bytes, chunk_header_bytes, decode_chunk, encode_chunk, intensity_bits, validate_chunk, Channel, ChannelSet,
    Chunk, ChunkEntry, ChunkManifest, CANONICAL_NAN_F32, CHUNK_MAGIC, CHUNK_VERSION, ENTRY_BYTES, MANIFEST_MAGIC,
    MANIFEST_VERSION, MAX_CHUNK_POINTS_LIMIT, MAX_MANIFEST_CHUNKS,
};
pub use layout::{memory_needed, to_chunks, to_chunks_in, to_memory, LayoutParams, DEFAULT_MAX_CHUNK_POINTS};
pub use store::{blob_address, ChunkStore, MemStore, STORE_CHUNK_SIZE};
pub use writer::{ChunkWriter, CloudHeader, PointAttrs, WriterConfig, WriterStats, DEFAULT_SPILL_BUDGET};

/// Stores the manifest bytes as a blob and returns its address and length. For a manifest
/// of at most [`STORE_CHUNK_SIZE`] bytes the address is the manifest digest.
pub fn put_manifest(store: &dyn ChunkStore, m: &ChunkManifest) -> Result<([u8; 32], u64), OocError> {
    let bytes = m.to_bytes();
    let a = store.put_blob(&bytes)?;
    Ok((a, bytes.len() as u64))
}

/// Largest manifest [`get_manifest`] reads: [`MAX_MANIFEST_CHUNKS`] entries plus room for
/// the header, channel list and up to 2^16 scan origins.
pub const MAX_MANIFEST_BYTES: u64 = MAX_MANIFEST_CHUNKS * ENTRY_BYTES + (1 << 21);

/// Reads, verifies and validates the manifest stored under `address` (`size` bytes).
pub fn get_manifest(store: &dyn ChunkStore, address: &[u8; 32], size: u64) -> Result<ChunkManifest, OocError> {
    if size > MAX_MANIFEST_BYTES {
        return Err(OocError::CorruptManifest { fault: Fault::Oversized { needed: size, limit: MAX_MANIFEST_BYTES } });
    }
    let bytes = store.get_blob(address, size)?;
    ChunkManifest::from_bytes(&bytes, MAX_MANIFEST_BYTES)
}
