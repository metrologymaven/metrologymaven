//! Canonical encoding v1, the bounded decoder and the body digest. Spec node
//! `GEO.CAD.TOPO.MODEL` (REQ-006, REQ-007).
//!
//! # Layout v1 (pinned in the NODE Design; integers little-endian)
//!
//! ```text
//! payload  = "FIDBREP" | version u8 (= 1) | body_id [16]
//!            | n_shells u64 | n_faces u64 | n_loops u64 | n_coedges u64 | n_edges u64 | n_vertices u64
//!            | shells | faces | loops | coedges | edges | vertices        (each in canonical order)
//! encoding = payload | SHA-256(payload) [32]
//! shell    = id | closed u8 | face_count u64
//! face     = id | sense u8 | loop_count u64 | surface
//! loop     = id | outer u8 | coedge_count u64
//! coedge   = id | edge u64 | sense u8 | pcurve
//! edge     = id | start u64 | end u64 | t0 f64 | t1 f64 | curve
//! vertex   = id | x f64 | y f64 | z f64
//! id       = 0x00 | uuid [16]  |  0x01 | len u16 | UTF-8 [len]   (1 <= len <= 256; shells 0x00 only)
//! ```
//!
//! A shell owns the next `face_count` faces, a face the next `loop_count` loops, a loop the
//! next `coedge_count` coedges, so the counts must add up. Booleans are `0`/`1`; senses
//! `0` same, `1` reversed. Every `f64` is its IEEE 754 binary64 bits; NaN, infinities and
//! `-0.0` are never written (the model stores `+0.0`) and are refused on decode.
//!
//! Curves (`curve`, tag u8): `0` absent, `1` line, `2` circle, `3` ellipse, `16` NURBS.
//! Surfaces (tag u8): `1` plane, `2` cylinder, `3` cone, `4` sphere, `5` torus, `16` NURBS.
//! Pcurves (tag u8): `0` absent, `1` 2-D line (`origin` 2 f64, `dir` 2 f64), `16` NURBS.
//! Analytic: frame (`origin`, `axis`, `ref_dir`, 9 f64), then the sizes in constructor order
//! (circle `radius`; ellipse `semi_axis_1`, `semi_axis_2`; cylinder and sphere `radius`;
//! cone `radius`, `half_angle`; torus `major_radius`, `minor_radius`).
//! NURBS curve, GEO.NURBS.DATA field order: `degree` u64 | `n_knots` u64 | knots f64 |
//! `n_ctrl` u64 | ctrl (3 f64 each; 2 f64 for a pcurve, whose `z` is zero) | weights flag u8 |
//! weights (`n_ctrl` f64, when the flag is 1) | `periodic` u8.
//! NURBS surface: `degree_u` u64 | `degree_v` u64 | `n_knots_u` u64 | knots_u |
//! `n_knots_v` u64 | knots_v | `n_u` u64 | `n_v` u64 | `n_ctrl` u64 | ctrl (3 f64 each) |
//! weights flag u8 | weights | `periodic_u` u8 | `periodic_v` u8.
//!
//! # Decoding
//!
//! In order: more than [`MAX_ENCODED_BYTES`] is `TooLarge`; then the trailer must equal the
//! SHA-256 of the payload (`Malformed`), so truncation and bit flips fail before parsing;
//! then the magic (`Malformed`) and the version (`UnsupportedVersion`); then counts (a total
//! above [`MAX_ENTITIES`] is `TooLarge`; counts that cannot fit the remaining bytes are
//! `Malformed`); then every field. The parsed description goes through
//! [`BodyBuilder::build`], so a decoded body passes every build check, and re-encoding it must
//! give the input bytes back (`Malformed` with `NonCanonical` otherwise).

use core::fmt;

use fid_geo_nurbs::{NurbsCurve, NurbsCurveParts, NurbsSurface, NurbsSurfaceParts};
use fid_kci::kci::{sha256, sha256_digest};
use fid_types::Digest;

use super::geom::{Curve2, CurveDef, CurveParams, Frame, GeomError, SurfaceDef, SurfaceParams};
use super::model::{Body, BodyBuilder, Sense, TopoBuildError, TopologyId, MAX_ENTITIES, MAX_LABEL_BYTES};

/// Encoding version written and accepted.
pub const VERSION: u8 = 1;

/// Largest accepted encoding, 1 GiB (REQ-007).
pub const MAX_ENCODED_BYTES: usize = 1 << 30;

const MAGIC: &[u8; 7] = b"FIDBREP";
const TRAILER: usize = 32;
const HEADER: usize = 7 + 1 + 16 + 6 * 8;

/// Smallest encoded size of each kind (shell, face, loop, coedge, edge, vertex), used to
/// refuse counts the input cannot hold before anything is allocated.
const MIN_BYTES: [u64; 6] = [17 + 1 + 8, 4 + 1 + 8 + 1 + 72, 4 + 1 + 8, 4 + 8 + 1 + 1, 4 + 8 + 8 + 16 + 1, 4 + 24];

const CURVE_NONE: u8 = 0;
const CURVE_LINE: u8 = 1;
const CURVE_CIRCLE: u8 = 2;
const CURVE_ELLIPSE: u8 = 3;
const SURF_PLANE: u8 = 1;
const SURF_CYLINDER: u8 = 2;
const SURF_CONE: u8 = 3;
const SURF_SPHERE: u8 = 4;
const SURF_TORUS: u8 = 5;
const PCURVE_LINE: u8 = 1;
const TAG_NURBS: u8 = 16;
const ID_DERIVED: u8 = 0;
const ID_IMPORTED: u8 = 1;

/// What was wrong with malformed bytes.
#[derive(Debug, Clone, PartialEq)]
pub enum MalformedReason {
    /// The input ends before a field (or is shorter than header and trailer).
    Truncated,
    /// The trailer is not the SHA-256 of the payload.
    Checksum,
    /// The payload does not start with `FIDBREP`.
    Magic,
    /// An ID, curve, surface or pcurve tag is not defined.
    BadTag(u8),
    /// A boolean or sense byte is not 0 or 1.
    BadFlag(u8),
    /// An `f64` is NaN, infinite or `-0.0`.
    BadFloat,
    /// A label is not UTF-8 or its length is outside `1..=256`; or a shell has a label.
    BadLabel,
    /// A count or index does not fit this platform's `usize`, or the counts do not add up.
    BadCount,
    /// GEO.NURBS refused a curve or surface.
    Geometry(GeomError),
    /// The description fails a build check.
    Invalid(Box<TopoBuildError>),
    /// The bytes describe a valid body but are not its canonical encoding.
    NonCanonical,
    /// Bytes remain after the last vertex.
    TrailingBytes,
}

/// Why [`decode`] refused its input.
#[derive(Debug, Clone, PartialEq)]
pub enum DecodeError {
    /// More than [`MAX_ENCODED_BYTES`] bytes, or more than [`MAX_ENTITIES`] entities.
    TooLarge { bytes: usize, entities: u64 },
    /// The bytes are not a canonical encoding of a valid body; `offset` is where reading
    /// stopped.
    Malformed { offset: usize, reason: MalformedReason },
    /// The magic is right but the version is not [`VERSION`].
    UnsupportedVersion { version: u8 },
}

impl DecodeError {
    /// Stable variant name.
    pub fn code(&self) -> &'static str {
        match self {
            DecodeError::TooLarge { .. } => "TooLarge",
            DecodeError::Malformed { .. } => "Malformed",
            DecodeError::UnsupportedVersion { .. } => "UnsupportedVersion",
        }
    }
}

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DecodeError::TooLarge { bytes, entities } => {
                write!(f, "encoded body too large: {bytes} bytes, {entities} entities")
            }
            DecodeError::Malformed { offset, reason } => match reason {
                MalformedReason::Geometry(e) => write!(f, "malformed at byte {offset}: {e}"),
                MalformedReason::Invalid(e) => write!(f, "malformed at byte {offset}: {e}"),
                other => write!(f, "malformed at byte {offset}: {other:?}"),
            },
            DecodeError::UnsupportedVersion { version } => write!(f, "unsupported encoding version {version}"),
        }
    }
}

impl core::error::Error for DecodeError {}

// ---------------------------------------------------------------- encoder

struct Writer(Vec<u8>);

impl Writer {
    fn u8(&mut self, v: u8) {
        self.0.push(v);
    }
    fn u64(&mut self, v: u64) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn size(&mut self, v: usize) {
        self.u64(v as u64);
    }
    fn f64(&mut self, v: f64) {
        self.0.extend_from_slice(&v.to_bits().to_le_bytes());
    }
    fn f64s(&mut self, v: &[f64]) {
        for &x in v {
            self.f64(x);
        }
    }
    fn flag(&mut self, b: bool) {
        self.u8(u8::from(b));
    }
    fn sense(&mut self, s: Sense) {
        self.u8(match s {
            Sense::Same => 0,
            Sense::Reversed => 1,
        });
    }
    fn id(&mut self, id: &TopologyId) {
        match id {
            TopologyId::Derived(b) => {
                self.u8(ID_DERIVED);
                self.0.extend_from_slice(b);
            }
            TopologyId::Imported(l) => {
                let bytes = l.as_str().as_bytes();
                self.u8(ID_IMPORTED);
                // A label is at most 256 bytes, so the length fits u16.
                self.0.extend_from_slice(&u16::try_from(bytes.len()).unwrap_or(u16::MAX).to_le_bytes());
                self.0.extend_from_slice(bytes);
            }
        }
    }
    fn frame(&mut self, fr: &Frame) {
        self.f64s(&fr.origin);
        self.f64s(&fr.axis);
        self.f64s(&fr.ref_dir);
    }
    fn nurbs_curve(&mut self, c: &NurbsCurve, dims: usize) {
        self.size(c.degree());
        let knots = c.knots().as_slice();
        self.size(knots.len());
        self.f64s(knots);
        self.size(c.ctrl().len());
        for p in c.ctrl() {
            self.f64s(&p[..dims]);
        }
        self.weights(c.weights());
        self.flag(c.periodic());
    }
    fn weights(&mut self, w: Option<&[f64]>) {
        match w {
            Some(w) => {
                self.u8(1);
                self.f64s(w);
            }
            None => self.u8(0),
        }
    }
    fn curve(&mut self, c: Option<&CurveDef>) {
        let Some(c) = c else {
            self.u8(CURVE_NONE);
            return;
        };
        if let Some(n) = c.as_nurbs() {
            self.u8(TAG_NURBS);
            self.nurbs_curve(n, 3);
            return;
        }
        match c.params() {
            Some(CurveParams::Line { frame }) => {
                self.u8(CURVE_LINE);
                self.frame(frame);
            }
            Some(CurveParams::Circle { frame, radius }) => {
                self.u8(CURVE_CIRCLE);
                self.frame(frame);
                self.f64(*radius);
            }
            Some(CurveParams::Ellipse { frame, semi_axis_1, semi_axis_2 }) => {
                self.u8(CURVE_ELLIPSE);
                self.frame(frame);
                self.f64(*semi_axis_1);
                self.f64(*semi_axis_2);
            }
            // A CurveDef is analytic or NURBS; nothing else exists.
            None => self.u8(CURVE_NONE),
        }
    }
    fn surface(&mut self, s: &SurfaceDef) {
        if let Some(n) = s.as_nurbs() {
            self.u8(TAG_NURBS);
            self.size(n.degree_u());
            self.size(n.degree_v());
            let ku = n.knots_u().as_slice();
            self.size(ku.len());
            self.f64s(ku);
            let kv = n.knots_v().as_slice();
            self.size(kv.len());
            self.f64s(kv);
            let (n_u, n_v) = n.shape();
            self.size(n_u);
            self.size(n_v);
            self.size(n.ctrl().len());
            for p in n.ctrl() {
                self.f64s(p);
            }
            self.weights(n.weights());
            self.flag(n.periodic_u());
            self.flag(n.periodic_v());
            return;
        }
        match s.params() {
            Some(SurfaceParams::Plane { frame }) => {
                self.u8(SURF_PLANE);
                self.frame(frame);
            }
            Some(SurfaceParams::Cylinder { frame, radius }) => {
                self.u8(SURF_CYLINDER);
                self.frame(frame);
                self.f64(*radius);
            }
            Some(SurfaceParams::Cone { frame, radius, half_angle }) => {
                self.u8(SURF_CONE);
                self.frame(frame);
                self.f64(*radius);
                self.f64(*half_angle);
            }
            Some(SurfaceParams::Sphere { frame, radius }) => {
                self.u8(SURF_SPHERE);
                self.frame(frame);
                self.f64(*radius);
            }
            Some(SurfaceParams::Torus { frame, major_radius, minor_radius }) => {
                self.u8(SURF_TORUS);
                self.frame(frame);
                self.f64(*major_radius);
                self.f64(*minor_radius);
            }
            // A SurfaceDef is analytic or NURBS; nothing else exists.
            None => self.u8(0),
        }
    }
    fn pcurve(&mut self, c: Option<&Curve2>) {
        match c {
            None => self.u8(CURVE_NONE),
            Some(c) => {
                if let Some((origin, dir)) = c.as_line() {
                    self.u8(PCURVE_LINE);
                    self.f64s(&origin);
                    self.f64s(&dir);
                } else if let Some(n) = c.as_nurbs() {
                    self.u8(TAG_NURBS);
                    self.nurbs_curve(n, 2);
                }
            }
        }
    }
}

/// The canonical encoding v1 of a body (module documentation).
pub fn encode(body: &Body) -> Vec<u8> {
    let mut w = Writer(Vec::new());
    w.0.extend_from_slice(MAGIC);
    w.u8(VERSION);
    w.0.extend_from_slice(body.id());
    for n in [
        body.shells().len(),
        body.faces().len(),
        body.loops().len(),
        body.coedges().len(),
        body.edges().len(),
        body.vertices().len(),
    ] {
        w.size(n);
    }
    for s in body.shells() {
        w.id(s.id());
        w.flag(s.closed());
        w.size(s.faces().len());
    }
    for f in body.faces() {
        w.id(f.id());
        w.sense(f.sense());
        w.size(f.loops().len());
        w.surface(f.surface());
    }
    for l in body.loops() {
        w.id(l.id());
        w.flag(l.outer());
        w.size(l.coedges().len());
    }
    for c in body.coedges() {
        w.id(c.id());
        w.size(c.edge().index());
        w.sense(c.sense());
        w.pcurve(c.pcurve());
    }
    for e in body.edges() {
        w.id(e.id());
        w.size(e.start().index());
        w.size(e.end().index());
        w.f64s(&e.param_range());
        w.curve(e.curve());
    }
    for v in body.vertices() {
        w.id(v.id());
        w.f64s(&v.point());
    }
    let trailer = sha256(&w.0);
    w.0.extend_from_slice(&trailer);
    w.0
}

/// SHA-256 of [`encode`], in the `sha256:` wire form.
pub fn body_digest(body: &Body) -> Digest {
    sha256_digest(&encode(body))
}

// ---------------------------------------------------------------- decoder

struct Reader<'a> {
    bytes: &'a [u8],
    pos: usize,
}

type R<T> = Result<T, DecodeError>;

impl Reader<'_> {
    fn bad(&self, reason: MalformedReason) -> DecodeError {
        DecodeError::Malformed { offset: self.pos, reason }
    }
    fn remaining(&self) -> usize {
        self.bytes.len().saturating_sub(self.pos)
    }
    fn take(&mut self, n: usize) -> R<&[u8]> {
        let end = self.pos.checked_add(n).ok_or_else(|| self.bad(MalformedReason::Truncated))?;
        match self.bytes.get(self.pos..end) {
            Some(s) => {
                self.pos = end;
                Ok(s)
            }
            None => Err(self.bad(MalformedReason::Truncated)),
        }
    }
    fn array<const N: usize>(&mut self) -> R<[u8; N]> {
        let mut out = [0u8; N];
        out.copy_from_slice(self.take(N)?);
        Ok(out)
    }
    fn u8(&mut self) -> R<u8> {
        Ok(self.array::<1>()?[0])
    }
    fn u64(&mut self) -> R<u64> {
        Ok(u64::from_le_bytes(self.array::<8>()?))
    }
    fn usize(&mut self) -> R<usize> {
        let v = self.u64()?;
        usize::try_from(v).map_err(|_| self.bad(MalformedReason::BadCount))
    }
    /// A length of `elem`-byte items that must fit the remaining input.
    fn count(&mut self, elem: usize) -> R<usize> {
        let n = self.usize()?;
        match n.checked_mul(elem) {
            Some(b) if b <= self.remaining() => Ok(n),
            _ => Err(self.bad(MalformedReason::Truncated)),
        }
    }
    fn f64(&mut self) -> R<f64> {
        let bits = self.u64()?;
        let x = f64::from_bits(bits);
        if !x.is_finite() || bits == 0x8000_0000_0000_0000 {
            self.pos -= 8;
            return Err(self.bad(MalformedReason::BadFloat));
        }
        Ok(x)
    }
    fn f64_vec(&mut self, n: usize) -> R<Vec<f64>> {
        let mut v = Vec::with_capacity(n);
        for _ in 0..n {
            v.push(self.f64()?);
        }
        Ok(v)
    }
    fn f3(&mut self) -> R<[f64; 3]> {
        Ok([self.f64()?, self.f64()?, self.f64()?])
    }
    fn flag(&mut self) -> R<bool> {
        match self.u8()? {
            0 => Ok(false),
            1 => Ok(true),
            b => {
                self.pos -= 1;
                Err(self.bad(MalformedReason::BadFlag(b)))
            }
        }
    }
    fn sense(&mut self) -> R<Sense> {
        Ok(if self.flag()? { Sense::Reversed } else { Sense::Same })
    }
    /// `None` for a derived ID (its value is re-derived by the build and compared through
    /// the re-encoding), `Some(label)` for an imported one.
    fn id(&mut self, label_allowed: bool) -> R<Option<String>> {
        match self.u8()? {
            ID_DERIVED => {
                self.take(16)?;
                Ok(None)
            }
            ID_IMPORTED if label_allowed => {
                let len = usize::from(u16::from_le_bytes(self.array::<2>()?));
                if len == 0 || len > MAX_LABEL_BYTES {
                    return Err(self.bad(MalformedReason::BadLabel));
                }
                let raw = self.take(len)?;
                match core::str::from_utf8(raw) {
                    Ok(s) => Ok(Some(s.to_string())),
                    Err(_) => Err(self.bad(MalformedReason::BadLabel)),
                }
            }
            ID_IMPORTED => Err(self.bad(MalformedReason::BadLabel)),
            t => Err(self.bad(MalformedReason::BadTag(t))),
        }
    }
    fn frame(&mut self) -> R<Frame> {
        Ok(Frame { origin: self.f3()?, axis: self.f3()?, ref_dir: self.f3()? })
    }
    fn geom<T>(&self, r: Result<T, GeomError>) -> R<T> {
        r.map_err(|e| self.bad(MalformedReason::Geometry(e)))
    }
    fn nurbs_curve(&mut self, dims: usize) -> R<NurbsCurve> {
        let degree = self.usize()?;
        let n_knots = self.count(8)?;
        let knots = self.f64_vec(n_knots)?;
        let n_ctrl = self.count(8 * dims)?;
        let mut ctrl = Vec::with_capacity(n_ctrl);
        for _ in 0..n_ctrl {
            let x = self.f64()?;
            let y = self.f64()?;
            let z = if dims == 3 { self.f64()? } else { 0.0 };
            ctrl.push([x, y, z]);
        }
        let weights = self.weights(n_ctrl)?;
        let periodic = self.flag()?;
        let r = NurbsCurve::new(NurbsCurveParts { degree, knots, ctrl, weights, periodic }).map_err(GeomError::from);
        self.geom(r)
    }
    fn weights(&mut self, n: usize) -> R<Option<Vec<f64>>> {
        if self.flag()? {
            let n = n.checked_mul(8).filter(|b| *b <= self.remaining()).map(|_| n);
            match n {
                Some(n) => Ok(Some(self.f64_vec(n)?)),
                None => Err(self.bad(MalformedReason::Truncated)),
            }
        } else {
            Ok(None)
        }
    }
    fn curve(&mut self) -> R<Option<CurveDef>> {
        let tag = self.u8()?;
        let params = match tag {
            CURVE_NONE => return Ok(None),
            CURVE_LINE => CurveParams::Line { frame: self.frame()? },
            CURVE_CIRCLE => CurveParams::Circle { frame: self.frame()?, radius: self.f64()? },
            CURVE_ELLIPSE => {
                CurveParams::Ellipse { frame: self.frame()?, semi_axis_1: self.f64()?, semi_axis_2: self.f64()? }
            }
            TAG_NURBS => {
                let n = self.nurbs_curve(3)?;
                return Ok(Some(self.geom(CurveDef::nurbs(n))?));
            }
            t => {
                self.pos -= 1;
                return Err(self.bad(MalformedReason::BadTag(t)));
            }
        };
        Ok(Some(self.geom(CurveDef::analytic(params))?))
    }
    fn surface(&mut self) -> R<SurfaceDef> {
        let tag = self.u8()?;
        let params = match tag {
            SURF_PLANE => SurfaceParams::Plane { frame: self.frame()? },
            SURF_CYLINDER => SurfaceParams::Cylinder { frame: self.frame()?, radius: self.f64()? },
            SURF_CONE => SurfaceParams::Cone { frame: self.frame()?, radius: self.f64()?, half_angle: self.f64()? },
            SURF_SPHERE => SurfaceParams::Sphere { frame: self.frame()?, radius: self.f64()? },
            SURF_TORUS => {
                SurfaceParams::Torus { frame: self.frame()?, major_radius: self.f64()?, minor_radius: self.f64()? }
            }
            TAG_NURBS => {
                let degree_u = self.usize()?;
                let degree_v = self.usize()?;
                let n = self.count(8)?;
                let knots_u = self.f64_vec(n)?;
                let n = self.count(8)?;
                let knots_v = self.f64_vec(n)?;
                let n_u = self.usize()?;
                let n_v = self.usize()?;
                let n_ctrl = self.count(24)?;
                let mut ctrl = Vec::with_capacity(n_ctrl);
                for _ in 0..n_ctrl {
                    ctrl.push(self.f3()?);
                }
                let weights = self.weights(n_ctrl)?;
                let periodic_u = self.flag()?;
                let periodic_v = self.flag()?;
                let parts = NurbsSurfaceParts {
                    degree_u,
                    degree_v,
                    knots_u,
                    knots_v,
                    n_u,
                    n_v,
                    ctrl,
                    weights,
                    periodic_u,
                    periodic_v,
                };
                let s = self.geom(NurbsSurface::new(parts).map_err(GeomError::from))?;
                return self.geom(SurfaceDef::nurbs(s));
            }
            t => {
                self.pos -= 1;
                return Err(self.bad(MalformedReason::BadTag(t)));
            }
        };
        self.geom(SurfaceDef::analytic(params))
    }
    fn pcurve(&mut self) -> R<Option<Curve2>> {
        match self.u8()? {
            CURVE_NONE => Ok(None),
            PCURVE_LINE => {
                let origin = [self.f64()?, self.f64()?];
                let dir = [self.f64()?, self.f64()?];
                Ok(Some(self.geom(Curve2::line(origin, dir))?))
            }
            TAG_NURBS => {
                let n = self.nurbs_curve(2)?;
                Ok(Some(self.geom(Curve2::nurbs(n))?))
            }
            t => {
                self.pos -= 1;
                Err(self.bad(MalformedReason::BadTag(t)))
            }
        }
    }
}

/// The next `count` children of a kind with `total` entities, starting at `*next`.
fn owned(r: &Reader<'_>, next: &mut usize, count: usize, total: usize) -> R<Vec<usize>> {
    let end = next.checked_add(count).filter(|e| *e <= total).ok_or_else(|| r.bad(MalformedReason::BadCount))?;
    let v: Vec<usize> = (*next..end).collect();
    *next = end;
    Ok(v)
}

/// Decodes a canonical encoding v1 (module documentation). Never panics; every failure is a
/// typed error, and a returned body is exactly as valid as one from [`BodyBuilder::build`].
pub fn decode(bytes: &[u8]) -> Result<Body, DecodeError> {
    if bytes.len() > MAX_ENCODED_BYTES {
        return Err(DecodeError::TooLarge { bytes: bytes.len(), entities: 0 });
    }
    if bytes.len() < HEADER + TRAILER {
        return Err(DecodeError::Malformed { offset: bytes.len(), reason: MalformedReason::Truncated });
    }
    let split = bytes.len() - TRAILER;
    let (payload, trailer) = bytes.split_at(split);
    if sha256(payload) != trailer {
        return Err(DecodeError::Malformed { offset: split, reason: MalformedReason::Checksum });
    }
    let mut r = Reader { bytes: payload, pos: 0 };
    if r.take(7)? != MAGIC {
        return Err(DecodeError::Malformed { offset: 0, reason: MalformedReason::Magic });
    }
    let version = r.u8()?;
    if version != VERSION {
        return Err(DecodeError::UnsupportedVersion { version });
    }
    let body_id = r.array::<16>()?;
    let mut counts = [0u64; 6];
    for c in counts.iter_mut() {
        *c = r.u64()?;
    }
    let total = counts.iter().fold(0u64, |a, &n| a.saturating_add(n));
    if total > MAX_ENTITIES as u64 {
        return Err(DecodeError::TooLarge { bytes: bytes.len(), entities: total });
    }
    let need = counts.iter().zip(MIN_BYTES).fold(0u64, |a, (&n, m)| a.saturating_add(n.saturating_mul(m)));
    if need > r.remaining() as u64 {
        return Err(r.bad(MalformedReason::Truncated));
    }
    // Every count is at most MAX_ENTITIES here, so it fits usize on every target.
    let n = counts.map(|c| usize::try_from(c).unwrap_or(usize::MAX));
    let [n_shells, n_faces, n_loops, n_coedges, n_edges, n_vertices] = n;

    let mut b = BodyBuilder::new(body_id);
    let mut next_face = 0usize;
    let mut shells: Vec<(Vec<usize>, bool)> = Vec::with_capacity(n_shells);
    for _ in 0..n_shells {
        r.id(false)?;
        let closed = r.flag()?;
        let k = r.usize()?;
        shells.push((owned(&r, &mut next_face, k, n_faces)?, closed));
    }
    let mut next_loop = 0usize;
    let mut faces = Vec::with_capacity(n_faces);
    for _ in 0..n_faces {
        let label = r.id(true)?;
        let sense = r.sense()?;
        let k = r.usize()?;
        let loops = owned(&r, &mut next_loop, k, n_loops)?;
        let surface = r.surface()?;
        faces.push((surface, sense, loops, label));
    }
    let mut next_coedge = 0usize;
    let mut loops = Vec::with_capacity(n_loops);
    for _ in 0..n_loops {
        let label = r.id(true)?;
        let outer = r.flag()?;
        let k = r.usize()?;
        loops.push((owned(&r, &mut next_coedge, k, n_coedges)?, outer, label));
    }
    if next_face != n_faces || next_loop != n_loops || next_coedge != n_coedges {
        return Err(r.bad(MalformedReason::BadCount));
    }
    for _ in 0..n_coedges {
        let label = r.id(true)?;
        let edge = r.usize()?;
        let sense = r.sense()?;
        let pcurve = r.pcurve()?;
        b.add_coedge(edge, sense, pcurve, label.as_deref());
    }
    for _ in 0..n_edges {
        let label = r.id(true)?;
        let start = r.usize()?;
        let end = r.usize()?;
        let range = [r.f64()?, r.f64()?];
        let curve = r.curve()?;
        b.add_edge(curve, start, end, range, label.as_deref());
    }
    for _ in 0..n_vertices {
        let label = r.id(true)?;
        let point = r.f3()?;
        b.add_vertex(point, label.as_deref());
    }
    if r.remaining() != 0 {
        return Err(r.bad(MalformedReason::TrailingBytes));
    }
    for (coedges, outer, label) in loops {
        b.add_loop(coedges, outer, label.as_deref());
    }
    for (surface, sense, loops, label) in faces {
        b.add_face(surface, sense, loops, label.as_deref());
    }
    for (faces, closed) in shells {
        b.add_shell(faces, closed);
    }
    let body = b
        .build()
        .map_err(|e| DecodeError::Malformed { offset: split, reason: MalformedReason::Invalid(Box::new(e)) })?;
    if encode(&body) != bytes {
        return Err(DecodeError::Malformed { offset: split, reason: MalformedReason::NonCanonical });
    }
    Ok(body)
}
