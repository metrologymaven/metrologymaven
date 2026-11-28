//! The section-curve view: the read-only form of a planar section curve this crate fits
//! elements to, and its JSON format `fid.scan.sect.curve/1`.
//!
//! The curve is an artifact (D-034): `SCAN.SECT` computes sections and is a later L2 domain
//! than GDT, so this crate reads the curve as data and never depends on SCAN code. **SCAN does
//! not publish this format yet**; it is defined here, provisionally, as the shape this crate
//! reads (the `GDT.DIM.SECT.SPAN` Open questions record the hand-over to `SCAN.SECT`).
//!
//! # Content
//!
//! - `section_id`: the section's ID as the curve states it (`[a-z0-9][a-z0-9._:-]*`, at most
//!   96 bytes). It is a claim of the curve, not checked against any section store.
//! - `frame`: the `fid-geo-frame` [`FrameId`] the plane is expressed in (required).
//! - `plane`: the section plane in that frame: an origin and two in-plane axes, unit length
//!   and orthogonal within [`AXIS_TOL`] ([`SectionPlane`]).
//! - `vertices`: the ordered vertices `[u, v]` in the plane's coordinates (metres along the x
//!   and y axes from the origin); 2 to [`MAX_VERTICES`] (3 for a closed curve), every
//!   coordinate finite, not subnormal and at most [`MAX_COORD_M`] in magnitude.
//! - `closed`: whether a closing segment joins the last vertex to the first.
//! - `digest`: the SHA-256 of the canonical JSON of every other field ([`content_bytes`]). The
//!   loader recomputes it and refuses a curve whose stated digest differs, so the digest a
//!   result records always matches the vertices it was fitted to.
//!
//! # Arc length (pinned)
//!
//! Segment `i` (from vertex `i - 1` to vertex `i`) has length `sqrt(du * du + dv * dv)` with
//! `du = u_i - u_{i-1}`, `dv = v_i - v_{i-1}` (each a single IEEE-754 operation, no fused
//! multiply-add, correctly rounded square root through `fid-num`). The arc-length parameter of
//! vertex 0 is `+0.0` and of vertex `i` is `s_{i-1} + len_i`, summed in vertex order. A closed
//! curve's length adds the closing segment (last vertex to vertex 0); an open curve's length
//! is the parameter of its last vertex. A segment of length zero
//! (`bad_section{zero_length_segment}`) or a sum that does not increase
//! (`bad_section{arc_length_not_increasing}`: the segment is below the rounding resolution of
//! the running sum) is refused, so the parameters are strictly increasing and each vertex has
//! exactly one.
//!
//! # JSON (`fid.scan.sect.curve/1`)
//!
//! ```text
//! {"closed":B,"digest":"sha256:..","format":"fid.scan.sect.curve/1","frame":R,
//!  "plane":{"origin":V,"x_axis":V,"y_axis":V},"section_id":S,"vertices":[[X,X],...]}
//! V = [X,X,X]
//! ```
//!
//! `B` is a JSON boolean; `R` the frame's 32 lowercase hex digits; every `X` the 16 lowercase
//! hex digits of the IEEE-754 bits of a coordinate (no other number form is accepted, and
//! `8000000000000000`, negative zero, is refused so that each value has one text; the
//! constructors store `-0.0` as `+0.0`). The loader refuses a byte-order mark, input over
//! [`MAX_INPUT_BYTES`], duplicate keys (`fid-canon`), unknown or missing fields, and checks
//! `format` before anything else: `fid.scan.sect.curve/<n>` for another `n` is
//! `unsupported_section_schema{version: n}`. Keys may come in any order and with any JSON
//! whitespace; the digest is over the canonical bytes, so it does not depend on either.

use super::SpanError;
use fid_canon::CValue;
use fid_geo_frame::FrameId;
use fid_types::Digest;
use std::cmp::Ordering;
use std::collections::BTreeMap;

/// A 2D point or vector `[u, v]` in the section plane, metres.
pub type V2 = [f64; 2];
/// A 3D point or vector `[x, y, z]` in the curve's frame, metres.
pub type V3 = [f64; 3];

/// The format family; the version follows the slash.
pub const FORMAT_FAMILY: &str = "fid.scan.sect.curve";
/// The only version read.
pub const FORMAT_VERSION: u32 = 1;
/// The format tag, `fid.scan.sect.curve/1`.
pub const FORMAT: &str = "fid.scan.sect.curve/1";
/// Most vertices per curve.
pub const MAX_VERTICES: usize = 1_000_000;
/// Largest coordinate magnitude, metres (vertices and plane origin).
pub const MAX_COORD_M: f64 = 1.0e4;
/// Tolerance on the plane axes: `|x.x - 1|`, `|y.y - 1|` and `|x.y|` must not exceed it. It is
/// about 4500 times the rounding resolution of a three-term dot product of unit vectors
/// (`2^-52`), so an axis pair that was orthonormal before rounding always passes.
pub const AXIS_TOL: f64 = 1e-12;
/// Longest section ID, bytes.
pub const MAX_SECTION_ID_LEN: usize = 96;
/// Largest accepted JSON input, bytes (also `fid-canon`'s cap).
pub const MAX_INPUT_BYTES: usize = 64 * 1024 * 1024;
/// Most parsed JSON elements: three per vertex plus the fixed fields.
const MAX_ELEMENTS: usize = 3 * MAX_VERTICES + 64;

const TOP_FIELDS: [&str; 7] = ["closed", "digest", "format", "frame", "plane", "section_id", "vertices"];
const PLANE_FIELDS: [&str; 3] = ["origin", "x_axis", "y_axis"];

fn bad(reason: &'static str) -> SpanError {
    SpanError::BadSection { reason, index: None }
}

fn bad_at(reason: &'static str, index: usize) -> SpanError {
    SpanError::BadSection { reason, index: Some(index) }
}

/// `-0.0` becomes `+0.0`; every other value is unchanged.
fn unsign_zero(x: f64) -> f64 {
    if x == 0.0 {
        0.0
    } else {
        x
    }
}

fn coord_ok(x: f64) -> bool {
    x.is_finite() && !x.is_subnormal() && x.abs().total_cmp(&MAX_COORD_M) != Ordering::Greater
}

/// Left-to-right dot product, one IEEE-754 operation per step.
fn dot3(a: V3, b: V3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn within(x: f64, tol: f64) -> bool {
    x.abs().total_cmp(&tol) != Ordering::Greater
}

/// A section plane in the curve's frame: origin and two unit, orthogonal in-plane axes. Fields
/// are private; [`SectionPlane::new`] checks them.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SectionPlane {
    origin: V3,
    x_axis: V3,
    y_axis: V3,
}

impl SectionPlane {
    /// Builds a plane. Every component must be finite and not subnormal, the origin within
    /// [`MAX_COORD_M`] (`bad_section{plane_value}`), and the axes unit and orthogonal within
    /// [`AXIS_TOL`] (`bad_section{plane_axes}`). `-0.0` is stored as `+0.0`.
    pub fn new(origin: V3, x_axis: V3, y_axis: V3) -> Result<SectionPlane, SpanError> {
        let all = origin.iter().chain(x_axis.iter()).chain(y_axis.iter());
        if !all.copied().all(coord_ok) {
            return Err(bad("plane_value"));
        }
        let (origin, x_axis, y_axis) = (origin.map(unsign_zero), x_axis.map(unsign_zero), y_axis.map(unsign_zero));
        let unit_x = within(dot3(x_axis, x_axis) - 1.0, AXIS_TOL);
        let unit_y = within(dot3(y_axis, y_axis) - 1.0, AXIS_TOL);
        let orthogonal = within(dot3(x_axis, y_axis), AXIS_TOL);
        if !(unit_x && unit_y && orthogonal) {
            return Err(bad("plane_axes"));
        }
        Ok(SectionPlane { origin, x_axis, y_axis })
    }

    /// Origin, metres in the curve's frame.
    pub fn origin(&self) -> V3 {
        self.origin
    }

    /// In-plane x axis (the `u` direction).
    pub fn x_axis(&self) -> V3 {
        self.x_axis
    }

    /// In-plane y axis (the `v` direction).
    pub fn y_axis(&self) -> V3 {
        self.y_axis
    }

    /// The plane normal `x_axis x y_axis` (component by component, not renormalised).
    pub fn normal(&self) -> V3 {
        let (a, b) = (self.x_axis, self.y_axis);
        [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
    }
}

/// A read-only section curve (module documentation). Fields are private: a view is built only
/// by [`SectionCurveView::new`] or [`SectionCurveView::from_json`], so its arc lengths and
/// digest always match its vertices.
#[derive(Debug, Clone, PartialEq)]
pub struct SectionCurveView {
    section_id: String,
    frame: FrameId,
    plane: SectionPlane,
    vertices: Vec<V2>,
    closed: bool,
    arc: Vec<f64>,
    length: f64,
    digest: Digest,
}

fn is_section_id(s: &str) -> bool {
    let b = s.as_bytes();
    let lower_alnum = |c: u8| c.is_ascii_lowercase() || c.is_ascii_digit();
    !b.is_empty()
        && b.len() <= MAX_SECTION_ID_LEN
        && b.first().is_some_and(|&c| lower_alnum(c))
        && b.iter().all(|&c| lower_alnum(c) || matches!(c, b'.' | b'_' | b':' | b'-'))
}

fn segment(a: V2, b: V2, index: usize) -> Result<f64, SpanError> {
    let du = b[0] - a[0];
    let dv = b[1] - a[1];
    let len = fid_num::f::sqrt(du * du + dv * dv).map_err(|_| bad_at("coordinate", index))?;
    if len == 0.0 {
        return Err(bad_at("zero_length_segment", index));
    }
    Ok(len)
}

fn hex_f64(x: f64) -> CValue {
    CValue::Str(format!("{:016x}", x.to_bits()))
}

fn v3_value(a: V3) -> CValue {
    CValue::Arr(a.iter().map(|&x| hex_f64(x)).collect())
}

impl SectionCurveView {
    /// Builds a view and computes its arc lengths and digest. Checks, in order:
    /// `bad_section{section_id}`; `bad_section{vertex_count}` (fewer than 2, or 3 when
    /// `closed`, or more than [`MAX_VERTICES`]); per vertex in order
    /// `bad_section{coordinate, index}`; then the arc lengths of the module documentation
    /// (`zero_length_segment` / `arc_length_not_increasing` with the segment's end vertex
    /// index, `0` for the closing segment). `-0.0` is stored as `+0.0`.
    pub fn new(
        section_id: &str,
        frame: FrameId,
        plane: SectionPlane,
        mut vertices: Vec<V2>,
        closed: bool,
    ) -> Result<SectionCurveView, SpanError> {
        if !is_section_id(section_id) {
            return Err(bad("section_id"));
        }
        let min = if closed { 3 } else { 2 };
        if vertices.len() < min || vertices.len() > MAX_VERTICES {
            return Err(bad("vertex_count"));
        }
        for (i, p) in vertices.iter_mut().enumerate() {
            if !(coord_ok(p[0]) && coord_ok(p[1])) {
                return Err(bad_at("coordinate", i));
            }
            *p = p.map(unsign_zero);
        }
        let mut arc: Vec<f64> = Vec::new();
        arc.try_reserve_exact(vertices.len()).map_err(|_| SpanError::Allocation)?;
        let mut s = 0.0f64;
        let mut prev: Option<V2> = None;
        for (i, &p) in vertices.iter().enumerate() {
            if let Some(a) = prev {
                let next = s + segment(a, p, i)?;
                if next.total_cmp(&s) != Ordering::Greater {
                    return Err(bad_at("arc_length_not_increasing", i));
                }
                s = next;
            }
            arc.push(s);
            prev = Some(p);
        }
        let length = match (closed, vertices.first(), vertices.last()) {
            (true, Some(&first), Some(&last)) => {
                let next = s + segment(last, first, 0)?;
                if next.total_cmp(&s) != Ordering::Greater {
                    return Err(bad_at("arc_length_not_increasing", 0));
                }
                next
            }
            _ => s,
        };
        let mut view = SectionCurveView {
            section_id: section_id.to_owned(),
            frame,
            plane,
            vertices,
            closed,
            arc,
            length,
            digest: Digest::TOMBSTONE,
        };
        let bytes = content_bytes(&view)?;
        view.digest = Digest::from_bytes(fid_canon::digest_bytes(&bytes).0);
        Ok(view)
    }

    /// Reads a `fid.scan.sect.curve/1` document (module documentation) and checks its stated
    /// digest against the recomputed one (`section_digest_mismatch`). Order: byte-order mark
    /// (`bad_section{bom}`); JSON (`bad_section{<fid-canon code>}`); a top-level object
    /// (`bad_section{wrong_type}`); `format` (`bad_section{format}` or
    /// `unsupported_section_schema{version}`); the field set (`bad_section{unknown_field}`,
    /// `bad_section{missing_field}`); then each field's type and text (`wrong_type`,
    /// `section_id`, `frame`, `plane_value`, `coordinate`, `negative_zero`, `digest`); then
    /// [`SectionCurveView::new`]; then the digest.
    pub fn from_json(bytes: &[u8]) -> Result<SectionCurveView, SpanError> {
        if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
            return Err(bad("bom"));
        }
        let doc = fid_canon::parse_with_budget(bytes, MAX_INPUT_BYTES, 8, MAX_ELEMENTS)
            .map_err(|e| SpanError::BadSection { reason: e.code(), index: None })?;
        let top = match &doc {
            CValue::Obj(o) => o,
            _ => return Err(bad("wrong_type")),
        };
        match top.get("format") {
            Some(CValue::Str(f)) => check_format(f)?,
            Some(_) => return Err(bad("wrong_type")),
            None => return Err(bad("missing_field")),
        }
        fields(top, &TOP_FIELDS)?;
        let section_id = match top.get("section_id") {
            Some(CValue::Str(s)) => s.as_str(),
            _ => return Err(bad("wrong_type")),
        };
        let frame = match top.get("frame") {
            Some(CValue::Str(s)) if is_lower_hex(s, 32) => FrameId::parse(s).map_err(|_| bad("frame"))?,
            Some(CValue::Str(_)) => return Err(bad("frame")),
            _ => return Err(bad("wrong_type")),
        };
        let plane = match top.get("plane") {
            Some(CValue::Obj(p)) => {
                fields(p, &PLANE_FIELDS)?;
                let get = |k: &str| match p.get(k) {
                    Some(v) => v3_of(v, "plane_value"),
                    None => Err(bad("missing_field")),
                };
                SectionPlane::new(get("origin")?, get("x_axis")?, get("y_axis")?)?
            }
            _ => return Err(bad("wrong_type")),
        };
        let closed = match top.get("closed") {
            Some(CValue::Bool(b)) => *b,
            _ => return Err(bad("wrong_type")),
        };
        let raw = match top.get("vertices") {
            Some(CValue::Arr(a)) => a,
            _ => return Err(bad("wrong_type")),
        };
        if raw.len() > MAX_VERTICES {
            return Err(bad("vertex_count"));
        }
        let mut vertices: Vec<V2> = Vec::new();
        vertices.try_reserve_exact(raw.len()).map_err(|_| SpanError::Allocation)?;
        for (i, v) in raw.iter().enumerate() {
            match v {
                CValue::Arr(uv) if uv.len() == 2 => {
                    let mut p = [0.0f64; 2];
                    for (slot, x) in p.iter_mut().zip(uv.iter()) {
                        *slot = f64_of(x).map_err(|reason| bad_at(reason, i))?;
                    }
                    vertices.push(p);
                }
                _ => return Err(bad_at("wrong_type", i)),
            }
        }
        let stated = match top.get("digest") {
            Some(CValue::Str(s)) => Digest::parse(s).map_err(|_| bad("digest"))?,
            _ => return Err(bad("wrong_type")),
        };
        let view = SectionCurveView::new(section_id, frame, plane, vertices, closed)?;
        if view.digest != stated {
            return Err(SpanError::SectionDigestMismatch);
        }
        Ok(view)
    }

    /// The full canonical JSON document, digest included (RFC 8785, `fid-canon`).
    pub fn to_canonical_json(&self) -> Result<Vec<u8>, SpanError> {
        let mut o = content_value(self)?;
        o.insert("digest".to_owned(), CValue::Str(self.digest.as_str().to_owned()));
        fid_canon::to_canonical_bytes(&CValue::Obj(o))
            .map_err(|e| SpanError::BadSection { reason: e.code(), index: None })
    }

    /// The section ID the curve states.
    pub fn section_id(&self) -> &str {
        &self.section_id
    }

    /// The frame the plane is expressed in.
    pub fn frame(&self) -> FrameId {
        self.frame
    }

    /// The section plane.
    pub fn plane(&self) -> &SectionPlane {
        &self.plane
    }

    /// The vertices `[u, v]` in curve order, metres in the plane.
    pub fn vertices(&self) -> &[V2] {
        &self.vertices
    }

    /// Whether the curve is closed.
    pub fn closed(&self) -> bool {
        self.closed
    }

    /// The arc-length parameter of each vertex, metres, strictly increasing from `+0.0`.
    pub fn arc_lengths(&self) -> &[f64] {
        &self.arc
    }

    /// The curve length, metres (including the closing segment of a closed curve).
    pub fn length(&self) -> f64 {
        self.length
    }

    /// The SHA-256 of [`content_bytes`]: recomputed from the vertices, never taken on trust.
    pub fn digest(&self) -> Digest {
        self.digest
    }
}

fn content_value(view: &SectionCurveView) -> Result<BTreeMap<String, CValue>, SpanError> {
    let mut verts: Vec<CValue> = Vec::new();
    verts.try_reserve_exact(view.vertices.len()).map_err(|_| SpanError::Allocation)?;
    for p in &view.vertices {
        verts.push(CValue::Arr(vec![hex_f64(p[0]), hex_f64(p[1])]));
    }
    let plane = BTreeMap::from([
        ("origin".to_owned(), v3_value(view.plane.origin)),
        ("x_axis".to_owned(), v3_value(view.plane.x_axis)),
        ("y_axis".to_owned(), v3_value(view.plane.y_axis)),
    ]);
    Ok(BTreeMap::from([
        ("closed".to_owned(), CValue::Bool(view.closed)),
        ("format".to_owned(), CValue::Str(FORMAT.to_owned())),
        ("frame".to_owned(), CValue::Str(view.frame.to_string())),
        ("plane".to_owned(), CValue::Obj(plane)),
        ("section_id".to_owned(), CValue::Str(view.section_id.clone())),
        ("vertices".to_owned(), CValue::Arr(verts)),
    ]))
}

/// The canonical JSON (RFC 8785) of every field of the curve except `digest`: the bytes its
/// digest is the SHA-256 of.
pub fn content_bytes(view: &SectionCurveView) -> Result<Vec<u8>, SpanError> {
    let v = CValue::Obj(content_value(view)?);
    fid_canon::to_canonical_bytes(&v).map_err(|e| SpanError::BadSection { reason: e.code(), index: None })
}

fn check_format(f: &str) -> Result<(), SpanError> {
    if f == FORMAT {
        return Ok(());
    }
    match f.strip_prefix(FORMAT_FAMILY).and_then(|r| r.strip_prefix('/')) {
        Some(v) if !v.is_empty() && v.len() <= 9 && v.bytes().all(|c| c.is_ascii_digit()) => {
            Err(SpanError::UnsupportedSectionSchema { version: v.to_owned() })
        }
        _ => Err(bad("format")),
    }
}

fn fields(o: &BTreeMap<String, CValue>, allowed: &[&str]) -> Result<(), SpanError> {
    if o.keys().any(|k| !allowed.contains(&k.as_str())) {
        return Err(bad("unknown_field"));
    }
    if allowed.iter().any(|k| !o.contains_key(*k)) {
        return Err(bad("missing_field"));
    }
    Ok(())
}

fn is_lower_hex(s: &str, len: usize) -> bool {
    s.len() == len && s.bytes().all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}

/// One coordinate: 16 lowercase hex digits of the bits; negative zero and non-finite values
/// are refused here, the range in [`SectionCurveView::new`] / [`SectionPlane::new`].
fn f64_of(v: &CValue) -> Result<f64, &'static str> {
    let s = match v {
        CValue::Str(s) => s,
        _ => return Err("wrong_type"),
    };
    if !is_lower_hex(s, 16) {
        return Err("coordinate");
    }
    let bits = u64::from_str_radix(s, 16).map_err(|_| "coordinate")?;
    if bits == 1u64 << 63 {
        return Err("negative_zero");
    }
    let x = f64::from_bits(bits);
    if !x.is_finite() {
        return Err("coordinate");
    }
    Ok(x)
}

fn v3_of(v: &CValue, reason: &'static str) -> Result<V3, SpanError> {
    match v {
        CValue::Arr(a) if a.len() == 3 => {
            let mut out = [0.0f64; 3];
            for (slot, x) in out.iter_mut().zip(a.iter()) {
                *slot = f64_of(x).map_err(|r| if r == "coordinate" { bad(reason) } else { bad(r) })?;
            }
            Ok(out)
        }
        _ => Err(bad("wrong_type")),
    }
}
