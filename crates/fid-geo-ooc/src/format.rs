//! The chunk byte format v1 and the manifest byte format v1 (pinned; see the crate docs).
//!
//! Readers are bounded and fail closed: every length is checked against the bytes present
//! and the caller's limit before anything is allocated, an unknown version is refused, and
//! truncated, oversized or trailing bytes are typed [`Fault`]s.

use crate::cell::{CellCode, Grid, MAX_DEPTH};
use crate::error::{chunk_fault, manifest_fault, Fault, OocError};
use fid_geo_types::{Aabb, AttributeSet, ElementDigest, FrameRef, PointBuffer};
use fid_kci::kci::Sha256;

/// Tag every chunk starts with.
pub const CHUNK_MAGIC: [u8; 6] = *b"FIDCHK";
/// The chunk format version this crate writes and reads.
pub const CHUNK_VERSION: u8 = 1;
/// Tag every manifest starts with.
pub const MANIFEST_MAGIC: [u8; 6] = *b"FIDCMF";
/// The manifest format version this crate writes and reads.
pub const MANIFEST_VERSION: u8 = 1;
/// Most chunks a manifest may list.
pub const MAX_MANIFEST_CHUNKS: u64 = 1 << 20;
/// Largest `max_chunk_points` accepted.
pub const MAX_CHUNK_POINTS_LIMIT: u64 = 1 << 32;
/// Bytes of one manifest chunk entry: cell, count, bytes (u64 each) and the 32-byte blob.
pub const ENTRY_BYTES: u64 = 56;
/// Bytes of one scan origin in a manifest: ID u32 and x, y, z f64.
const ORIGIN_BYTES: u64 = 28;

/// A per-point attribute channel that chunks carry. Declaration order is ascending
/// name-byte order, the order of every channel list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Channel {
    Colour,
    Intensity,
    Normal,
    Quality,
    ScannerId,
    TimestampNs,
}

impl Channel {
    /// Every channel, in name order.
    pub const ALL: [Channel; 6] = [
        Channel::Colour,
        Channel::Intensity,
        Channel::Normal,
        Channel::Quality,
        Channel::ScannerId,
        Channel::TimestampNs,
    ];

    /// The channel name (as in GEO.TYPES.DIGEST).
    pub const fn name(self) -> &'static str {
        match self {
            Channel::Colour => "colour",
            Channel::Intensity => "intensity",
            Channel::Normal => "normal",
            Channel::Quality => "quality",
            Channel::ScannerId => "scanner_id",
            Channel::TimestampNs => "timestamp_ns",
        }
    }

    /// The type byte (as in GEO.TYPES.DIGEST).
    pub const fn type_byte(self) -> u8 {
        match self {
            Channel::Colour => 3,
            Channel::Intensity => 2,
            Channel::Normal => 8,
            Channel::Quality => 4,
            Channel::ScannerId => 5,
            Channel::TimestampNs => 6,
        }
    }

    /// Bytes per point.
    pub const fn width(self) -> u64 {
        match self {
            Channel::Colour => 3,
            Channel::Intensity => 4,
            Channel::Normal => 24,
            Channel::Quality => 2,
            Channel::ScannerId => 4,
            Channel::TimestampNs => 8,
        }
    }

    const fn bit(self) -> u8 {
        match self {
            Channel::Colour => 1,
            Channel::Intensity => 2,
            Channel::Normal => 4,
            Channel::Quality => 8,
            Channel::ScannerId => 16,
            Channel::TimestampNs => 32,
        }
    }
}

/// A set of [`Channel`]s.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub struct ChannelSet(u8);

impl ChannelSet {
    /// No channels.
    pub const EMPTY: ChannelSet = ChannelSet(0);

    /// The set with `c` added.
    pub const fn with(self, c: Channel) -> ChannelSet {
        ChannelSet(self.0 | c.bit())
    }

    /// Whether `c` is in the set.
    pub const fn contains(self, c: Channel) -> bool {
        self.0 & c.bit() != 0
    }

    /// The members in name order.
    pub fn iter(self) -> impl Iterator<Item = Channel> {
        Channel::ALL.into_iter().filter(move |c| self.contains(*c))
    }

    /// The channels present in `a` (scan origins and grid are cloud-level, not channels).
    pub fn of(a: &AttributeSet) -> ChannelSet {
        let mut s = ChannelSet::EMPTY;
        let present = [
            (Channel::Colour, a.colour.is_some()),
            (Channel::Intensity, a.intensity.is_some()),
            (Channel::Normal, a.normal.is_some()),
            (Channel::Quality, a.quality.is_some()),
            (Channel::ScannerId, a.scanner_id.is_some()),
            (Channel::TimestampNs, a.timestamp_ns.is_some()),
        ];
        for (c, p) in present {
            if p {
                s = s.with(c);
            }
        }
        s
    }

    /// Bytes per point in a chunk: original index, x, y, z and every channel.
    pub fn record_bytes(self) -> u64 {
        32 + self.iter().map(Channel::width).sum::<u64>()
    }

    /// Bytes of the channel list: count byte, then per channel name length, name, type.
    fn list_bytes(self) -> u64 {
        1 + self.iter().map(|c| 2 + c.name().len() as u64).sum::<u64>()
    }

    fn write_list(self, out: &mut dyn FnMut(&[u8])) {
        out(&[self.iter().count() as u8]);
        for c in self.iter() {
            out(&[c.name().len() as u8]);
            out(c.name().as_bytes());
            out(&[c.type_byte()]);
        }
    }

    fn read_list(r: &mut Reader<'_>) -> Result<Option<ChannelSet>, Fault> {
        let n = r.u8()?;
        let mut set = ChannelSet::EMPTY;
        let mut next = 0usize;
        for _ in 0..n {
            let len = usize::from(r.u8()?);
            let name = r.take(len)?;
            let ty = r.u8()?;
            let Some(pos) = Channel::ALL.iter().position(|c| c.name().as_bytes() == name) else {
                return Ok(None);
            };
            let c = Channel::ALL[pos];
            if pos < next || c.type_byte() != ty {
                return Ok(None);
            }
            next = pos + 1;
            set = set.with(c);
        }
        Ok(Some(set))
    }
}

/// Bytes of the chunk header for `channels`: tag, version, cell, count, channel list.
pub fn chunk_header_bytes(channels: ChannelSet) -> u64 {
    6 + 1 + 8 + 8 + channels.list_bytes()
}

/// Bytes of a chunk of `count` points with `channels`; `None` on overflow.
pub fn chunk_bytes(channels: ChannelSet, count: u64) -> Option<u64> {
    count.checked_mul(channels.record_bytes())?.checked_add(chunk_header_bytes(channels))
}

/// One decoded chunk: the points of one octree cell, ordered by original index. The
/// attributes hold per-point channels only (no scan origins, no grid).
#[derive(Debug, Clone, PartialEq)]
pub struct Chunk {
    pub cell: CellCode,
    pub index: Vec<u64>,
    pub points: PointBuffer,
    pub attrs: AttributeSet,
}

impl Chunk {
    /// The number of points.
    pub fn len(&self) -> u64 {
        self.index.len() as u64
    }

    /// Whether the chunk has no points (never valid in the format).
    pub fn is_empty(&self) -> bool {
        self.index.is_empty()
    }

    /// The channels the chunk carries.
    pub fn channels(&self) -> ChannelSet {
        ChannelSet::of(&self.attrs)
    }
}

/// Checks a column length.
fn col_len(column: &'static str, len: usize, n: usize) -> Result<(), OocError> {
    if len == n {
        Ok(())
    } else {
        Err(chunk_fault(Fault::ColumnLength { column }))
    }
}

/// Checks a coordinate or normal column: no NaN, no infinity.
fn check_f64(column: &'static str, v: &[f64]) -> Result<(), Fault> {
    for (i, x) in v.iter().enumerate() {
        if x.is_nan() {
            return Err(Fault::NaN { column, at: i as u64 });
        }
        if x.is_infinite() {
            return Err(Fault::NonFinite { column, at: i as u64 });
        }
    }
    Ok(())
}

/// The canonical f32 NaN a chunk stores for any NaN intensity (as GEO.TYPES.DIGEST).
pub const CANONICAL_NAN_F32: u32 = 0x7fc0_0000;

/// The chunk bits of an intensity: raw bits, except any NaN becomes [`CANONICAL_NAN_F32`].
pub fn intensity_bits(v: f32) -> u32 {
    if v.is_nan() {
        CANONICAL_NAN_F32
    } else {
        v.to_bits()
    }
}

/// Checks the float content. The encoder accepts any NaN intensity (it writes the
/// canonical NaN); the decoder (`decoding`) accepts only the canonical NaN bit pattern.
fn check_values(c: &Chunk, decoding: bool) -> Result<(), Fault> {
    for (w, i) in c.index.windows(2).zip(1u64..) {
        if w[1] <= w[0] {
            return Err(Fault::IndexOrder { at: i });
        }
    }
    check_f64("x", &c.points.x)?;
    check_f64("y", &c.points.y)?;
    check_f64("z", &c.points.z)?;
    if let Some([nx, ny, nz]) = &c.attrs.normal {
        check_f64("normal_x", nx)?;
        check_f64("normal_y", ny)?;
        check_f64("normal_z", nz)?;
    }
    if let Some(v) = &c.attrs.intensity {
        if decoding {
            if let Some(i) = v.iter().position(|x| x.is_nan() && x.to_bits() != CANONICAL_NAN_F32) {
                return Err(Fault::NaN { column: "intensity", at: i as u64 });
            }
        }
    }
    Ok(())
}

/// Validates a chunk for encoding: at least one point, equal column lengths, strictly
/// ascending original indexes, finite coordinates and normals, no cloud-level attributes.
/// A NaN intensity is accepted and encoded as [`CANONICAL_NAN_F32`].
pub fn validate_chunk(c: &Chunk) -> Result<(), OocError> {
    let n = c.index.len();
    if n == 0 {
        return Err(chunk_fault(Fault::ZeroCount));
    }
    if !c.attrs.scan_origins.is_empty() || c.attrs.grid.is_some() {
        return Err(chunk_fault(Fault::CloudAttributeInChunk));
    }
    col_len("x", c.points.x.len(), n)?;
    col_len("y", c.points.y.len(), n)?;
    col_len("z", c.points.z.len(), n)?;
    let a = &c.attrs;
    if let Some([nx, ny, nz]) = &a.normal {
        col_len("normal", nx.len(), n)?;
        col_len("normal", ny.len(), n)?;
        col_len("normal", nz.len(), n)?;
    }
    if let Some(v) = &a.colour {
        col_len("colour", v.len(), n)?;
    }
    if let Some(v) = &a.intensity {
        col_len("intensity", v.len(), n)?;
    }
    if let Some(v) = &a.quality {
        col_len("quality", v.len(), n)?;
    }
    if let Some(v) = &a.scanner_id {
        col_len("scanner_id", v.len(), n)?;
    }
    if let Some(v) = &a.timestamp_ns {
        col_len("timestamp_ns", v.len(), n)?;
    }
    check_values(c, false).map_err(chunk_fault)
}

/// A `Vec<u8>` of exactly `n` bytes capacity, or `AllocationFailed`.
pub(crate) fn try_bytes(n: u64) -> Result<Vec<u8>, OocError> {
    let mut v = Vec::new();
    let len = usize::try_from(n).map_err(|_| OocError::AllocationFailed { needed: n })?;
    v.try_reserve_exact(len).map_err(|_| OocError::AllocationFailed { needed: n })?;
    Ok(v)
}

/// The chunk bytes (format v1) of a valid chunk. Floats are written as their raw IEEE 754
/// bits: `-0.0` stays `-0.0`; the one exception is a NaN intensity, written as
/// [`CANONICAL_NAN_F32`].
pub fn encode_chunk(c: &Chunk) -> Result<Vec<u8>, OocError> {
    validate_chunk(c)?;
    let channels = c.channels();
    let total = chunk_bytes(channels, c.len()).ok_or(OocError::AllocationFailed { needed: u64::MAX })?;
    let mut out = try_bytes(total)?;
    {
        let mut put = |b: &[u8]| out.extend_from_slice(b);
        put(&CHUNK_MAGIC);
        put(&[CHUNK_VERSION]);
        put(&c.cell.raw().to_le_bytes());
        put(&c.len().to_le_bytes());
        channels.write_list(&mut put);
    }
    for &i in &c.index {
        out.extend_from_slice(&i.to_le_bytes());
    }
    for col in [&c.points.x, &c.points.y, &c.points.z] {
        put_f64s(&mut out, col);
    }
    let a = &c.attrs;
    if let Some(v) = &a.colour {
        for rgb in v {
            out.extend_from_slice(rgb);
        }
    }
    if let Some(v) = &a.intensity {
        for x in v {
            out.extend_from_slice(&intensity_bits(*x).to_le_bytes());
        }
    }
    if let Some([nx, ny, nz]) = &a.normal {
        put_f64s(&mut out, nx);
        put_f64s(&mut out, ny);
        put_f64s(&mut out, nz);
    }
    if let Some(v) = &a.quality {
        for x in v {
            out.extend_from_slice(&x.to_le_bytes());
        }
    }
    if let Some(v) = &a.scanner_id {
        for x in v {
            out.extend_from_slice(&x.to_le_bytes());
        }
    }
    if let Some(v) = &a.timestamp_ns {
        for x in v {
            out.extend_from_slice(&x.to_le_bytes());
        }
    }
    Ok(out)
}

fn put_f64s(out: &mut Vec<u8>, v: &[f64]) {
    for x in v {
        out.extend_from_slice(&x.to_bits().to_le_bytes());
    }
}

/// A bounds-checked cursor over input bytes.
pub(crate) struct Reader<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    pub(crate) fn new(bytes: &'a [u8]) -> Reader<'a> {
        Reader { bytes, pos: 0 }
    }

    pub(crate) fn pos(&self) -> u64 {
        self.pos as u64
    }

    pub(crate) fn remaining(&self) -> u64 {
        (self.bytes.len() - self.pos.min(self.bytes.len())) as u64
    }

    pub(crate) fn take(&mut self, n: usize) -> Result<&'a [u8], Fault> {
        let end = self.pos.checked_add(n);
        match end.and_then(|e| self.bytes.get(self.pos..e)) {
            Some(s) => {
                self.pos += n;
                Ok(s)
            }
            None => Err(Fault::Truncated {
                needed: (self.pos as u64).saturating_add(n as u64),
                found: self.bytes.len() as u64,
            }),
        }
    }

    fn arr<const N: usize>(&mut self) -> Result<[u8; N], Fault> {
        let s = self.take(N)?;
        let mut a = [0u8; N];
        a.copy_from_slice(s);
        Ok(a)
    }

    pub(crate) fn u8(&mut self) -> Result<u8, Fault> {
        Ok(self.arr::<1>()?[0])
    }

    pub(crate) fn u32(&mut self) -> Result<u32, Fault> {
        Ok(u32::from_le_bytes(self.arr()?))
    }

    pub(crate) fn u64(&mut self) -> Result<u64, Fault> {
        Ok(u64::from_le_bytes(self.arr()?))
    }

    pub(crate) fn f64(&mut self) -> Result<f64, Fault> {
        Ok(f64::from_bits(self.u64()?))
    }

    pub(crate) fn digest(&mut self) -> Result<[u8; 32], Fault> {
        self.arr()
    }

    fn frame(&mut self) -> Result<[u8; 16], Fault> {
        self.arr()
    }
}

/// Reads `n` fixed-width values of a column.
fn column<T, const W: usize>(r: &mut Reader<'_>, n: usize, f: impl Fn([u8; W]) -> T) -> Result<Vec<T>, Fault> {
    let bytes = r.take(n.saturating_mul(W))?;
    let mut out = Vec::with_capacity(n);
    for c in bytes.chunks_exact(W) {
        let mut a = [0u8; W];
        a.copy_from_slice(c);
        out.push(f(a));
    }
    Ok(out)
}

fn f64_col(r: &mut Reader<'_>, n: usize) -> Result<Vec<f64>, Fault> {
    column::<f64, 8>(r, n, |a| f64::from_bits(u64::from_le_bytes(a)))
}

/// Decodes and fully validates chunk bytes (format v1). `max_bytes` bounds the input and
/// the size its header may declare; nothing is allocated before the declared size is
/// checked against both the limit and the bytes present.
pub fn decode_chunk(bytes: &[u8], max_bytes: u64) -> Result<Chunk, OocError> {
    let len = bytes.len() as u64;
    if len > max_bytes {
        return Err(chunk_fault(Fault::Oversized { needed: len, limit: max_bytes }));
    }
    let mut r = Reader::new(bytes);
    let magic = r.take(6).map_err(chunk_fault)?;
    if magic != CHUNK_MAGIC {
        return Err(chunk_fault(Fault::BadMagic));
    }
    let version = r.u8().map_err(chunk_fault)?;
    if version != CHUNK_VERSION {
        return Err(OocError::UnsupportedChunkVersion { found: version });
    }
    let raw = r.u64().map_err(chunk_fault)?;
    let cell = CellCode::from_raw(raw).ok_or(chunk_fault(Fault::BadCell { code: raw }))?;
    let count = r.u64().map_err(chunk_fault)?;
    if count == 0 {
        return Err(chunk_fault(Fault::ZeroCount));
    }
    let channels = ChannelSet::read_list(&mut r).map_err(chunk_fault)?.ok_or(chunk_fault(Fault::BadChannel))?;
    let needed = match count.checked_mul(channels.record_bytes()).and_then(|b| b.checked_add(r.pos())) {
        Some(n) if n <= max_bytes => n,
        Some(n) => return Err(chunk_fault(Fault::Oversized { needed: n, limit: max_bytes })),
        None => return Err(chunk_fault(Fault::Oversized { needed: u64::MAX, limit: max_bytes })),
    };
    if len < needed {
        return Err(chunk_fault(Fault::Truncated { needed, found: len }));
    }
    if len > needed {
        return Err(chunk_fault(Fault::TrailingBytes { extra: len - needed }));
    }
    // count * record <= len, so it fits in usize.
    let n = usize::try_from(count).map_err(|_| chunk_fault(Fault::Oversized { needed, limit: max_bytes }))?;
    let c = read_columns(&mut r, n, cell, channels).map_err(chunk_fault)?;
    check_values(&c, true).map_err(chunk_fault)?;
    Ok(c)
}

fn read_columns(r: &mut Reader<'_>, n: usize, cell: CellCode, channels: ChannelSet) -> Result<Chunk, Fault> {
    let index = column::<u64, 8>(r, n, u64::from_le_bytes)?;
    let x = f64_col(r, n)?;
    let y = f64_col(r, n)?;
    let z = f64_col(r, n)?;
    let mut attrs = AttributeSet::default();
    for ch in channels.iter() {
        match ch {
            Channel::Colour => attrs.colour = Some(column::<[u8; 3], 3>(r, n, |a| a)?),
            Channel::Intensity => {
                attrs.intensity = Some(column::<f32, 4>(r, n, |a| f32::from_bits(u32::from_le_bytes(a)))?)
            }
            Channel::Normal => {
                let nx = f64_col(r, n)?;
                let ny = f64_col(r, n)?;
                let nz = f64_col(r, n)?;
                attrs.normal = Some([nx, ny, nz]);
            }
            Channel::Quality => attrs.quality = Some(column::<u16, 2>(r, n, u16::from_le_bytes)?),
            Channel::ScannerId => attrs.scanner_id = Some(column::<u32, 4>(r, n, u32::from_le_bytes)?),
            Channel::TimestampNs => attrs.timestamp_ns = Some(column::<i64, 8>(r, n, i64::from_le_bytes)?),
        }
    }
    Ok(Chunk { cell, index, points: PointBuffer::new(x, y, z), attrs })
}

// ------------------------------------------------------------------ manifest

/// One chunk of a manifest.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChunkEntry {
    /// The octree cell.
    pub cell: CellCode,
    /// Points in the chunk (at least 1).
    pub count: u64,
    /// Byte length of the chunk blob.
    pub bytes: u64,
    /// The blob's address in the content-addressed store ([`crate::blob_address`]).
    pub blob: [u8; 32],
}

/// Everything that names a chunked cloud: frame, partition bounds and parameters,
/// cloud-level attributes, the chunks in Morton order, the original element digest, and
/// the manifest digest (SHA-256 of [`ChunkManifest::to_bytes`]).
#[derive(Debug, Clone, PartialEq)]
pub struct ChunkManifest {
    pub frame: FrameRef,
    /// The bounds the partition quantises (the cloud's bounding box for
    /// [`crate::to_chunks`]).
    pub aabb: Aabb,
    pub max_chunk_points: u64,
    /// Points in the cloud.
    pub n_points: u64,
    /// Per-point channels every chunk carries.
    pub channels: ChannelSet,
    /// Cloud-level scan origins, ascending by scanner ID.
    pub scan_origins: Vec<(u32, [f64; 3])>,
    /// Cloud-level organised grid shape.
    pub grid: Option<(u64, u64)>,
    /// Chunks in Morton order.
    pub chunks: Vec<ChunkEntry>,
    /// GEO.TYPES.DIGEST of the original cloud.
    pub original_digest: ElementDigest,
    /// SHA-256 of the manifest bytes.
    pub manifest_digest: [u8; 32],
}

impl ChunkManifest {
    /// Streams the manifest bytes (format v1) to `out`.
    pub fn write_bytes(&self, out: &mut dyn FnMut(&[u8])) {
        out(&MANIFEST_MAGIC);
        out(&[MANIFEST_VERSION]);
        out(&self.frame.to_bytes());
        for v in self.aabb.min().into_iter().chain(self.aabb.max()) {
            out(&v.to_bits().to_le_bytes());
        }
        out(&self.max_chunk_points.to_le_bytes());
        out(&[MAX_DEPTH]);
        out(&self.n_points.to_le_bytes());
        self.channels.write_list(out);
        out(&(self.scan_origins.len() as u64).to_le_bytes());
        for (id, o) in &self.scan_origins {
            out(&id.to_le_bytes());
            for v in o {
                out(&v.to_bits().to_le_bytes());
            }
        }
        match self.grid {
            None => out(&[0]),
            Some((rows, cols)) => {
                out(&[1]);
                out(&rows.to_le_bytes());
                out(&cols.to_le_bytes());
            }
        }
        out(&self.original_digest.0);
        out(&(self.chunks.len() as u64).to_le_bytes());
        for e in &self.chunks {
            out(&e.cell.raw().to_le_bytes());
            out(&e.count.to_le_bytes());
            out(&e.bytes.to_le_bytes());
            out(&e.blob);
        }
    }

    /// The manifest bytes (format v1).
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut v = Vec::new();
        self.write_bytes(&mut |b: &[u8]| v.extend_from_slice(b));
        v
    }

    /// SHA-256 of the manifest bytes, streamed (no copy).
    pub fn compute_digest(&self) -> [u8; 32] {
        let mut h = Sha256::new();
        self.write_bytes(&mut |b: &[u8]| h.update(b));
        h.finish()
    }

    /// Sets [`ChunkManifest::manifest_digest`] from the other fields.
    pub fn seal(&mut self) {
        self.manifest_digest = self.compute_digest();
    }

    /// The largest chunk blob, in bytes.
    pub fn max_chunk_bytes(&self) -> u64 {
        self.chunks.iter().map(|e| e.bytes).max().unwrap_or(0)
    }

    /// Checks every field: parameters, bounds, scan origins, grid, chunk entries (valid
    /// cells in strictly ascending Morton order without overlap, counts at least 1 and at
    /// most `max_chunk_points` unless at the depth limit, byte lengths as the format
    /// implies, counts adding up to `n_points`) and the manifest digest. Allocates nothing.
    pub fn validate(&self) -> Result<(), OocError> {
        if self.max_chunk_points == 0 || self.max_chunk_points > MAX_CHUNK_POINTS_LIMIT {
            return Err(manifest_fault(Fault::BadParams));
        }
        if Grid::new(&self.aabb).is_err() {
            return Err(manifest_fault(Fault::BadBounds));
        }
        let mut prev: Option<u32> = None;
        for (id, o) in &self.scan_origins {
            if prev.is_some_and(|p| *id <= p) || !o.iter().all(|v| v.is_finite()) {
                return Err(manifest_fault(Fault::BadScanOrigins));
            }
            prev = Some(*id);
        }
        if let Some((rows, cols)) = self.grid {
            if rows.checked_mul(cols) != Some(self.n_points) {
                return Err(manifest_fault(Fault::BadGrid));
            }
        }
        if self.chunks.len() as u64 > MAX_MANIFEST_CHUNKS {
            return Err(manifest_fault(Fault::Oversized {
                needed: self.chunks.len() as u64,
                limit: MAX_MANIFEST_CHUNKS,
            }));
        }
        let mut total: u64 = 0;
        let mut end: u64 = 0;
        for (e, at) in self.chunks.iter().zip(0u64..) {
            if at > 0 && e.cell.range_start() < end {
                return Err(manifest_fault(Fault::ChunkOrder { at }));
            }
            end = e.cell.range_end();
            if e.count == 0 {
                return Err(manifest_fault(Fault::ZeroCount));
            }
            if e.count > self.max_chunk_points && e.cell.depth() < MAX_DEPTH {
                return Err(manifest_fault(Fault::OverfullChunk { at }));
            }
            let expected = chunk_bytes(self.channels, e.count).unwrap_or(u64::MAX);
            if e.bytes != expected {
                return Err(manifest_fault(Fault::WrongSize { expected, found: e.bytes }));
            }
            total = total.checked_add(e.count).ok_or(manifest_fault(Fault::CountMismatch))?;
        }
        if total != self.n_points {
            return Err(manifest_fault(Fault::CountMismatch));
        }
        if self.compute_digest() != self.manifest_digest {
            return Err(manifest_fault(Fault::DigestMismatch));
        }
        Ok(())
    }

    /// Decodes and validates manifest bytes (format v1); `max_bytes` bounds the input.
    /// The manifest digest is the SHA-256 of `bytes`.
    pub fn from_bytes(bytes: &[u8], max_bytes: u64) -> Result<ChunkManifest, OocError> {
        let len = bytes.len() as u64;
        if len > max_bytes {
            return Err(manifest_fault(Fault::Oversized { needed: len, limit: max_bytes }));
        }
        let mut r = Reader::new(bytes);
        let magic = r.take(6).map_err(manifest_fault)?;
        if magic != MANIFEST_MAGIC {
            return Err(manifest_fault(Fault::BadMagic));
        }
        let version = r.u8().map_err(manifest_fault)?;
        if version != MANIFEST_VERSION {
            return Err(OocError::UnsupportedManifestVersion { found: version });
        }
        let m = read_manifest(&mut r).map_err(manifest_fault)?;
        if r.remaining() != 0 {
            return Err(manifest_fault(Fault::TrailingBytes { extra: r.remaining() }));
        }
        let mut h = Sha256::new();
        h.update(bytes);
        let m = ChunkManifest { manifest_digest: h.finish(), ..m };
        m.validate()?;
        Ok(m)
    }
}

/// Reads the fields after the version byte.
fn read_manifest(r: &mut Reader<'_>) -> Result<ChunkManifest, Fault> {
    let frame = FrameRef::from_bytes(r.frame()?);
    let mut corners = [0.0f64; 6];
    for v in &mut corners {
        *v = r.f64()?;
    }
    let max_chunk_points = r.u64()?;
    let depth = r.u8()?;
    if depth != MAX_DEPTH {
        return Err(Fault::BadParams);
    }
    let n_points = r.u64()?;
    let Some(channels) = ChannelSet::read_list(r)? else { return Err(Fault::BadChannel) };
    let n_origins = r.u64()?;
    let want = n_origins.saturating_mul(ORIGIN_BYTES);
    if want > r.remaining() {
        return Err(Fault::Truncated { needed: r.pos().saturating_add(want), found: r.pos() + r.remaining() });
    }
    let mut scan_origins = Vec::with_capacity(n_origins as usize);
    for _ in 0..n_origins {
        let id = r.u32()?;
        scan_origins.push((id, [r.f64()?, r.f64()?, r.f64()?]));
    }
    let grid = match r.u8()? {
        0 => None,
        1 => Some((r.u64()?, r.u64()?)),
        _ => return Err(Fault::BadGrid),
    };
    let original_digest = ElementDigest(r.digest()?);
    let n_chunks = r.u64()?;
    if n_chunks > MAX_MANIFEST_CHUNKS {
        return Err(Fault::Oversized { needed: n_chunks, limit: MAX_MANIFEST_CHUNKS });
    }
    let want = n_chunks * ENTRY_BYTES;
    if want > r.remaining() {
        return Err(Fault::Truncated { needed: r.pos() + want, found: r.pos() + r.remaining() });
    }
    let mut chunks = Vec::with_capacity(n_chunks as usize);
    for _ in 0..n_chunks {
        let raw = r.u64()?;
        let cell = CellCode::from_raw(raw).ok_or(Fault::BadCell { code: raw })?;
        let count = r.u64()?;
        let bytes = r.u64()?;
        let blob = r.digest()?;
        chunks.push(ChunkEntry { cell, count, bytes, blob });
    }
    let [a, b, c, d, e, f] = corners;
    let aabb = match Aabb::new([a, b, c], [d, e, f]) {
        Ok(x) => x,
        Err(_) => return Err(Fault::BadBounds),
    };
    Ok(ChunkManifest {
        frame,
        aabb,
        max_chunk_points,
        n_points,
        channels,
        scan_origins,
        grid,
        chunks,
        original_digest,
        manifest_digest: [0; 32],
    })
}
