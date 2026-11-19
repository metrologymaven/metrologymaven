//! The streaming chunk writer: builds a chunked cloud from points fed in original order,
//! with memory bounded by a configured spill budget (REQ-GEO.OOC.LAYOUT-004).
//!
//! # Algorithm
//!
//! * **Push.** Each point is validated, written as one fixed-size record (the chunk record:
//!   original index, x, y, z, channels) to an in-memory buffer tagged with its bucket (the
//!   cell at depth 3, 512 buckets), and its canonical layout values are appended to one
//!   column file per GEO.TYPES.DIGEST column. A full buffer is counting-sorted by bucket
//!   (stable, so original order is kept) and appended to the bucket files.
//! * **Finish.** The original element digest is the SHA-256 of the GEO.TYPES.DIGEST
//!   layout, streamed from the column files (the point count comes first in the layout,
//!   so the columns are spilled rather than hashed as the points arrive). Then the octree
//!   is walked from the root in Morton order with the same split rule as
//!   [`crate::to_chunks`]: a cell whose points fit the load capacity is read into memory
//!   and partitioned there; a larger cell is split, either by bucket ranges (depth below 3)
//!   or by one streaming pass that distributes its file into eight child files.
//!
//! Every chunk is therefore built from exactly the points of its cell, ordered by original
//! index, so its bytes, and the manifest, equal those of [`crate::to_chunks`] for the same
//! cloud and bounds.
//!
//! # Memory
//!
//! The budget `B` is split into a fixed reserve ([`FIXED_OVERHEAD`]: the blob store's
//! 8 MiB put buffer, file buffers of 64 KiB, bucket table) and a working part
//! `W = B - FIXED_OVERHEAD`. One sixteenth of `W` holds the manifest entries; the rest
//! holds either the push buffer (`R + 6` bytes per record, `R` the record size) or, while
//! finishing, one loaded cell (`3 R + 40` bytes per point: records, sort keys, a leaf's
//! index list, the decoded chunk and its bytes). The writer refuses a budget whose load
//! capacity is below `max_chunk_points` ([`OocError::BudgetTooSmall`]); a cell at the depth
//! limit with more points than the load capacity fails with
//! [`OocError::BudgetExceeded`].

use crate::cell::{CellCode, Grid, MAX_DEPTH};
use crate::error::{spill, OocError};
use crate::format::{
    encode_chunk, Channel, ChannelSet, Chunk, ChunkEntry, ChunkManifest, ENTRY_BYTES, MAX_MANIFEST_CHUNKS,
};
use crate::layout::{check_params, for_each_leaf, push_entry, Key, DEFAULT_MAX_CHUNK_POINTS};
use crate::store::ChunkStore;
use fid_geo_types::digest::{
    canonical_f32_bits, canonical_f64_bits, kind_byte, ChannelType, LAYOUT_TAG, LAYOUT_VERSION,
};
use fid_geo_types::{
    is_unit_normal, Aabb, AttributeSet, ElementDigest, ElementKind, FrameRef, GeoTypeError, PointBuffer,
};
use fid_kci::kci::Sha256;
use std::fs::{self, File, OpenOptions};
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};

/// Default spill budget: 1 GiB [unverified as sufficient for every cloud shape].
pub const DEFAULT_SPILL_BUDGET: u64 = 1 << 30;
/// The fixed part of the budget: the blob store's put buffer, file buffers and tables.
pub const FIXED_OVERHEAD: u64 = 10 * 1024 * 1024;
/// File buffer size.
const IO_BUF: usize = 64 * 1024;
/// Depth of the bucket cells.
const BUCKET_DEPTH: u8 = 3;
/// Number of buckets (8^3).
const BUCKETS: usize = 512;
/// A Morton code shifted right by this gives its bucket.
const BUCKET_SHIFT: u32 = 54;
/// Working bytes per buffered record beyond the record: bucket tag (u16) and sort slot (u32).
const PUSH_EXTRA: u64 = 6;
/// Working bytes per loaded point beyond three records: sort key (24) and leaf list (16).
const LOAD_EXTRA: u64 = 40;
/// Bytes charged per manifest entry; at least `size_of::<ChunkEntry>()` (tested).
pub const ENTRY_COST: u64 = ENTRY_BYTES;
/// Longest record: index, x, y, z and every channel.
const MAX_RECORD: usize = 32 + 3 + 4 + 24 + 2 + 4 + 8;

/// Parameters of a [`ChunkWriter`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WriterConfig {
    /// The split threshold, as [`crate::LayoutParams::max_chunk_points`].
    pub max_chunk_points: u64,
    /// Bytes of memory the writer may hold at once.
    pub spill_budget: u64,
}

impl Default for WriterConfig {
    fn default() -> WriterConfig {
        WriterConfig { max_chunk_points: DEFAULT_MAX_CHUNK_POINTS, spill_budget: DEFAULT_SPILL_BUDGET }
    }
}

/// What the writer must know before the first point: frame, partition bounds (from the
/// file header or a first pass), the per-point channels, and the cloud-level attributes.
#[derive(Debug, Clone, PartialEq)]
pub struct CloudHeader {
    pub frame: FrameRef,
    pub bounds: Aabb,
    pub channels: ChannelSet,
    pub scan_origins: Vec<(u32, [f64; 3])>,
    pub grid: Option<(u64, u64)>,
}

/// The attributes of one pushed point; exactly the header's channels must be present.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct PointAttrs {
    pub normal: Option<[f64; 3]>,
    pub intensity: Option<f32>,
    pub colour: Option<[u8; 3]>,
    pub scanner_id: Option<u32>,
    pub timestamp_ns: Option<i64>,
    pub quality: Option<u16>,
}

impl PointAttrs {
    fn channels(&self) -> ChannelSet {
        let mut s = ChannelSet::EMPTY;
        let present = [
            (Channel::Colour, self.colour.is_some()),
            (Channel::Intensity, self.intensity.is_some()),
            (Channel::Normal, self.normal.is_some()),
            (Channel::Quality, self.quality.is_some()),
            (Channel::ScannerId, self.scanner_id.is_some()),
            (Channel::TimestampNs, self.timestamp_ns.is_some()),
        ];
        for (c, p) in present {
            if p {
                s = s.with(c);
            }
        }
        s
    }
}

/// Counters of a writer run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct WriterStats {
    /// Points pushed.
    pub points: u64,
    /// Largest number of working bytes the writer accounted at once (buffers, loaded
    /// cells, manifest entries; the fixed reserve not included).
    pub peak_working_bytes: u64,
    /// Record bytes written to bucket files.
    pub spilled_bytes: u64,
    /// Cells split by a streaming pass over their file.
    pub external_splits: u64,
    /// Chunks written.
    pub chunks: u64,
}

/// Where the points of a cell are while finishing.
enum Src {
    /// Bucket files `lo..hi` (cells above the bucket depth).
    Buckets(usize, usize),
    /// One file (a bucket, or a child file of a streaming split).
    File(PathBuf),
}

/// Streaming builder of a chunked cloud. Create it with [`ChunkWriter::new`], feed every
/// point in original order with [`ChunkWriter::push`], then call [`ChunkWriter::finish`].
/// The spill directory is created by `new` and removed when the writer is dropped.
pub struct ChunkWriter<'s> {
    store: &'s dyn ChunkStore,
    dir: PathBuf,
    header: CloudHeader,
    grid: Grid,
    max: u64,
    rec: usize,
    push_cap: usize,
    load_cap: u64,
    entry_cap: u64,
    n: u64,
    buf: Vec<u8>,
    tags: Vec<u16>,
    perm: Vec<u32>,
    bucket_counts: Vec<u64>,
    columns: Vec<(PathBuf, u64, BufWriter<File>)>,
    stats: WriterStats,
    working: u64,
    next_file: u64,
    failed: bool,
}

/// The working-set split of a budget: (push records, load points, manifest entries).
fn capacities(budget: u64, rec: u64, max: u64) -> Result<(u64, u64, u64), OocError> {
    let per_load = 3 * rec + LOAD_EXTRA;
    let min_work = max.saturating_mul(per_load).max(16 * ENTRY_COST);
    let needed = FIXED_OVERHEAD.saturating_add(min_work.saturating_mul(16) / 15).saturating_add(64);
    if budget < needed {
        return Err(OocError::BudgetTooSmall { needed });
    }
    let w = budget - FIXED_OVERHEAD;
    let entries = w / 16;
    let work = w - entries;
    let push = work / (rec + PUSH_EXTRA);
    let load = work / per_load;
    let entry_cap = (entries / ENTRY_COST).min(MAX_MANIFEST_CHUNKS);
    if load < max || push == 0 || entry_cap == 0 {
        return Err(OocError::BudgetTooSmall { needed });
    }
    Ok((push, load, entry_cap))
}

fn open_append(path: &Path) -> Result<BufWriter<File>, OocError> {
    let f = OpenOptions::new().create(true).append(true).open(path).map_err(spill)?;
    Ok(BufWriter::with_capacity(IO_BUF, f))
}

fn read_u64(b: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(field(b, at))
}

fn read_f64(b: &[u8], at: usize) -> f64 {
    f64::from_bits(read_u64(b, at))
}

fn morton_of_record(grid: &Grid, r: &[u8]) -> u64 {
    grid.morton([read_f64(r, 8), read_f64(r, 16), read_f64(r, 24)])
}

impl<'s> ChunkWriter<'s> {
    /// A writer that stores chunks in `store` and spills to `spill_dir`, which it creates
    /// (its parent must exist; it must not exist yet). Fails on invalid parameters, bounds
    /// whose extent overflows, invalid scan origins, or a budget below the minimum.
    pub fn new(
        store: &'s dyn ChunkStore,
        spill_dir: &Path,
        header: CloudHeader,
        config: WriterConfig,
    ) -> Result<ChunkWriter<'s>, OocError> {
        check_params(config.max_chunk_points)?;
        let grid = Grid::new(&header.bounds)?;
        let mut prev: Option<u32> = None;
        for &(id, o) in &header.scan_origins {
            if prev.is_some_and(|p| id <= p) || !o.iter().all(|v| v.is_finite()) {
                return Err(OocError::Geometry(GeoTypeError::ScanOrigin { scanner_id: id }));
            }
            prev = Some(id);
        }
        let rec = header.channels.record_bytes();
        let (push, load, entry_cap) = capacities(config.spill_budget, rec, config.max_chunk_points)?;
        let push_cap = usize::try_from(push).map_err(|_| OocError::AllocationFailed { needed: push })?;
        let rec_usize = rec as usize;
        let mut buf = Vec::new();
        buf.try_reserve_exact(push_cap.saturating_mul(rec_usize))
            .map_err(|_| OocError::AllocationFailed { needed: push * rec })?;
        let mut tags = Vec::new();
        tags.try_reserve_exact(push_cap).map_err(|_| OocError::AllocationFailed { needed: push * 2 })?;
        let mut perm = Vec::new();
        perm.try_reserve_exact(push_cap).map_err(|_| OocError::AllocationFailed { needed: push * 4 })?;
        fs::create_dir(spill_dir).map_err(spill)?;
        let mut w = ChunkWriter {
            store,
            dir: spill_dir.to_path_buf(),
            grid,
            max: config.max_chunk_points,
            rec: rec_usize,
            push_cap,
            load_cap: load,
            entry_cap,
            n: 0,
            buf,
            tags,
            perm,
            bucket_counts: vec![0; BUCKETS],
            columns: Vec::new(),
            stats: WriterStats::default(),
            working: 0,
            next_file: 0,
            failed: false,
            header,
        };
        let mut names: Vec<&str> = vec!["x", "y", "z"];
        for c in w.header.channels.iter() {
            match c {
                Channel::Normal => names.extend(["normal_x", "normal_y", "normal_z"]),
                c => names.push(c.name()),
            }
        }
        for name in names {
            let path = w.dir.join(format!("col_{name}"));
            let f = open_append(&path)?;
            w.columns.push((path, 0, f));
        }
        w.note(w.push_cap as u64 * (rec + PUSH_EXTRA));
        Ok(w)
    }

    /// Records the current working size and the peak.
    fn note(&mut self, bytes: u64) {
        self.working = bytes;
        self.stats.peak_working_bytes = self.stats.peak_working_bytes.max(bytes);
    }

    /// Counters so far.
    pub fn stats(&self) -> WriterStats {
        WriterStats { points: self.n, ..self.stats }
    }

    /// Adds the next point (original index = number of points pushed before). Validates it
    /// as the element constructors do (finite coordinates, unit normals within 1e-9) and
    /// refuses a NaN intensity, a point outside the bounds, or a channel set different
    /// from the header's. A validation error leaves the writer usable; an I/O error does
    /// not.
    pub fn push(&mut self, p: [f64; 3], a: &PointAttrs) -> Result<(), OocError> {
        if self.failed {
            return Err(OocError::WriterFailed);
        }
        let index = self.n;
        if a.channels() != self.header.channels {
            return Err(OocError::ChannelMismatch { index });
        }
        if !p.iter().all(|v| v.is_finite()) {
            return Err(OocError::Geometry(GeoTypeError::NonFinite { index }));
        }
        if let Some(nrm) = a.normal {
            if !is_unit_normal(nrm) {
                return Err(OocError::Geometry(GeoTypeError::BadNormal { index }));
            }
        }
        if a.intensity.is_some_and(f32::is_nan) {
            return Err(OocError::NaNValue { channel: "intensity", index });
        }
        if !self.grid.inside(p) {
            return Err(OocError::OutOfBounds { index });
        }
        let r = self.write_point(index, p, a);
        if r.is_err() {
            self.failed = true;
        }
        r?;
        self.n += 1;
        Ok(())
    }

    fn write_point(&mut self, index: u64, p: [f64; 3], a: &PointAttrs) -> Result<(), OocError> {
        let bucket = (self.grid.morton(p) >> BUCKET_SHIFT) as u16;
        let mut rec = [0u8; MAX_RECORD];
        let mut len = 0usize;
        let mut put = |b: &[u8]| {
            if let Some(s) = rec.get_mut(len..len + b.len()) {
                s.copy_from_slice(b);
            }
            len += b.len();
        };
        put(&index.to_le_bytes());
        for v in p {
            put(&v.to_bits().to_le_bytes());
        }
        // Column values in canonical layout form (GEO.TYPES.DIGEST), in column order.
        let mut col_vals: [([u8; 8], usize); 11] = [([0; 8], 0); 11];
        let mut k = 0usize;
        let mut col = |b: &[u8]| {
            if let Some(slot) = col_vals.get_mut(k) {
                if let Some(s) = slot.0.get_mut(..b.len()) {
                    s.copy_from_slice(b);
                }
                slot.1 = b.len();
            }
            k += 1;
        };
        for v in p {
            col(&canonical_f64_bits(v).to_le_bytes());
        }
        for c in self.header.channels.iter() {
            match c {
                Channel::Colour => {
                    let v = a.colour.unwrap_or_default();
                    put(&v);
                    col(&v);
                }
                Channel::Intensity => {
                    let v = a.intensity.unwrap_or_default();
                    put(&v.to_bits().to_le_bytes());
                    col(&canonical_f32_bits(v).to_le_bytes());
                }
                Channel::Normal => {
                    for v in a.normal.unwrap_or_default() {
                        put(&v.to_bits().to_le_bytes());
                        col(&canonical_f64_bits(v).to_le_bytes());
                    }
                }
                Channel::Quality => {
                    let v = a.quality.unwrap_or_default().to_le_bytes();
                    put(&v);
                    col(&v);
                }
                Channel::ScannerId => {
                    let v = a.scanner_id.unwrap_or_default().to_le_bytes();
                    put(&v);
                    col(&v);
                }
                Channel::TimestampNs => {
                    let v = a.timestamp_ns.unwrap_or_default().to_le_bytes();
                    put(&v);
                    col(&v);
                }
            }
        }
        for ((_, written, w), (bytes, n)) in self.columns.iter_mut().zip(col_vals.iter()) {
            w.write_all(bytes.get(..*n).unwrap_or(&[])).map_err(spill)?;
            *written += *n as u64;
        }
        self.buf.extend_from_slice(rec.get(..self.rec).unwrap_or(&[]));
        self.tags.push(bucket);
        if self.tags.len() >= self.push_cap {
            self.flush_buckets()?;
        }
        Ok(())
    }

    fn bucket_path(&self, b: usize) -> PathBuf {
        self.dir.join(format!("b{b:03}"))
    }

    /// Appends the buffered records to their bucket files, bucket by bucket, keeping
    /// original order inside each bucket.
    fn flush_buckets(&mut self) -> Result<(), OocError> {
        let count = self.tags.len();
        if count == 0 {
            return Ok(());
        }
        let mut start = [0u32; BUCKETS + 1];
        for &t in &self.tags {
            if let Some(s) = start.get_mut(usize::from(t) + 1) {
                *s += 1;
            }
        }
        let mut sum = 0u32;
        for s in start.iter_mut() {
            sum += *s;
            *s = sum;
        }
        let mut next = start;
        self.perm.clear();
        self.perm.resize(count, 0);
        for (i, &t) in self.tags.iter().enumerate() {
            if let Some(slot) = next.get_mut(usize::from(t)) {
                if let Some(p) = self.perm.get_mut(*slot as usize) {
                    *p = i as u32;
                }
                *slot += 1;
            }
        }
        let rec = self.rec;
        for (b, win) in start.windows(2).enumerate() {
            let (lo, hi) = (win[0] as usize, win[1] as usize);
            if lo == hi {
                continue;
            }
            let mut w = open_append(&self.bucket_path(b))?;
            for &i in self.perm.get(lo..hi).unwrap_or(&[]) {
                let at = i as usize * rec;
                w.write_all(self.buf.get(at..at + rec).unwrap_or(&[])).map_err(spill)?;
            }
            w.flush().map_err(spill)?;
            let n = (hi - lo) as u64;
            if let Some(c) = self.bucket_counts.get_mut(b) {
                *c += n;
            }
            self.stats.spilled_bytes += n * rec as u64;
        }
        self.buf.clear();
        self.tags.clear();
        Ok(())
    }

    /// Flushes everything, computes the original digest, writes every chunk to the store
    /// and returns the sealed manifest with the run's counters.
    pub fn finish(mut self) -> Result<(ChunkManifest, WriterStats), OocError> {
        if self.failed {
            return Err(OocError::WriterFailed);
        }
        let m = self.finish_inner()?;
        Ok((m, self.stats()))
    }

    fn finish_inner(&mut self) -> Result<ChunkManifest, OocError> {
        self.flush_buckets()?;
        self.buf = Vec::new();
        self.tags = Vec::new();
        self.perm = Vec::new();
        self.note(0);
        let mut cols = Vec::new();
        for (path, written, w) in self.columns.drain(..) {
            w.into_inner().map_err(|e| spill(e.into_error()))?;
            cols.push((path, written));
        }
        let n = self.n;
        if n == 0 {
            return Err(OocError::EmptyCloud);
        }
        if let Some((rows, cols)) = self.header.grid {
            if rows.checked_mul(cols) != Some(n) {
                return Err(OocError::Geometry(GeoTypeError::GridShape { rows, cols, count: n }));
            }
        }
        let original_digest = self.original_digest(&cols)?;
        for (path, _) in &cols {
            fs::remove_file(path).map_err(spill)?;
        }
        let entries = self.partition()?;
        let mut m = ChunkManifest {
            frame: self.header.frame,
            aabb: self.header.bounds,
            max_chunk_points: self.max,
            n_points: n,
            channels: self.header.channels,
            scan_origins: self.header.scan_origins.clone(),
            grid: self.header.grid,
            chunks: entries,
            original_digest,
            manifest_digest: [0; 32],
        };
        m.seal();
        Ok(m)
    }

    /// GEO.TYPES.DIGEST layout v1 of the cloud, streamed from the column files.
    fn original_digest(&self, cols: &[(PathBuf, u64)]) -> Result<ElementDigest, OocError> {
        let n = self.n;
        let h = &self.header;
        let mut sha = Sha256::new();
        sha.update(&LAYOUT_TAG);
        sha.update(&[LAYOUT_VERSION, kind_byte(ElementKind::PointCloud)]);
        sha.update(&h.frame.to_bytes());
        sha.update(&n.to_le_bytes());
        let mut block = vec![0u8; IO_BUF];
        let mut files = cols.iter();
        let mut stream = |sha: &mut Sha256, files: &mut core::slice::Iter<'_, (PathBuf, u64)>, width: u64| {
            let Some((path, written)) = files.next() else {
                return Err(OocError::WriterFailed);
            };
            if *written != n.saturating_mul(width) {
                return Err(OocError::WriterFailed);
            }
            let mut f = File::open(path).map_err(spill)?;
            let mut total = 0u64;
            loop {
                let got = f.read(&mut block).map_err(spill)?;
                if got == 0 {
                    break;
                }
                sha.update(block.get(..got).unwrap_or(&[]));
                total += got as u64;
            }
            if total != *written {
                return Err(OocError::WriterFailed);
            }
            Ok(())
        };
        for _ in 0..3 {
            stream(&mut sha, &mut files, 8)?;
        }
        sha.update(&0u64.to_le_bytes());
        let ch = h.channels;
        let n_channels = ch.iter().count() as u64 + u64::from(h.grid.is_some()) + u64::from(!h.scan_origins.is_empty());
        sha.update(&n_channels.to_le_bytes());
        let head = |sha: &mut Sha256, name: &str, ty: ChannelType, records: u64| {
            sha.update(&(name.len() as u16).to_le_bytes());
            sha.update(name.as_bytes());
            sha.update(&[ty.code()]);
            sha.update(&records.to_le_bytes());
        };
        // Channels in ascending name order, as GEO.TYPES.DIGEST writes them.
        if ch.contains(Channel::Colour) {
            head(&mut sha, "colour", ChannelType::U8x3, n);
            stream(&mut sha, &mut files, 3)?;
        }
        if let Some((rows, cols)) = h.grid {
            head(&mut sha, "grid", ChannelType::U64, 2);
            sha.update(&rows.to_le_bytes());
            sha.update(&cols.to_le_bytes());
        }
        if ch.contains(Channel::Intensity) {
            head(&mut sha, "intensity", ChannelType::F32, n);
            stream(&mut sha, &mut files, 4)?;
        }
        if ch.contains(Channel::Normal) {
            head(&mut sha, "normal", ChannelType::F64x3Columns, n);
            for _ in 0..3 {
                stream(&mut sha, &mut files, 8)?;
            }
        }
        if ch.contains(Channel::Quality) {
            head(&mut sha, "quality", ChannelType::U16, n);
            stream(&mut sha, &mut files, 2)?;
        }
        if !h.scan_origins.is_empty() {
            head(&mut sha, "scan_origins", ChannelType::Origin, h.scan_origins.len() as u64);
            for (id, o) in &h.scan_origins {
                sha.update(&id.to_le_bytes());
                for v in o {
                    sha.update(&canonical_f64_bits(*v).to_le_bytes());
                }
            }
        }
        if ch.contains(Channel::ScannerId) {
            head(&mut sha, "scanner_id", ChannelType::U32, n);
            stream(&mut sha, &mut files, 4)?;
        }
        if ch.contains(Channel::TimestampNs) {
            head(&mut sha, "timestamp_ns", ChannelType::I64, n);
            stream(&mut sha, &mut files, 8)?;
        }
        Ok(ElementDigest(sha.finish()))
    }

    fn fresh_path(&mut self) -> PathBuf {
        self.next_file += 1;
        self.dir.join(format!("s{}", self.next_file))
    }

    fn bucket_range_count(&self, lo: usize, hi: usize) -> u64 {
        self.bucket_counts.get(lo..hi).map_or(0, |c| c.iter().sum())
    }

    /// Walks the octree in Morton order and writes every chunk.
    fn partition(&mut self) -> Result<Vec<ChunkEntry>, OocError> {
        let mut entries: Vec<ChunkEntry> = Vec::new();
        let mut stack: Vec<(CellCode, Src, u64)> = vec![(CellCode::ROOT, Src::Buckets(0, BUCKETS), self.n)];
        while let Some((cell, src, count)) = stack.pop() {
            if count == 0 {
                if let Src::File(p) = &src {
                    remove_if_present(p)?;
                }
                continue;
            }
            if count <= self.load_cap {
                let table = self.load(&src, count)?;
                if let Src::File(p) = &src {
                    remove_if_present(p)?;
                }
                self.chunks_of(cell, &table, &mut entries)?;
                continue;
            }
            if cell.depth() >= MAX_DEPTH {
                let per = 3 * self.rec as u64 + LOAD_EXTRA;
                return Err(OocError::BudgetExceeded {
                    needed: FIXED_OVERHEAD.saturating_add(count.saturating_mul(per)),
                });
            }
            let mut children: Vec<(CellCode, Src, u64)> = Vec::with_capacity(8);
            match src {
                Src::Buckets(lo, hi) => {
                    let step = (hi - lo) / 8;
                    for o in 0..8u8 {
                        let Some(ch) = cell.child(o) else { continue };
                        let (a, b) = (lo + usize::from(o) * step, lo + usize::from(o + 1) * step);
                        let c = self.bucket_range_count(a, b);
                        let s = if ch.depth() >= BUCKET_DEPTH {
                            Src::File(self.bucket_path(a))
                        } else {
                            Src::Buckets(a, b)
                        };
                        children.push((ch, s, c));
                    }
                }
                Src::File(path) => {
                    let (paths, counts) = self.split_file(cell, &path)?;
                    remove_if_present(&path)?;
                    for (o, (p, c)) in paths.into_iter().zip(counts).enumerate() {
                        let Some(ch) = cell.child(o as u8) else { continue };
                        children.push((ch, Src::File(p), c));
                    }
                }
            }
            while let Some(c) = children.pop() {
                stack.push(c);
            }
        }
        Ok(entries)
    }

    /// Distributes the records of `path` (all in `cell`) into eight child files, keeping
    /// their order.
    fn split_file(&mut self, cell: CellCode, path: &Path) -> Result<(Vec<PathBuf>, [u64; 8]), OocError> {
        self.stats.external_splits += 1;
        let shift = 3 * u32::from(MAX_DEPTH - cell.depth() - 1);
        let mut paths = Vec::with_capacity(8);
        let mut writers = Vec::with_capacity(8);
        for _ in 0..8 {
            let p = self.fresh_path();
            writers.push(open_append(&p)?);
            paths.push(p);
        }
        let mut counts = [0u64; 8];
        let mut reader = BufReader::with_capacity(IO_BUF, File::open(path).map_err(spill)?);
        let mut rec = [0u8; MAX_RECORD];
        let r = rec.get_mut(..self.rec).ok_or(OocError::WriterFailed)?;
        loop {
            match read_record(&mut reader, r)? {
                false => break,
                true => {
                    let o = ((morton_of_record(&self.grid, r) >> shift) & 7) as usize;
                    if let (Some(w), Some(c)) = (writers.get_mut(o), counts.get_mut(o)) {
                        w.write_all(r).map_err(spill)?;
                        *c += 1;
                    }
                }
            }
        }
        for w in writers {
            w.into_inner().map_err(|e| spill(e.into_error()))?;
        }
        Ok((paths, counts))
    }

    /// Reads the `count` records of a cell into memory.
    fn load(&mut self, src: &Src, count: u64) -> Result<Vec<u8>, OocError> {
        let rec = self.rec as u64;
        let total = count.saturating_mul(rec);
        let len = usize::try_from(total).map_err(|_| OocError::AllocationFailed { needed: total })?;
        let mut table = Vec::new();
        table.try_reserve_exact(len).map_err(|_| OocError::AllocationFailed { needed: total })?;
        table.resize(len, 0);
        let paths: Vec<PathBuf> = match src {
            Src::Buckets(lo, hi) => (*lo..*hi)
                .filter(|&b| self.bucket_counts.get(b).is_some_and(|&c| c > 0))
                .map(|b| self.bucket_path(b))
                .collect(),
            Src::File(p) => vec![p.clone()],
        };
        let mut at = 0usize;
        for p in &paths {
            let mut f = File::open(p).map_err(spill)?;
            let size = f.metadata().map_err(spill)?.len();
            let size = usize::try_from(size).map_err(|_| OocError::WriterFailed)?;
            let dst = table.get_mut(at..at + size).ok_or(OocError::WriterFailed)?;
            f.read_exact(dst).map_err(spill)?;
            at += size;
        }
        if at != len {
            return Err(OocError::WriterFailed);
        }
        if let Src::Buckets(..) = src {
            for p in &paths {
                remove_if_present(p)?;
            }
        }
        Ok(table)
    }

    /// Partitions a loaded cell in memory and writes its chunks.
    fn chunks_of(&mut self, cell: CellCode, table: &[u8], entries: &mut Vec<ChunkEntry>) -> Result<(), OocError> {
        let rec = self.rec;
        let count = table.len() / rec.max(1);
        let mut keys: Vec<Key> = Vec::new();
        keys.try_reserve_exact(count).map_err(|_| OocError::AllocationFailed { needed: count as u64 * 24 })?;
        for (pos, r) in table.chunks_exact(rec).enumerate() {
            keys.push((morton_of_record(&self.grid, r), read_u64(r, 0), pos));
        }
        keys.sort_unstable();
        let entries_bytes = entries.capacity() as u64 * ENTRY_COST;
        let per = 3 * rec as u64 + LOAD_EXTRA;
        self.note(entries_bytes + count as u64 * per);
        let (store, channels, max, entry_cap) = (self.store, self.header.channels, self.max, self.entry_cap);
        let mut written = 0u64;
        for_each_leaf(cell, &keys, max, &mut |leaf_cell, leaf| {
            let mut pos: Vec<(u64, usize)> = leaf.iter().map(|k| (k.1, k.2)).collect();
            pos.sort_unstable();
            let chunk = gather_records(table, rec, channels, leaf_cell, &pos)?;
            drop(pos);
            let bytes = encode_chunk(&chunk)?;
            drop(chunk);
            let blob = store.put_blob(&bytes)?;
            let e = ChunkEntry { cell: leaf_cell, count: leaf.len() as u64, bytes: bytes.len() as u64, blob };
            if entries.len() == entries.capacity() {
                let want = (entries.capacity() * 2).max(16) as u64;
                let add = want.min(entry_cap).saturating_sub(entries.len() as u64);
                entries.try_reserve_exact(add as usize).map_err(|_| OocError::AllocationFailed { needed: add })?;
            }
            push_entry(entries, e, entry_cap)?;
            written += 1;
            Ok(())
        })?;
        self.stats.chunks += written;
        Ok(())
    }
}

/// Reads one record; `false` at a clean end of file.
fn read_record(r: &mut impl Read, buf: &mut [u8]) -> Result<bool, OocError> {
    let mut got = 0usize;
    while got < buf.len() {
        let Some(dst) = buf.get_mut(got..) else { break };
        let k = r.read(dst).map_err(spill)?;
        if k == 0 {
            if got == 0 {
                return Ok(false);
            }
            return Err(OocError::WriterFailed);
        }
        got += k;
    }
    Ok(true)
}

fn remove_if_present(p: &Path) -> Result<(), OocError> {
    match fs::remove_file(p) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(spill(e)),
    }
}

/// `N` bytes of `r` at `at` (zeros past the end, which a checked table never reaches).
fn field<const N: usize>(r: &[u8], at: usize) -> [u8; N] {
    let mut a = [0u8; N];
    if let Some(s) = r.get(at..at + N) {
        a.copy_from_slice(s);
    }
    a
}

fn push_to<T>(col: &mut Option<Vec<T>>, v: T) {
    if let Some(c) = col.as_mut() {
        c.push(v);
    }
}

/// The chunk of `cell` made of the records at `pos` (sorted by original index).
fn gather_records(
    table: &[u8],
    rec: usize,
    channels: ChannelSet,
    cell: CellCode,
    pos: &[(u64, usize)],
) -> Result<Chunk, OocError> {
    let n = pos.len();
    let mut index = Vec::with_capacity(n);
    let (mut x, mut y, mut z) = (Vec::with_capacity(n), Vec::with_capacity(n), Vec::with_capacity(n));
    let mut attrs = AttributeSet::default();
    let on = |c| channels.contains(c);
    let mut normal = on(Channel::Normal).then(|| [Vec::with_capacity(n), Vec::with_capacity(n), Vec::with_capacity(n)]);
    let mut intensity = on(Channel::Intensity).then(|| Vec::with_capacity(n));
    let mut colour = on(Channel::Colour).then(|| Vec::with_capacity(n));
    let mut quality = on(Channel::Quality).then(|| Vec::with_capacity(n));
    let mut scanner = on(Channel::ScannerId).then(|| Vec::with_capacity(n));
    let mut ts = on(Channel::TimestampNs).then(|| Vec::with_capacity(n));
    for &(idx, p) in pos {
        let at = p * rec;
        let r = table.get(at..at + rec).ok_or(OocError::WriterFailed)?;
        index.push(idx);
        x.push(read_f64(r, 8));
        y.push(read_f64(r, 16));
        z.push(read_f64(r, 24));
        let mut o = 32usize;
        for c in channels.iter() {
            match c {
                Channel::Colour => push_to(&mut colour, field::<3>(r, o)),
                Channel::Intensity => push_to(&mut intensity, f32::from_bits(u32::from_le_bytes(field(r, o)))),
                Channel::Normal => {
                    if let Some([nx, ny, nz]) = normal.as_mut() {
                        nx.push(read_f64(r, o));
                        ny.push(read_f64(r, o + 8));
                        nz.push(read_f64(r, o + 16));
                    }
                }
                Channel::Quality => push_to(&mut quality, u16::from_le_bytes(field(r, o))),
                Channel::ScannerId => push_to(&mut scanner, u32::from_le_bytes(field(r, o))),
                Channel::TimestampNs => push_to(&mut ts, i64::from_le_bytes(field(r, o))),
            }
            o += c.width() as usize;
        }
    }
    attrs.normal = normal;
    attrs.intensity = intensity;
    attrs.colour = colour;
    attrs.quality = quality;
    attrs.scanner_id = scanner;
    attrs.timestamp_ns = ts;
    Ok(Chunk { cell, index, points: PointBuffer::new(x, y, z), attrs })
}

impl Drop for ChunkWriter<'_> {
    fn drop(&mut self) {
        self.columns.clear();
        let _ = fs::remove_dir_all(&self.dir);
    }
}
