//! Ordered full-resolution point streaming over chunked clouds (spec node
//! `GEO.OOC.STREAM`). [`ChunkStream`] implements the `PointSource` trait of `fid-geo-types`,
//! so kernel code reads a chunked cloud without doing I/O and without depending on this
//! crate.
//!
//! # Orders (D0; the sequence depends only on the manifest)
//!
//! * **Spatial** ([`StreamOrder::Spatial`]): the chunks in manifest order, which
//!   [`ChunkManifest::validate`] guarantees is ascending Morton order of their cells, and the
//!   points of each chunk by ascending original index (the chunk format's order)
//!   (REQ-GEO.OOC.STREAM-002). At most one chunk is pinned at a time.
//! * **Original** ([`StreamOrder::Original`]): ascending original index
//!   (REQ-GEO.OOC.STREAM-003), by a k-way merge over one cursor per chunk. A min-heap holds
//!   `(head index, chunk position)` per unfinished chunk; the smallest is popped and its
//!   chunk emits the run of points whose indexes stay at or below the next head. At most
//!   `fan_in` chunks are pinned at once: when another is needed, the least recently used
//!   pin is dropped and the chunk is fetched again through the [`ChunkCache`] (its budget
//!   bounds the memory; a re-fetch is verified like any load). If the budget cannot hold
//!   `fan_in` chunks, pins are dropped until the load fits. When the budget holds far fewer
//!   chunks than the cloud has, and the cloud's original order jumps between chunks, the
//!   merge re-fetches chunks often: memory stays bounded, time grows. The output does not depend
//!   on which chunks are pinned or resident.
//!
//! Blocks hold exactly `block_points` points, except the last, and may span chunks, so the
//! sequence of points does not depend on the block size (REQ-GEO.OOC.STREAM-005).
//!
//! # Checks (fail closed)
//!
//! The constructor validates the manifest ([`ChunkManifest::validate`], digest included)
//! and the configuration, and does no I/O. Every chunk is loaded through
//! [`ChunkCache::get`], which verifies its bytes against the manifest's store address before
//! decoding. On its first load in a stream every point of a chunk is checked: its original
//! index is below the point count ([`Fault::IndexOutOfRange`]) and it lies in its chunk's cell
//! ([`Fault::PointOutsideCell`]). The original-order stream also checks that the indexes it
//! emits are exactly `0, 1, ..., n - 1` ([`Fault::DuplicateIndex`], [`Fault::MissingIndex`]),
//! which proves that every point is visited exactly once (REQ-GEO.OOC.STREAM-001). The
//! spatial stream checks the range and the cell of every point and that the chunk counts add
//! up to the point count; an index repeated in two different chunks is found only by the
//! original-order stream (and by [`original_digest`]), because finding it in spatial order
//! needs memory proportional to the cloud.
//!
//! # Errors, cancellation and memory
//!
//! The cancellation flag is read before every block (and between chunk loads while the
//! original-order merge starts). After any error, [`OocError::Cancelled`] included, the
//! stream is failed: every later call returns the same error, never a partial block or a
//! silent end. Blocks are allocated fallibly ([`OocError::AllocationFailed`]); a block holds
//! at most [`MAX_BLOCK_POINTS`] points, and the merge state is 24 bytes per chunk, reserved
//! fallibly when the merge starts. Nothing here panics and no floating-point arithmetic is
//! done: coordinates are copied bit for bit; the cell check reuses the partition's
//! quantisation ([`Grid::morton`]).
//!
//! # `PointSource` channels
//!
//! The trait carries channels as named `f64` columns. [`ChunkStream`] emits the requested
//! channels in name order as: `colour_r`, `colour_g`, `colour_b` (0..=255); `intensity` (the
//! stored `f32`, widened exactly); `normal_x`, `normal_y`, `normal_z`; `quality`;
//! `scanner_id`; `timestamp_ns` (refused with [`OocError::InexactChannel`] for a value
//! beyond +-2^53, which has no exact `f64`). [`ChunkStream::next_stream_block`] returns the
//! typed columns instead.
//!
//! # LOD data
//!
//! `PointSource` is implemented here only for [`ChunkStream`], which reads full-resolution
//! chunks; nothing converts other data into a stream (REQ-GEO.OOC.STREAM-004).

use crate::cache::{ChunkCache, ChunkHandle};
use crate::cell::Grid;
use crate::error::{chunk_fault, manifest_fault, Fault, OocError};
use crate::format::{Channel, ChannelSet, Chunk, ChunkManifest};
use crate::store::ChunkStore;
use core::cmp::Reverse;
use core::ops::Range;
use core::sync::atomic::{AtomicBool, Ordering};
use fid_geo_types::digest::{
    canonical_f32_bits, canonical_f64_bits, kind_byte, ChannelType, LAYOUT_TAG, LAYOUT_VERSION,
};
use fid_geo_types::{AttributeSet, ElementDigest, ElementKind, FrameRef, GeoTypeError, PointBlock, PointSource};
use fid_kci::kci::Sha256;
use std::collections::BinaryHeap;

/// Default points per block.
pub const DEFAULT_BLOCK_POINTS: usize = 65_536;
/// Largest `block_points` accepted (2^20).
pub const MAX_BLOCK_POINTS: usize = 1 << 20;
/// Default number of chunks the original-order merge keeps pinned.
pub const DEFAULT_FAN_IN: usize = 8;
/// Largest `fan_in` accepted.
pub const MAX_FAN_IN: usize = 1024;
/// Bytes of merge state per chunk: a heap entry `(u64, usize)` and a cursor `u64`.
pub const MERGE_BYTES_PER_CHUNK: u64 = 24;

/// Largest timestamp magnitude with an exact `f64` form for every smaller value (2^53).
const EXACT_I64: i64 = 1 << 53;

/// The order of a [`ChunkStream`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamOrder {
    /// Chunks in Morton order of their cells, points within a chunk by original index.
    Spatial,
    /// Points by ascending original index.
    Original,
}

/// Configuration of a [`ChunkStream`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StreamConfig {
    pub order: StreamOrder,
    /// Points per block, `1..=MAX_BLOCK_POINTS`.
    pub block_points: usize,
    /// Channels to carry; a subset of the manifest's.
    pub channels: ChannelSet,
    /// Chunks the original-order merge keeps pinned, `1..=MAX_FAN_IN`. Ignored in spatial
    /// order, which pins one chunk at a time.
    pub fan_in: usize,
}

impl Default for StreamConfig {
    fn default() -> StreamConfig {
        StreamConfig {
            order: StreamOrder::Spatial,
            block_points: DEFAULT_BLOCK_POINTS,
            channels: ChannelSet::EMPTY,
            fan_in: DEFAULT_FAN_IN,
        }
    }
}

impl StreamConfig {
    /// The default configuration with `order`.
    pub fn new(order: StreamOrder) -> StreamConfig {
        StreamConfig { order, ..StreamConfig::default() }
    }
}

/// One block of a [`ChunkStream`] with typed channel columns. All columns have the same
/// length; `attrs` holds the requested channels only (no scan origins, no grid).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct StreamBlock {
    pub original_index: Vec<u64>,
    pub x: Vec<f64>,
    pub y: Vec<f64>,
    pub z: Vec<f64>,
    pub attrs: AttributeSet,
}

impl StreamBlock {
    /// The number of points.
    pub fn len(&self) -> usize {
        self.original_index.len()
    }

    /// Whether the block has no points (never returned by a stream).
    pub fn is_empty(&self) -> bool {
        self.original_index.is_empty()
    }
}

fn reserve<T>(cap: usize, needed: u64) -> Result<Vec<T>, OocError> {
    let mut v = Vec::new();
    v.try_reserve_exact(cap).map_err(|_| OocError::AllocationFailed { needed })?;
    Ok(v)
}

fn new_block(cap: usize, ch: ChannelSet) -> Result<StreamBlock, OocError> {
    let needed = (cap as u64).saturating_mul(ch.record_bytes());
    let mut attrs = AttributeSet::default();
    if ch.contains(Channel::Colour) {
        attrs.colour = Some(reserve(cap, needed)?);
    }
    if ch.contains(Channel::Intensity) {
        attrs.intensity = Some(reserve(cap, needed)?);
    }
    if ch.contains(Channel::Normal) {
        attrs.normal = Some([reserve(cap, needed)?, reserve(cap, needed)?, reserve(cap, needed)?]);
    }
    if ch.contains(Channel::Quality) {
        attrs.quality = Some(reserve(cap, needed)?);
    }
    if ch.contains(Channel::ScannerId) {
        attrs.scanner_id = Some(reserve(cap, needed)?);
    }
    if ch.contains(Channel::TimestampNs) {
        attrs.timestamp_ns = Some(reserve(cap, needed)?);
    }
    Ok(StreamBlock {
        original_index: reserve(cap, needed)?,
        x: reserve(cap, needed)?,
        y: reserve(cap, needed)?,
        z: reserve(cap, needed)?,
        attrs,
    })
}

/// Appends `src[r]` to `dst` (capacity reserved fallibly; a no-op on a reserved block).
fn ext<T: Copy>(dst: &mut Vec<T>, src: &[T], r: Range<usize>, column: &'static str) -> Result<(), OocError> {
    let s = src.get(r).ok_or(chunk_fault(Fault::ColumnLength { column }))?;
    dst.try_reserve(s.len()).map_err(|_| OocError::AllocationFailed { needed: (s.len() * size_of::<T>()) as u64 })?;
    dst.extend_from_slice(s);
    Ok(())
}

fn ext_opt<T: Copy>(
    dst: &mut Option<Vec<T>>,
    src: &Option<Vec<T>>,
    r: &Range<usize>,
    column: &'static str,
) -> Result<(), OocError> {
    match (dst, src) {
        (None, _) => Ok(()),
        (Some(d), Some(s)) => ext(d, s, r.clone(), column),
        (Some(_), None) => Err(chunk_fault(Fault::WrongChannels)),
    }
}

/// Appends points `r` of chunk `c` to block `b`.
fn append(b: &mut StreamBlock, c: &Chunk, r: Range<usize>) -> Result<(), OocError> {
    ext(&mut b.original_index, &c.index, r.clone(), "index")?;
    ext(&mut b.x, &c.points.x, r.clone(), "x")?;
    ext(&mut b.y, &c.points.y, r.clone(), "y")?;
    ext(&mut b.z, &c.points.z, r.clone(), "z")?;
    let (d, s) = (&mut b.attrs, &c.attrs);
    ext_opt(&mut d.colour, &s.colour, &r, "colour")?;
    ext_opt(&mut d.intensity, &s.intensity, &r, "intensity")?;
    match (&mut d.normal, &s.normal) {
        (None, _) => {}
        (Some([dx, dy, dz]), Some([sx, sy, sz])) => {
            ext(dx, sx, r.clone(), "normal_x")?;
            ext(dy, sy, r.clone(), "normal_y")?;
            ext(dz, sz, r.clone(), "normal_z")?;
        }
        (Some(_), None) => return Err(chunk_fault(Fault::WrongChannels)),
    }
    ext_opt(&mut d.quality, &s.quality, &r, "quality")?;
    ext_opt(&mut d.scanner_id, &s.scanner_id, &r, "scanner_id")?;
    ext_opt(&mut d.timestamp_ns, &s.timestamp_ns, &r, "timestamp_ns")?;
    Ok(())
}

/// Checks every point of a freshly pinned chunk: index below `n`, point inside its cell.
fn verify_chunk(grid: &Grid, c: &Chunk, n: u64) -> Result<(), OocError> {
    for (j, &idx) in c.index.iter().enumerate() {
        if idx >= n {
            return Err(chunk_fault(Fault::IndexOutOfRange { index: idx }));
        }
        let p = match (c.points.x.get(j), c.points.y.get(j), c.points.z.get(j)) {
            (Some(&a), Some(&b), Some(&d)) => [a, b, d],
            _ => return Err(chunk_fault(Fault::ColumnLength { column: "x" })),
        };
        if !grid.inside(p) || !c.cell.contains(grid.morton(p)) {
            return Err(chunk_fault(Fault::PointOutsideCell { index: idx }));
        }
    }
    Ok(())
}

struct Pin<'a> {
    chunk: usize,
    handle: ChunkHandle<'a>,
    used: u64,
}

enum State<'a> {
    Spatial {
        /// Position of the current chunk in the manifest.
        chunk: usize,
        /// Next point within it.
        pos: usize,
        handle: Option<ChunkHandle<'a>>,
    },
    Original {
        started: bool,
        heap: BinaryHeap<Reverse<(u64, usize)>>,
        cursor: Vec<u64>,
        pins: Vec<Pin<'a>>,
        tick: u64,
        /// Next original index to emit.
        expected: u64,
    },
}

/// The context every chunk load needs.
struct Ctx<'a> {
    manifest: &'a ChunkManifest,
    store: &'a dyn ChunkStore,
    cache: &'a ChunkCache,
    cancel: Option<&'a AtomicBool>,
    grid: Grid,
}

impl Ctx<'_> {
    fn cancelled(&self) -> bool {
        self.cancel.is_some_and(|c| c.load(Ordering::Acquire))
    }
}

/// A stream of full-resolution point blocks over a chunked cloud. See the
/// [module documentation](self).
pub struct ChunkStream<'a> {
    ctx: Ctx<'a>,
    cfg: StreamConfig,
    state: State<'a>,
    emitted: u64,
    failed: Option<OocError>,
}

impl core::fmt::Debug for ChunkStream<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ChunkStream")
            .field("cfg", &self.cfg)
            .field("n_points", &self.ctx.manifest.n_points)
            .field("emitted", &self.emitted)
            .field("failed", &self.failed)
            .finish()
    }
}

impl<'a> ChunkStream<'a> {
    /// A stream over the cloud of `manifest`, loading chunks from `store` through `cache`.
    /// Does no I/O. `cancel`, when given, is read before every block.
    ///
    /// Errors: [`OocError::InvalidParams`] (`what` = `"block_points"`, `"fan_in"` or
    /// `"channels"` for channels the manifest does not carry); the manifest's validation
    /// errors ([`OocError::CorruptManifest`]); [`OocError::BoundsTooLarge`].
    pub fn new(
        manifest: &'a ChunkManifest,
        store: &'a dyn ChunkStore,
        cache: &'a ChunkCache,
        cancel: Option<&'a AtomicBool>,
        cfg: StreamConfig,
    ) -> Result<ChunkStream<'a>, OocError> {
        if cfg.block_points == 0 || cfg.block_points > MAX_BLOCK_POINTS {
            return Err(OocError::InvalidParams { what: "block_points" });
        }
        if cfg.fan_in == 0 || cfg.fan_in > MAX_FAN_IN {
            return Err(OocError::InvalidParams { what: "fan_in" });
        }
        if cfg.channels.iter().any(|c| !manifest.channels.contains(c)) {
            return Err(OocError::InvalidParams { what: "channels" });
        }
        manifest.validate()?;
        let grid = Grid::new(&manifest.aabb)?;
        let state = match cfg.order {
            StreamOrder::Spatial => State::Spatial { chunk: 0, pos: 0, handle: None },
            StreamOrder::Original => State::Original {
                started: false,
                heap: BinaryHeap::new(),
                cursor: Vec::new(),
                pins: Vec::new(),
                tick: 0,
                expected: 0,
            },
        };
        Ok(ChunkStream { ctx: Ctx { manifest, store, cache, cancel, grid }, cfg, state, emitted: 0, failed: None })
    }

    /// The configuration.
    pub fn config(&self) -> StreamConfig {
        self.cfg
    }

    /// Points emitted so far.
    pub fn emitted(&self) -> u64 {
        self.emitted
    }

    /// The next block with typed channel columns; `None` after the last point. After an
    /// error every call returns that error again.
    pub fn next_stream_block(&mut self) -> Option<Result<StreamBlock, OocError>> {
        if let Some(e) = &self.failed {
            return Some(Err(e.clone()));
        }
        let n = self.ctx.manifest.n_points;
        if self.emitted >= n {
            self.release();
            return None;
        }
        let r = if self.ctx.cancelled() { Err(OocError::Cancelled) } else { self.fill() };
        match r {
            Ok(b) => {
                self.emitted = self.emitted.saturating_add(b.len() as u64);
                if self.emitted >= n {
                    self.release();
                }
                Some(Ok(b))
            }
            Err(e) => Some(Err(self.fail(e))),
        }
    }

    fn fail(&mut self, e: OocError) -> OocError {
        self.release();
        self.failed = Some(e.clone());
        e
    }

    /// Unpins every chunk the stream holds.
    fn release(&mut self) {
        match &mut self.state {
            State::Spatial { handle, .. } => *handle = None,
            State::Original { pins, .. } => pins.clear(),
        }
    }

    fn fill(&mut self) -> Result<StreamBlock, OocError> {
        let remaining = self.ctx.manifest.n_points.saturating_sub(self.emitted);
        let cap = usize::try_from(remaining).map_or(self.cfg.block_points, |r| r.min(self.cfg.block_points));
        let mut b = new_block(cap, self.cfg.channels)?;
        let ctx = &self.ctx;
        match &mut self.state {
            State::Spatial { chunk, pos, handle } => fill_spatial(ctx, &mut b, cap, chunk, pos, handle)?,
            State::Original { started, heap, cursor, pins, tick, expected } => {
                if !*started {
                    start_merge(ctx, self.cfg.fan_in, heap, cursor, pins, tick)?;
                    *started = true;
                }
                fill_original(ctx, self.cfg.fan_in, &mut b, cap, heap, cursor, pins, tick, expected)?;
            }
        }
        if b.len() != cap {
            // The chunks ran out before the manifest's point count (or the merge stopped).
            return Err(manifest_fault(Fault::CountMismatch));
        }
        Ok(b)
    }

    /// The next block as a `PointSource` block (see the module documentation for the
    /// channel columns), with the stream's error type.
    pub fn next_point_block(&mut self) -> Option<Result<PointBlock, OocError>> {
        let b = match self.next_stream_block()? {
            Ok(b) => b,
            Err(e) => return Some(Err(e)),
        };
        let first = self.emitted.saturating_sub(b.len() as u64);
        match to_point_block(b, first) {
            Ok(p) => Some(Ok(p)),
            Err(e) => Some(Err(self.fail(e))),
        }
    }
}

fn fill_spatial<'a>(
    ctx: &Ctx<'a>,
    b: &mut StreamBlock,
    cap: usize,
    chunk: &mut usize,
    pos: &mut usize,
    handle: &mut Option<ChunkHandle<'a>>,
) -> Result<(), OocError> {
    let m = ctx.manifest;
    while b.len() < cap && *chunk < m.chunks.len() {
        if handle.is_none() {
            let h = ctx.cache.get(ctx.store, m, *chunk)?;
            verify_chunk(&ctx.grid, h.chunk(), m.n_points)?;
            *pos = 0;
            *handle = Some(h);
        }
        let Some(h) = handle.as_ref() else { break };
        let len = h.chunk().index.len();
        let take = len.saturating_sub(*pos).min(cap - b.len());
        let end = *pos + take;
        append(b, h.chunk(), *pos..end)?;
        *pos = end;
        if *pos >= len {
            *handle = None;
            *chunk += 1;
            *pos = 0;
        }
    }
    Ok(())
}

/// Pins chunk `c` for the merge, reusing a held pin, and returns its slot. Drops the least
/// recently used pin beyond `fan_in`; while the cache is exhausted and pins are held, drops
/// them least recently used first and tries again.
fn pin<'a>(ctx: &Ctx<'a>, fan_in: usize, pins: &mut Vec<Pin<'a>>, tick: &mut u64, c: usize) -> Result<usize, OocError> {
    *tick = tick.saturating_add(1);
    if let Some(slot) = pins.iter().position(|p| p.chunk == c) {
        if let Some(p) = pins.get_mut(slot) {
            p.used = *tick;
        }
        return Ok(slot);
    }
    if pins.len() >= fan_in {
        if let Some(lru) = pins.iter().enumerate().min_by_key(|(_, p)| p.used).map(|(i, _)| i) {
            pins.swap_remove(lru);
        }
    }
    // If the held pins leave the cache no room, drop them least recently used first.
    let handle = loop {
        match ctx.cache.get(ctx.store, ctx.manifest, c) {
            Ok(h) => break h,
            Err(OocError::CacheExhausted { .. }) if !pins.is_empty() => {
                if let Some(lru) = pins.iter().enumerate().min_by_key(|(_, p)| p.used).map(|(i, _)| i) {
                    pins.swap_remove(lru);
                }
            }
            Err(e) => return Err(e),
        }
    };
    pins.try_reserve(1).map_err(|_| OocError::AllocationFailed { needed: size_of::<Pin<'_>>() as u64 })?;
    pins.push(Pin { chunk: c, handle, used: *tick });
    Ok(pins.len() - 1)
}

/// Loads every chunk once (checking it), and seeds the heap with each chunk's first index.
fn start_merge<'a>(
    ctx: &Ctx<'a>,
    fan_in: usize,
    heap: &mut BinaryHeap<Reverse<(u64, usize)>>,
    cursor: &mut Vec<u64>,
    pins: &mut Vec<Pin<'a>>,
    tick: &mut u64,
) -> Result<(), OocError> {
    let k = ctx.manifest.chunks.len();
    let needed = (k as u64).saturating_mul(MERGE_BYTES_PER_CHUNK);
    heap.try_reserve_exact(k).map_err(|_| OocError::AllocationFailed { needed })?;
    cursor.try_reserve_exact(k).map_err(|_| OocError::AllocationFailed { needed })?;
    pins.try_reserve_exact(fan_in.min(k))
        .map_err(|_| OocError::AllocationFailed { needed: (fan_in * size_of::<Pin<'_>>()) as u64 })?;
    for c in 0..k {
        if ctx.cancelled() {
            return Err(OocError::Cancelled);
        }
        let slot = pin(ctx, fan_in, pins, tick, c)?;
        let chunk = pins.get(slot).map(|p| p.handle.chunk()).ok_or(OocError::InvalidParams { what: "chunk" })?;
        verify_chunk(&ctx.grid, chunk, ctx.manifest.n_points)?;
        let first = *chunk.index.first().ok_or(chunk_fault(Fault::ZeroCount))?;
        cursor.push(0);
        heap.push(Reverse((first, c)));
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn fill_original<'a>(
    ctx: &Ctx<'a>,
    fan_in: usize,
    b: &mut StreamBlock,
    cap: usize,
    heap: &mut BinaryHeap<Reverse<(u64, usize)>>,
    cursor: &mut [u64],
    pins: &mut Vec<Pin<'a>>,
    tick: &mut u64,
    expected: &mut u64,
) -> Result<(), OocError> {
    let n = ctx.manifest.n_points;
    while b.len() < cap {
        let Some(Reverse((_, c))) = heap.pop() else { break };
        // Points of chunk `c` up to the next head index come before every other chunk's.
        let bound = heap.peek().map_or(u64::MAX, |Reverse((h, _))| *h);
        let slot = pin(ctx, fan_in, pins, tick, c)?;
        let chunk = pins.get(slot).map(|p| p.handle.chunk()).ok_or(OocError::InvalidParams { what: "chunk" })?;
        let cur = cursor.get_mut(c).ok_or(OocError::InvalidParams { what: "chunk" })?;
        let start = usize::try_from(*cur).map_err(|_| chunk_fault(Fault::IndexOrder { at: *cur }))?;
        let len = chunk.index.len();
        let room = cap - b.len();
        let mut end = start;
        while end < len && end - start < room {
            let idx = chunk.index.get(end).copied().unwrap_or(u64::MAX);
            if end > start && idx > bound {
                break;
            }
            if idx != *expected {
                return Err(chunk_fault(if idx < *expected {
                    Fault::DuplicateIndex { index: idx }
                } else if idx >= n {
                    Fault::IndexOutOfRange { index: idx }
                } else {
                    Fault::MissingIndex { index: *expected }
                }));
            }
            *expected += 1;
            end += 1;
        }
        append(b, chunk, start..end)?;
        *cur = end as u64;
        if let Some(&next) = chunk.index.get(end) {
            // The heap was reserved for every chunk and holds one entry per unfinished
            // chunk, so this never grows it.
            heap.push(Reverse((next, c)));
        }
    }
    if heap.is_empty() && *expected != n && b.len() < cap {
        return Err(chunk_fault(Fault::MissingIndex { index: *expected }));
    }
    Ok(())
}

fn named(name: &str, values: Vec<f64>) -> Result<fid_geo_types::Channel, OocError> {
    let mut s = String::new();
    s.try_reserve_exact(name.len()).map_err(|_| OocError::AllocationFailed { needed: name.len() as u64 })?;
    s.push_str(name);
    Ok(fid_geo_types::Channel { name: s, values })
}

fn widen<T: Copy>(v: &[T], f: impl Fn(T) -> f64) -> Result<Vec<f64>, OocError> {
    let mut out = reserve(v.len(), (v.len() as u64).saturating_mul(8))?;
    out.extend(v.iter().map(|&x| f(x)));
    Ok(out)
}

/// Converts a typed block into a `PointSource` block; `first` is the stream position of its
/// first point (for error reports).
fn to_point_block(b: StreamBlock, first: u64) -> Result<PointBlock, OocError> {
    let a = b.attrs;
    let mut channels = Vec::new();
    channels.try_reserve_exact(10).map_err(|_| OocError::AllocationFailed { needed: 10 * 48 })?;
    if let Some(c) = &a.colour {
        channels.push(named("colour_r", widen(c, |v| f64::from(v[0]))?)?);
        channels.push(named("colour_g", widen(c, |v| f64::from(v[1]))?)?);
        channels.push(named("colour_b", widen(c, |v| f64::from(v[2]))?)?);
    }
    if let Some(v) = &a.intensity {
        channels.push(named("intensity", widen(v, f64::from)?)?);
    }
    if let Some([nx, ny, nz]) = a.normal {
        channels.push(named("normal_x", nx)?);
        channels.push(named("normal_y", ny)?);
        channels.push(named("normal_z", nz)?);
    }
    if let Some(v) = &a.quality {
        channels.push(named("quality", widen(v, f64::from)?)?);
    }
    if let Some(v) = &a.scanner_id {
        channels.push(named("scanner_id", widen(v, f64::from)?)?);
    }
    if let Some(v) = &a.timestamp_ns {
        if let Some(i) = v.iter().position(|t| !(-EXACT_I64..=EXACT_I64).contains(t)) {
            let index = b.original_index.get(i).copied().unwrap_or(first.saturating_add(i as u64));
            return Err(OocError::InexactChannel { channel: "timestamp_ns", index });
        }
        // |t| <= 2^53, so the conversion is exact.
        channels.push(named("timestamp_ns", widen(v, |t| t as f64)?)?);
    }
    Ok(PointBlock { original_index: b.original_index, x: b.x, y: b.y, z: b.z, channels })
}

/// The `GeoTypeError` a `PointSource` consumer sees for a stream error.
pub fn to_geo_error(e: OocError) -> GeoTypeError {
    match e {
        OocError::Cancelled => GeoTypeError::Cancelled,
        OocError::Geometry(g) => g,
        OocError::AllocationFailed { needed } => GeoTypeError::AllocationFailed { needed },
        other => GeoTypeError::Source { code: other.code(), detail: other.to_string() },
    }
}

impl PointSource for ChunkStream<'_> {
    fn next_block(&mut self) -> Option<Result<PointBlock, GeoTypeError>> {
        Some(self.next_point_block()?.map_err(to_geo_error))
    }

    fn len(&self) -> u64 {
        self.ctx.manifest.n_points
    }

    fn frame(&self) -> FrameRef {
        self.ctx.manifest.frame
    }
}

// ------------------------------------------------------------------ original digest

/// A 64 KiB buffer in front of the hasher.
struct HashBuf {
    sha: Sha256,
    buf: Vec<u8>,
}

const HASH_BUF: usize = 64 * 1024;

impl HashBuf {
    fn new() -> Result<HashBuf, OocError> {
        Ok(HashBuf { sha: Sha256::new(), buf: reserve(HASH_BUF, HASH_BUF as u64)? })
    }

    fn put(&mut self, bytes: &[u8]) {
        if self.buf.len() + bytes.len() > HASH_BUF {
            self.sha.update(&self.buf);
            self.buf.clear();
        }
        if bytes.len() > HASH_BUF {
            self.sha.update(bytes);
        } else {
            self.buf.extend_from_slice(bytes);
        }
    }

    fn finish(mut self) -> [u8; 32] {
        self.sha.update(&self.buf);
        self.sha.finish()
    }
}

/// One column of the GEO.TYPES.DIGEST layout, hashed by one original-order pass.
#[derive(Clone, Copy)]
enum Column {
    X,
    Y,
    Z,
    Colour,
    Intensity,
    Normal(usize),
    Quality,
    ScannerId,
    TimestampNs,
}

fn f64_col(h: &mut HashBuf, v: &[f64]) {
    for &x in v {
        h.put(&canonical_f64_bits(x).to_le_bytes());
    }
}

fn hash_column(h: &mut HashBuf, b: &StreamBlock, col: Column) -> Result<(), OocError> {
    let missing = || chunk_fault(Fault::WrongChannels);
    let a = &b.attrs;
    match col {
        Column::X => f64_col(h, &b.x),
        Column::Y => f64_col(h, &b.y),
        Column::Z => f64_col(h, &b.z),
        Column::Colour => a.colour.as_ref().ok_or_else(missing)?.iter().for_each(|c| h.put(c)),
        Column::Intensity => {
            a.intensity.as_ref().ok_or_else(missing)?.iter().for_each(|v| h.put(&canonical_f32_bits(*v).to_le_bytes()))
        }
        Column::Normal(k) => f64_col(h, a.normal.as_ref().and_then(|n| n.get(k)).ok_or_else(missing)?),
        Column::Quality => a.quality.as_ref().ok_or_else(missing)?.iter().for_each(|v| h.put(&v.to_le_bytes())),
        Column::ScannerId => a.scanner_id.as_ref().ok_or_else(missing)?.iter().for_each(|v| h.put(&v.to_le_bytes())),
        Column::TimestampNs => {
            a.timestamp_ns.as_ref().ok_or_else(missing)?.iter().for_each(|v| h.put(&v.to_le_bytes()))
        }
    }
    Ok(())
}

fn channel_head(h: &mut HashBuf, name: &str, ty: ChannelType, records: u64) {
    h.put(&(name.len() as u16).to_le_bytes());
    h.put(name.as_bytes());
    h.put(&[ty.code()]);
    h.put(&records.to_le_bytes());
}

/// The GEO.TYPES.DIGEST element digest of the chunked cloud, computed from original-order
/// streams: the layout is columnar, so each column (x, y, z, then every channel column in
/// name order) is one pass over the cloud, with memory bounded by one block. `cfg` gives
/// the block size and fan-in; its order and channels are set per pass. Cancellation or any
/// error returns no digest.
pub fn original_digest(
    manifest: &ChunkManifest,
    store: &dyn ChunkStore,
    cache: &ChunkCache,
    cancel: Option<&AtomicBool>,
    cfg: StreamConfig,
) -> Result<ElementDigest, OocError> {
    let n = manifest.n_points;
    let ch = manifest.channels;
    let mut h = HashBuf::new()?;
    let pass = |h: &mut HashBuf, col: Column, channel: Option<Channel>| -> Result<(), OocError> {
        let channels = channel.map_or(ChannelSet::EMPTY, |c| ChannelSet::EMPTY.with(c));
        let c = StreamConfig { order: StreamOrder::Original, channels, ..cfg };
        let mut s = ChunkStream::new(manifest, store, cache, cancel, c)?;
        while let Some(b) = s.next_stream_block() {
            hash_column(h, &b?, col)?;
        }
        Ok(())
    };
    h.put(&LAYOUT_TAG);
    h.put(&[LAYOUT_VERSION, kind_byte(ElementKind::PointCloud)]);
    h.put(&manifest.frame.to_bytes());
    h.put(&n.to_le_bytes());
    for col in [Column::X, Column::Y, Column::Z] {
        pass(&mut h, col, None)?;
    }
    h.put(&0u64.to_le_bytes());
    let n_channels =
        ch.iter().count() as u64 + u64::from(manifest.grid.is_some()) + u64::from(!manifest.scan_origins.is_empty());
    h.put(&n_channels.to_le_bytes());
    if ch.contains(Channel::Colour) {
        channel_head(&mut h, "colour", ChannelType::U8x3, n);
        pass(&mut h, Column::Colour, Some(Channel::Colour))?;
    }
    if let Some((rows, cols)) = manifest.grid {
        channel_head(&mut h, "grid", ChannelType::U64, 2);
        h.put(&rows.to_le_bytes());
        h.put(&cols.to_le_bytes());
    }
    if ch.contains(Channel::Intensity) {
        channel_head(&mut h, "intensity", ChannelType::F32, n);
        pass(&mut h, Column::Intensity, Some(Channel::Intensity))?;
    }
    if ch.contains(Channel::Normal) {
        channel_head(&mut h, "normal", ChannelType::F64x3Columns, n);
        for k in 0..3 {
            pass(&mut h, Column::Normal(k), Some(Channel::Normal))?;
        }
    }
    if ch.contains(Channel::Quality) {
        channel_head(&mut h, "quality", ChannelType::U16, n);
        pass(&mut h, Column::Quality, Some(Channel::Quality))?;
    }
    if !manifest.scan_origins.is_empty() {
        channel_head(&mut h, "scan_origins", ChannelType::Origin, manifest.scan_origins.len() as u64);
        for (id, o) in &manifest.scan_origins {
            h.put(&id.to_le_bytes());
            for v in o {
                h.put(&canonical_f64_bits(*v).to_le_bytes());
            }
        }
    }
    if ch.contains(Channel::ScannerId) {
        channel_head(&mut h, "scanner_id", ChannelType::U32, n);
        pass(&mut h, Column::ScannerId, Some(Channel::ScannerId))?;
    }
    if ch.contains(Channel::TimestampNs) {
        channel_head(&mut h, "timestamp_ns", ChannelType::I64, n);
        pass(&mut h, Column::TimestampNs, Some(Channel::TimestampNs))?;
    }
    Ok(ElementDigest(h.finish()))
}

/// Checks that [`original_digest`] equals the manifest's original digest
/// (REQ-GEO.OOC.STREAM-003); [`OocError::DigestMismatch`] otherwise.
pub fn verify_original_digest(
    manifest: &ChunkManifest,
    store: &dyn ChunkStore,
    cache: &ChunkCache,
    cancel: Option<&AtomicBool>,
    cfg: StreamConfig,
) -> Result<(), OocError> {
    if original_digest(manifest, store, cache, cancel, cfg)? == manifest.original_digest {
        Ok(())
    } else {
        Err(OocError::DigestMismatch)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_bytes_cover_real_sizes() {
        assert!(
            MERGE_BYTES_PER_CHUNK as usize >= size_of::<Reverse<(u64, usize)>>() + size_of::<u64>(),
            "heap entry {} + cursor {}",
            size_of::<Reverse<(u64, usize)>>(),
            size_of::<u64>()
        );
    }

    #[test]
    fn stream_is_send() {
        fn is<T: Send>() {}
        is::<StreamBlock>();
    }
}
