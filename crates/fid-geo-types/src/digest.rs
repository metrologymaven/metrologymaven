//! Canonical byte layout and content digest of geometric elements (spec node
//! `GEO.TYPES.DIGEST`).
//!
//! [`digest`] is SHA-256 over the canonical layout below. The layout is streamed through
//! one fixed 64 KiB block buffer ([`BLOCK_BYTES`]) into the kernel SHA-256 of `fid-kci`,
//! so the element is never copied and the extra memory does not depend on its size
//! (REQ-GEO.TYPES.DIGEST-005). Only integer and byte operations are used (D0), so the
//! digest is the same on every platform, `wasm32` included.
//!
//! # Layout version 1
//!
//! All integers are little-endian.
//!
//! ```text
//! "FIDGEO" | 0x01 | kind u8 | frame [16]
//! point set (kind 1), point cloud (kind 2), mesh (kind 3):
//!     n_points u64 | x[n_points] f64 | y[n_points] f64 | z[n_points] f64
//!     n_tris u64   | n_tris × (a u64, b u64, c u64)        n_tris = 0 for point kinds
//!     n_channels u64
//!     per channel, in ascending name-byte order (CHANNEL_NAMES):
//!         name_len u16 | name (ASCII) | type u8 | n_records u64 | records
//! CAD body handle (kind 4):
//!     body [16] | source_digest [32 raw bytes]
//! ```
//!
//! Kind bytes: see [`kind_byte`]. Triangle indexes are always written as `u64`, whatever
//! the storage width, so a mesh's digest does not depend on it
//! (REQ-GEO.TYPES.DIGEST-003). Topology facts are derived data and are not hashed, nor is
//! the import-time box of a CAD body handle (the spec hashes body ID and source digest).
//!
//! ## Floats
//!
//! Every `f64` is written as its IEEE 754 bits, except that `-0.0` is written as `+0.0`
//! (REQ-GEO.TYPES.DIGEST-002) and any NaN as the quiet NaN `0x7ff8_0000_0000_0000`.
//! Valid elements hold no NaN or infinity in an `f64` field (their constructors reject
//! them), so the NaN rule only makes the function total. `f32` values (intensity, which
//! the element types do not validate) get the same treatment: `-0.0` is written as
//! `+0.0` and any NaN as `0x7fc0_0000`; infinities keep their bits. The rules work on bit
//! patterns, never on float arithmetic.
//!
//! ## Channels
//!
//! | name | type | records |
//! |---|---|---|
//! | `colour` | 3 `u8x3` | one per point |
//! | `grid` | 7 `u64` | two (rows, cols), only when a grid is declared |
//! | `intensity` | 2 `f32` | one per point |
//! | `normal` | 8 `f64x3col` | one per point |
//! | `quality` | 4 `u16` | one per point |
//! | `scan_origins` | 9 `origin` | one per origin, only when there is at least one |
//! | `scanner_id` | 5 `u32` | one per point |
//! | `timestamp_ns` | 6 `i64` | one per point |
//! | `triangle_quality` | 4 `u16` | one per triangle (meshes) |
//! | `triangle_scanner_id` | 5 `u32` | one per triangle (meshes) |
//!
//! Record encodings by type byte ([`ChannelType`]): 1 `f64`; 2 `f32`; 3 `u8x3` three
//! bytes r, g, b; 4 `u16`; 5 `u32`; 6 `i64` two's complement; 7 `u64`; 8 `f64x3col` all
//! x values, then all y values, then all z values (columns, like the coordinates);
//! 9 `origin` per record scanner ID `u32` then x, y, z `f64`. A channel that is present
//! with zero records is written (and differs from an absent one). Every variable-length
//! part carries its own count and every channel its name and type, so no two different
//! elements share a layout.
//!
//! The vectors in `fixtures/geo/types/digest_vectors.json` pin the layout; they come from
//! the independent Python reference `tools/fixtures/geo_digest_vectors.py` (`hashlib`).

use crate::envelope::{CadBodyRef, ElementKind, FrameRef, GeometryElement};
use crate::mesh::{Mesh, TriangleAttrs};
use crate::points::{AttributeSet, PointBuffer, PointCloud, PointSet};
use core::fmt;
use fid_kci::kci::Sha256;
use fid_types::Digest;

/// The ASCII tag every layout starts with.
pub const LAYOUT_TAG: [u8; 6] = *b"FIDGEO";

/// The layout version this module writes.
pub const LAYOUT_VERSION: u8 = 1;

/// The size of the streaming block buffer, and the largest block [`write_layout`] hands
/// to its sink.
pub const BLOCK_BYTES: usize = 64 * 1024;

/// The channel names in the order they are written (ascending bytes).
pub const CHANNEL_NAMES: [&str; 10] = [
    "colour",
    "grid",
    "intensity",
    "normal",
    "quality",
    "scan_origins",
    "scanner_id",
    "timestamp_ns",
    "triangle_quality",
    "triangle_scanner_id",
];

/// The canonical `f64` NaN written for any NaN.
const NAN64: u64 = 0x7ff8_0000_0000_0000;
/// The canonical `f32` NaN written for any NaN.
const NAN32: u32 = 0x7fc0_0000;

/// The record encoding of a channel, written as its type byte.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChannelType {
    F64,
    F32,
    U8x3,
    U16,
    U32,
    I64,
    U64,
    F64x3Columns,
    Origin,
}

impl ChannelType {
    /// The type byte.
    pub const fn code(self) -> u8 {
        match self {
            ChannelType::F64 => 1,
            ChannelType::F32 => 2,
            ChannelType::U8x3 => 3,
            ChannelType::U16 => 4,
            ChannelType::U32 => 5,
            ChannelType::I64 => 6,
            ChannelType::U64 => 7,
            ChannelType::F64x3Columns => 8,
            ChannelType::Origin => 9,
        }
    }
}

/// The kind byte of the layout: point set 1, point cloud 2, mesh 3, CAD body handle 4.
pub const fn kind_byte(kind: ElementKind) -> u8 {
    match kind {
        ElementKind::PointSet => 1,
        ElementKind::PointCloud => 2,
        ElementKind::Mesh => 3,
        ElementKind::CadBodyRef => 4,
    }
}

/// The canonical bits of an `f64`: `-0.0` becomes `+0.0`, any NaN the quiet NaN.
pub const fn canonical_f64_bits(v: f64) -> u64 {
    let b = v.to_bits();
    if b == 1 << 63 {
        0
    } else if b & 0x7ff0_0000_0000_0000 == 0x7ff0_0000_0000_0000 && b & 0x000f_ffff_ffff_ffff != 0 {
        // Only for totality: element constructors reject non-finite f64 values, so a
        // valid element never reaches this branch.
        NAN64
    } else {
        b
    }
}

/// The canonical bits of an `f32`: `-0.0` becomes `+0.0`, any NaN the quiet NaN.
pub const fn canonical_f32_bits(v: f32) -> u32 {
    let b = v.to_bits();
    if b == 1 << 31 {
        0
    } else if b & 0x7f80_0000 == 0x7f80_0000 && b & 0x007f_ffff != 0 {
        NAN32
    } else {
        b
    }
}

/// The SHA-256 content digest of a geometric element over layout version 1.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ElementDigest(pub [u8; 32]);

impl ElementDigest {
    /// The 32 digest bytes.
    pub const fn to_bytes(self) -> [u8; 32] {
        self.0
    }

    /// The shared wire form, `sha256:` plus 64 lowercase hex digits.
    pub const fn to_digest(self) -> Digest {
        Digest::from_bytes(self.0)
    }
}

impl From<ElementDigest> for Digest {
    fn from(d: ElementDigest) -> Digest {
        d.to_digest()
    }
}

impl fmt::Display for ElementDigest {
    /// 64 lowercase hex digits.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for b in self.0 {
            write!(f, "{b:02x}")?;
        }
        Ok(())
    }
}

impl fmt::Debug for ElementDigest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ElementDigest({self})")
    }
}

/// A `usize` length as `u64`; lossless on every supported target, saturating otherwise.
fn count_of(n: usize) -> u64 {
    u64::try_from(n).unwrap_or(u64::MAX)
}

/// The fixed block buffer between the layout writer and its sink.
struct Blocks<'a> {
    buf: Vec<u8>,
    sink: &'a mut dyn FnMut(&[u8]),
}

impl<'a> Blocks<'a> {
    fn new(sink: &'a mut dyn FnMut(&[u8])) -> Blocks<'a> {
        Blocks { buf: Vec::with_capacity(BLOCK_BYTES), sink }
    }

    fn put(&mut self, mut bytes: &[u8]) {
        while !bytes.is_empty() {
            let room = BLOCK_BYTES - self.buf.len();
            let take = core::cmp::min(room, bytes.len());
            self.buf.extend_from_slice(&bytes[..take]);
            bytes = &bytes[take..];
            if self.buf.len() == BLOCK_BYTES {
                self.flush();
            }
        }
    }

    fn flush(&mut self) {
        if !self.buf.is_empty() {
            (self.sink)(&self.buf);
            self.buf.clear();
        }
    }

    fn u8(&mut self, v: u8) {
        self.put(&[v]);
    }

    fn u16(&mut self, v: u16) {
        self.put(&v.to_le_bytes());
    }

    fn u32(&mut self, v: u32) {
        self.put(&v.to_le_bytes());
    }

    fn u64(&mut self, v: u64) {
        self.put(&v.to_le_bytes());
    }

    fn i64(&mut self, v: i64) {
        self.put(&v.to_le_bytes());
    }

    fn f64(&mut self, v: f64) {
        self.u64(canonical_f64_bits(v));
    }

    fn f32(&mut self, v: f32) {
        self.u32(canonical_f32_bits(v));
    }

    fn f64s(&mut self, col: &[f64]) {
        for &v in col {
            self.f64(v);
        }
    }

    fn header(&mut self, kind: ElementKind, frame: FrameRef) {
        self.put(&LAYOUT_TAG);
        self.u8(LAYOUT_VERSION);
        self.u8(kind_byte(kind));
        self.put(&frame.to_bytes());
    }

    fn channel(&mut self, name: &str, ty: ChannelType, records: usize) {
        self.u16(u16::try_from(name.len()).unwrap_or(u16::MAX));
        self.put(name.as_bytes());
        self.u8(ty.code());
        self.u64(count_of(records));
    }
}

/// Writes the channel count and the channels in [`CHANNEL_NAMES`] order.
fn channels(w: &mut Blocks<'_>, a: &AttributeSet, t: Option<&TriangleAttrs>) {
    let t_quality = t.and_then(|t| t.quality.as_deref());
    let t_scanner = t.and_then(|t| t.scanner_id.as_deref());
    let present = [
        a.colour.is_some(),
        a.grid.is_some(),
        a.intensity.is_some(),
        a.normal.is_some(),
        a.quality.is_some(),
        !a.scan_origins.is_empty(),
        a.scanner_id.is_some(),
        a.timestamp_ns.is_some(),
        t_quality.is_some(),
        t_scanner.is_some(),
    ];
    w.u64(count_of(present.iter().filter(|p| **p).count()));
    if let Some(v) = &a.colour {
        w.channel("colour", ChannelType::U8x3, v.len());
        for c in v {
            w.put(c);
        }
    }
    if let Some((rows, cols)) = a.grid {
        w.channel("grid", ChannelType::U64, 2);
        w.u64(rows);
        w.u64(cols);
    }
    if let Some(v) = &a.intensity {
        w.channel("intensity", ChannelType::F32, v.len());
        for &x in v {
            w.f32(x);
        }
    }
    if let Some([nx, ny, nz]) = &a.normal {
        w.channel("normal", ChannelType::F64x3Columns, nx.len());
        w.f64s(nx);
        w.f64s(ny);
        w.f64s(nz);
    }
    if let Some(v) = &a.quality {
        w.channel("quality", ChannelType::U16, v.len());
        for &q in v {
            w.u16(q);
        }
    }
    if !a.scan_origins.is_empty() {
        w.channel("scan_origins", ChannelType::Origin, a.scan_origins.len());
        for &(id, o) in &a.scan_origins {
            w.u32(id);
            w.f64s(&o);
        }
    }
    if let Some(v) = &a.scanner_id {
        w.channel("scanner_id", ChannelType::U32, v.len());
        for &s in v {
            w.u32(s);
        }
    }
    if let Some(v) = &a.timestamp_ns {
        w.channel("timestamp_ns", ChannelType::I64, v.len());
        for &s in v {
            w.i64(s);
        }
    }
    if let Some(v) = t_quality {
        w.channel("triangle_quality", ChannelType::U16, v.len());
        for &q in v {
            w.u16(q);
        }
    }
    if let Some(v) = t_scanner {
        w.channel("triangle_scanner_id", ChannelType::U32, v.len());
        for &s in v {
            w.u32(s);
        }
    }
}

/// Header, coordinate columns and the zero triangle count shared by the point kinds and
/// the mesh (whose triangles follow).
fn coordinates(w: &mut Blocks<'_>, kind: ElementKind, frame: FrameRef, buf: &PointBuffer) {
    w.header(kind, frame);
    w.u64(buf.len());
    w.f64s(&buf.x);
    w.f64s(&buf.y);
    w.f64s(&buf.z);
}

fn points_layout(w: &mut Blocks<'_>, kind: ElementKind, frame: FrameRef, buf: &PointBuffer, a: &AttributeSet) {
    coordinates(w, kind, frame, buf);
    w.u64(0);
    channels(w, a, None);
}

fn mesh_layout(w: &mut Blocks<'_>, m: &Mesh) {
    coordinates(w, ElementKind::Mesh, m.frame(), m.vertices());
    w.u64(m.triangle_count());
    for t in m.triangles() {
        for i in t {
            w.u64(i);
        }
    }
    channels(w, m.vertex_attrs(), Some(m.triangle_attrs()));
}

fn cad_layout(w: &mut Blocks<'_>, c: &CadBodyRef) {
    w.header(ElementKind::CadBodyRef, c.frame());
    w.put(&c.body());
    w.put(&c.source_digest().to_bytes());
}

/// Streams the layout of `e` to `sink` in blocks of at most [`BLOCK_BYTES`]. The
/// concatenated blocks are the canonical layout version 1.
pub fn write_layout(e: &GeometryElement, sink: &mut dyn FnMut(&[u8])) {
    let mut w = Blocks::new(sink);
    match e {
        GeometryElement::PointSet(p) => {
            points_layout(&mut w, ElementKind::PointSet, p.frame(), p.buffer(), p.attributes())
        }
        GeometryElement::PointCloud(p) => {
            points_layout(&mut w, ElementKind::PointCloud, p.frame(), p.buffer(), p.attributes())
        }
        GeometryElement::Mesh(m) => mesh_layout(&mut w, m),
        GeometryElement::CadBody(c) => cad_layout(&mut w, c),
    }
    w.flush();
}

/// Runs a layout writer into SHA-256.
fn hash_with(write: impl FnOnce(&mut Blocks<'_>)) -> ElementDigest {
    let mut h = Sha256::new();
    {
        let mut feed = |b: &[u8]| h.update(b);
        let mut w = Blocks::new(&mut feed);
        write(&mut w);
        w.flush();
    }
    ElementDigest(h.finish())
}

/// The content digest of any element (REQ-GEO.TYPES-005).
pub fn digest(e: &GeometryElement) -> ElementDigest {
    match e {
        GeometryElement::PointSet(p) => digest_point_set(p),
        GeometryElement::PointCloud(p) => digest_point_cloud(p),
        GeometryElement::Mesh(m) => digest_mesh(m),
        GeometryElement::CadBody(c) => digest_cad_body(c),
    }
}

/// The digest of a point set, equal to [`digest`] of it wrapped in a [`GeometryElement`].
pub fn digest_point_set(p: &PointSet) -> ElementDigest {
    hash_with(|w| points_layout(w, ElementKind::PointSet, p.frame(), p.buffer(), p.attributes()))
}

/// The digest of a point cloud, equal to [`digest`] of it wrapped in a [`GeometryElement`].
pub fn digest_point_cloud(p: &PointCloud) -> ElementDigest {
    hash_with(|w| points_layout(w, ElementKind::PointCloud, p.frame(), p.buffer(), p.attributes()))
}

/// The digest of a mesh, equal to [`digest`] of it wrapped in a [`GeometryElement`].
pub fn digest_mesh(m: &Mesh) -> ElementDigest {
    hash_with(|w| mesh_layout(w, m))
}

/// The digest of a CAD body handle, equal to [`digest`] of it wrapped in a
/// [`GeometryElement`].
pub fn digest_cad_body(c: &CadBodyRef) -> ElementDigest {
    hash_with(|w| cad_layout(w, c))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;
    use crate::mesh::MeshOptions;

    #[test]
    fn channel_names_ascend() {
        for w in CHANNEL_NAMES.windows(2) {
            assert!(w[0].as_bytes() < w[1].as_bytes(), "{} !< {}", w[0], w[1]);
        }
    }

    /// Walks the channel section of a layout and returns the names in written order.
    fn take<'a>(layout: &'a [u8], at: &mut usize, n: usize) -> &'a [u8] {
        let s = &layout[*at..*at + n];
        *at += n;
        s
    }

    fn written_names(layout: &[u8], channels_at: usize) -> Vec<String> {
        let mut at = channels_at;
        let rd = |at: &mut usize, n: usize| take(layout, at, n);
        let n = u64::from_le_bytes(rd(&mut at, 8).try_into().unwrap());
        let mut names = Vec::new();
        for _ in 0..n {
            let len = u16::from_le_bytes(rd(&mut at, 2).try_into().unwrap()) as usize;
            let name = String::from_utf8(rd(&mut at, len).to_vec()).unwrap();
            let ty = rd(&mut at, 1)[0];
            let records = u64::from_le_bytes(rd(&mut at, 8).try_into().unwrap()) as usize;
            let size = match ty {
                1 | 7 => 8,
                2 | 5 => 4,
                3 => 3,
                4 => 2,
                6 => 8,
                8 => 24,
                9 => 28,
                other => panic!("type {other}"),
            };
            at += records * size;
            names.push(name);
        }
        assert_eq!(at, layout.len());
        names
    }

    #[test]
    fn all_channels_written_in_name_order() {
        let attrs = AttributeSet {
            normal: Some([vec![1.0, 0.0, 0.0], vec![0.0, 1.0, 0.0], vec![0.0, 0.0, 1.0]]),
            intensity: Some(vec![0.5, -0.0, f32::NAN]),
            colour: Some(vec![[1, 2, 3]; 3]),
            scanner_id: Some(vec![1, 1, 2]),
            timestamp_ns: Some(vec![-5, 0, 5]),
            quality: Some(vec![0, 1, 2]),
            scan_origins: vec![(1, [0.0, 0.0, 1.0]), (2, [1.0, 0.0, 1.0])],
            grid: Some((3, 1)),
        };
        let options = MeshOptions {
            triangle_attrs: TriangleAttrs { scanner_id: Some(vec![7]), quality: Some(vec![9]) },
            ..MeshOptions::default()
        };
        let buf = PointBuffer::from_points(&[[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]]);
        let m = Mesh::with_options(FrameRef::from_bytes([1; 16]), buf, attrs, vec![[0, 1, 2]], options).unwrap();
        let mut layout = Vec::new();
        write_layout(&GeometryElement::Mesh(m), &mut |b: &[u8]| layout.extend_from_slice(b));
        // header 24, n_points 8, 3 columns of 3 f64, n_tris 8, one triangle of 3 u64.
        let channels_at = 24 + 8 + 72 + 8 + 24;
        assert_eq!(written_names(&layout, channels_at), CHANNEL_NAMES.to_vec());
    }

    #[test]
    fn canonical_bits() {
        assert_eq!(canonical_f64_bits(-0.0), 0);
        assert_eq!(canonical_f64_bits(0.0), 0);
        assert_eq!(canonical_f64_bits(f64::from_bits(0xfff0_0000_0000_0001)), NAN64);
        assert_eq!(canonical_f64_bits(f64::NEG_INFINITY), 0xfff0_0000_0000_0000);
        assert_eq!(canonical_f64_bits(-1.5), (-1.5f64).to_bits());
        assert_eq!(canonical_f32_bits(-0.0), 0);
        assert_eq!(canonical_f32_bits(f32::from_bits(0xffc0_0001)), NAN32);
        assert_eq!(canonical_f32_bits(f32::INFINITY), 0x7f80_0000);
        assert_eq!(canonical_f32_bits(f32::from_bits(1)), 1);
    }
}
