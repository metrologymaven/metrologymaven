//! The octree partition and conversion between in-memory clouds and chunks.

use crate::cell::{CellCode, Grid, MAX_DEPTH};
use crate::error::{chunk_fault, Fault, OocError};
use crate::format::{
    decode_chunk, encode_chunk, Channel, ChannelSet, Chunk, ChunkEntry, ChunkManifest, MAX_CHUNK_POINTS_LIMIT,
    MAX_MANIFEST_CHUNKS,
};
use crate::store::ChunkStore;
use fid_geo_types::digest::digest_point_cloud;
use fid_geo_types::{Aabb, AttributeSet, GeoTypeError, PointBuffer, PointCloud};

/// Default split threshold: a cell holding more points than this is split.
pub const DEFAULT_MAX_CHUNK_POINTS: u64 = 1 << 20;

/// Octree parameters of a partition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LayoutParams {
    /// A cell is split when it holds more points than this (1 to 2^32), unless it is at
    /// the depth limit.
    pub max_chunk_points: u64,
}

impl Default for LayoutParams {
    fn default() -> LayoutParams {
        LayoutParams { max_chunk_points: DEFAULT_MAX_CHUNK_POINTS }
    }
}

pub(crate) fn check_params(max_chunk_points: u64) -> Result<(), OocError> {
    if max_chunk_points == 0 || max_chunk_points > MAX_CHUNK_POINTS_LIMIT {
        return Err(OocError::InvalidParams { what: "max_chunk_points" });
    }
    Ok(())
}

/// A point in the partition: full Morton code, original index, position in its table.
/// Sorted ascending, keys group by cell in Morton order and by index inside a code.
pub(crate) type Key = (u64, u64, usize);

/// Calls `f` for every leaf of the subtree rooted at `cell`, in Morton order, with the
/// keys of its points. `keys` are sorted ascending and all lie in `cell`. A cell is a leaf
/// when it holds at most `max` points or is at the depth limit; empty cells are skipped.
pub(crate) fn for_each_leaf(
    cell: CellCode,
    keys: &[Key],
    max: u64,
    f: &mut dyn FnMut(CellCode, &[Key]) -> Result<(), OocError>,
) -> Result<(), OocError> {
    let mut stack: Vec<(CellCode, usize, usize)> = vec![(cell, 0, keys.len())];
    while let Some((c, lo, hi)) = stack.pop() {
        let Some(slice) = keys.get(lo..hi) else { continue };
        if slice.is_empty() {
            continue;
        }
        if slice.len() as u64 <= max || c.depth() >= MAX_DEPTH {
            f(c, slice)?;
            continue;
        }
        // Push children 7..0 so that child 0 is processed first.
        for o in (0..8u8).rev() {
            let Some(ch) = c.child(o) else { continue };
            let a = lo + slice.partition_point(|k| k.0 < ch.range_start());
            let b = lo + slice.partition_point(|k| k.0 < ch.range_end());
            stack.push((ch, a, b));
        }
    }
    Ok(())
}

fn pick<T: Copy>(v: &[T], pos: &[usize]) -> Result<Vec<T>, OocError> {
    pos.iter().map(|&p| v.get(p).copied().ok_or(OocError::ChannelMismatch { index: p as u64 })).collect()
}

fn pick_opt<T: Copy>(v: &Option<Vec<T>>, pos: &[usize]) -> Result<Option<Vec<T>>, OocError> {
    v.as_deref().map(|v| pick(v, pos)).transpose()
}

/// The chunk of `cell` made of the cloud's points at `pos` (ascending).
fn gather_cloud(cloud: &PointCloud, cell: CellCode, pos: &[usize]) -> Result<Chunk, OocError> {
    let b = cloud.buffer();
    let a = cloud.attributes();
    let normal = match &a.normal {
        Some([x, y, z]) => Some([pick(x, pos)?, pick(y, pos)?, pick(z, pos)?]),
        None => None,
    };
    Ok(Chunk {
        cell,
        index: pos.iter().map(|&p| p as u64).collect(),
        points: PointBuffer::new(pick(&b.x, pos)?, pick(&b.y, pos)?, pick(&b.z, pos)?),
        attrs: AttributeSet {
            normal,
            intensity: pick_opt(&a.intensity, pos)?,
            colour: pick_opt(&a.colour, pos)?,
            scanner_id: pick_opt(&a.scanner_id, pos)?,
            timestamp_ns: pick_opt(&a.timestamp_ns, pos)?,
            quality: pick_opt(&a.quality, pos)?,
            scan_origins: Vec::new(),
            grid: None,
        },
    })
}

/// Appends a manifest entry, refusing more than `limit` entries.
pub(crate) fn push_entry(entries: &mut Vec<ChunkEntry>, e: ChunkEntry, limit: u64) -> Result<(), OocError> {
    if entries.len() as u64 >= limit {
        return Err(OocError::TooManyChunks { limit });
    }
    entries.push(e);
    Ok(())
}

/// Encodes a chunk, stores it, and returns its manifest entry.
pub(crate) fn store_chunk(store: &dyn ChunkStore, chunk: &Chunk) -> Result<ChunkEntry, OocError> {
    let bytes = encode_chunk(chunk)?;
    let blob = store.put_blob(&bytes)?;
    Ok(ChunkEntry { cell: chunk.cell, count: chunk.len(), bytes: bytes.len() as u64, blob })
}

/// [`to_chunks_in`] with the cloud's own bounding box as partition bounds.
pub fn to_chunks(cloud: &PointCloud, store: &dyn ChunkStore, params: &LayoutParams) -> Result<ChunkManifest, OocError> {
    let bounds = cloud.aabb().map_err(|e| match e {
        GeoTypeError::Empty => OocError::EmptyCloud,
        e => OocError::Geometry(e),
    })?;
    to_chunks_in(cloud, &bounds, store, params)
}

/// Partitions `cloud` over `bounds` (which must contain every point), stores every chunk
/// in `store`, and returns the sealed manifest. Fails on an empty cloud, a NaN intensity,
/// a point outside `bounds`, or more than [`MAX_MANIFEST_CHUNKS`] chunks.
pub fn to_chunks_in(
    cloud: &PointCloud,
    bounds: &Aabb,
    store: &dyn ChunkStore,
    params: &LayoutParams,
) -> Result<ChunkManifest, OocError> {
    let max = params.max_chunk_points;
    check_params(max)?;
    if cloud.is_empty() {
        return Err(OocError::EmptyCloud);
    }
    let grid = Grid::new(bounds)?;
    let b = cloud.buffer();
    let a = cloud.attributes();
    if let Some(i) = a.intensity.as_deref().and_then(|v| v.iter().position(|x| x.is_nan())) {
        return Err(OocError::NaNValue { channel: "intensity", index: i as u64 });
    }
    let mut keys: Vec<Key> = Vec::new();
    keys.try_reserve_exact(b.x.len()).map_err(|_| OocError::AllocationFailed { needed: cloud.len() * 24 })?;
    for (i, ((&x, &y), &z)) in b.x.iter().zip(&b.y).zip(&b.z).enumerate() {
        let p = [x, y, z];
        if !grid.inside(p) {
            return Err(OocError::OutOfBounds { index: i as u64 });
        }
        keys.push((grid.morton(p), i as u64, i));
    }
    keys.sort_unstable();
    let mut entries = Vec::new();
    for_each_leaf(CellCode::ROOT, &keys, max, &mut |cell, leaf| {
        let mut pos: Vec<usize> = leaf.iter().map(|k| k.2).collect();
        pos.sort_unstable();
        let e = store_chunk(store, &gather_cloud(cloud, cell, &pos)?)?;
        push_entry(&mut entries, e, MAX_MANIFEST_CHUNKS)
    })?;
    let mut m = ChunkManifest {
        frame: cloud.frame(),
        aabb: *bounds,
        max_chunk_points: max,
        n_points: cloud.len(),
        channels: ChannelSet::of(a),
        scan_origins: a.scan_origins.clone(),
        grid: a.grid,
        chunks: entries,
        original_digest: digest_point_cloud(cloud),
        manifest_digest: [0; 32],
    };
    m.seal();
    Ok(m)
}

/// Bytes [`to_memory`] may hold for manifest `m`: the cloud's columns, a bitmap of the
/// indexes seen, one chunk blob plus its decoded columns, the scan origins and the 64 KiB
/// block buffer of the element digest. `u64::MAX` on overflow.
pub fn memory_needed(m: &ChunkManifest) -> u64 {
    let per_point = m.channels.record_bytes() - 8;
    let columns = m.n_points.checked_mul(per_point);
    let seen = m.n_points.div_ceil(64).checked_mul(8);
    let work = m.max_chunk_bytes().checked_mul(2);
    let origins = (m.scan_origins.len() as u64).checked_mul(32);
    match (columns, seen, work, origins) {
        (Some(c), Some(s), Some(w), Some(o)) => {
            [s, w, o, DIGEST_BLOCK].iter().try_fold(c, |acc, v| acc.checked_add(*v)).unwrap_or(u64::MAX)
        }
        _ => u64::MAX,
    }
}

/// The block buffer of the GEO.TYPES.DIGEST layout writer.
const DIGEST_BLOCK: u64 = 64 * 1024;

/// A zero-filled vector of `n` values, or `AllocationFailed`.
fn filled<T: Clone>(n: usize, v: T) -> Result<Vec<T>, OocError> {
    let mut out = Vec::new();
    let needed = (n as u64).saturating_mul(core::mem::size_of::<T>() as u64);
    out.try_reserve_exact(n).map_err(|_| OocError::AllocationFailed { needed })?;
    out.resize(n, v);
    Ok(out)
}

fn filled_opt<T: Clone>(on: bool, n: usize, v: T) -> Result<Option<Vec<T>>, OocError> {
    if on {
        filled(n, v).map(Some)
    } else {
        Ok(None)
    }
}

fn put<T: Copy>(dst: &mut Option<Vec<T>>, src: &Option<Vec<T>>, to: usize, from: usize) {
    if let (Some(d), Some(s)) = (dst.as_mut(), src.as_ref()) {
        if let (Some(slot), Some(v)) = (d.get_mut(to), s.get(from)) {
            *slot = *v;
        }
    }
}

fn put_col(dst: &mut [f64], src: &[f64], to: usize, from: usize) {
    if let (Some(slot), Some(v)) = (dst.get_mut(to), src.get(from)) {
        *slot = *v;
    }
}

/// Rebuilds the in-memory cloud from its chunks. Fails with
/// [`OocError::BudgetExceeded`] before allocating anything if [`memory_needed`] exceeds
/// `budget` (REQ-GEO.OOC.LAYOUT-005). Every chunk is fetched (the store verifies its
/// address), decoded with its entry size as limit, and checked against the manifest: cell,
/// count, channels, every point inside its cell, every original index below the count and
/// seen once. The result must have the manifest's original digest.
pub fn to_memory(m: &ChunkManifest, store: &dyn ChunkStore, budget: u64) -> Result<PointCloud, OocError> {
    let needed = memory_needed(m);
    if needed > budget {
        return Err(OocError::BudgetExceeded { needed });
    }
    m.validate()?;
    let n = usize::try_from(m.n_points).map_err(|_| OocError::AllocationFailed { needed })?;
    let grid = Grid::new(&m.aabb)?;
    let ch = m.channels;
    let mut x = filled(n, 0.0f64)?;
    let mut y = filled(n, 0.0f64)?;
    let mut z = filled(n, 0.0f64)?;
    let mut attrs = AttributeSet {
        normal: if ch.contains(Channel::Normal) {
            Some([filled(n, 0.0)?, filled(n, 0.0)?, filled(n, 0.0)?])
        } else {
            None
        },
        intensity: filled_opt(ch.contains(Channel::Intensity), n, 0.0f32)?,
        colour: filled_opt(ch.contains(Channel::Colour), n, [0u8; 3])?,
        scanner_id: filled_opt(ch.contains(Channel::ScannerId), n, 0u32)?,
        timestamp_ns: filled_opt(ch.contains(Channel::TimestampNs), n, 0i64)?,
        quality: filled_opt(ch.contains(Channel::Quality), n, 0u16)?,
        scan_origins: m.scan_origins.clone(),
        grid: m.grid,
    };
    let mut seen = filled(n.div_ceil(64), 0u64)?;
    for e in &m.chunks {
        let bytes = store.get_blob(&e.blob, e.bytes)?;
        if bytes.len() as u64 != e.bytes {
            return Err(chunk_fault(Fault::WrongSize { expected: e.bytes, found: bytes.len() as u64 }));
        }
        let c = decode_chunk(&bytes, e.bytes)?;
        drop(bytes);
        if c.cell != e.cell {
            return Err(chunk_fault(Fault::WrongCell { expected: e.cell.raw(), found: c.cell.raw() }));
        }
        if c.len() != e.count {
            return Err(chunk_fault(Fault::WrongCount { expected: e.count, found: c.len() }));
        }
        if c.channels() != ch {
            return Err(chunk_fault(Fault::WrongChannels));
        }
        for (j, &idx) in c.index.iter().enumerate() {
            let i = usize::try_from(idx)
                .ok()
                .filter(|&i| i < n)
                .ok_or(chunk_fault(Fault::IndexOutOfRange { index: idx }))?;
            let Some(word) = seen.get_mut(i / 64) else {
                return Err(chunk_fault(Fault::IndexOutOfRange { index: idx }));
            };
            let bit = 1u64 << (i % 64);
            if *word & bit != 0 {
                return Err(chunk_fault(Fault::DuplicateIndex { index: idx }));
            }
            *word |= bit;
            let p = match (c.points.x.get(j), c.points.y.get(j), c.points.z.get(j)) {
                (Some(&a), Some(&b), Some(&d)) => [a, b, d],
                _ => return Err(chunk_fault(Fault::ColumnLength { column: "x" })),
            };
            if !grid.inside(p) || !e.cell.contains(grid.morton(p)) {
                return Err(chunk_fault(Fault::PointOutsideCell { index: idx }));
            }
            put_col(&mut x, &c.points.x, i, j);
            put_col(&mut y, &c.points.y, i, j);
            put_col(&mut z, &c.points.z, i, j);
            if let (Some([dx, dy, dz]), Some([sx, sy, sz])) = (attrs.normal.as_mut(), c.attrs.normal.as_ref()) {
                put_col(dx, sx, i, j);
                put_col(dy, sy, i, j);
                put_col(dz, sz, i, j);
            }
            put(&mut attrs.intensity, &c.attrs.intensity, i, j);
            put(&mut attrs.colour, &c.attrs.colour, i, j);
            put(&mut attrs.scanner_id, &c.attrs.scanner_id, i, j);
            put(&mut attrs.timestamp_ns, &c.attrs.timestamp_ns, i, j);
            put(&mut attrs.quality, &c.attrs.quality, i, j);
        }
    }
    drop(seen);
    let cloud = PointCloud::new(m.frame, PointBuffer::new(x, y, z), attrs)?;
    if digest_point_cloud(&cloud) != m.original_digest {
        return Err(OocError::DigestMismatch);
    }
    Ok(cloud)
}
