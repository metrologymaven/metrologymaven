//! Shared edge discretisation. Spec node `GEO.CAD.TESS.EDGE`.
//!
//! [`tessellate_edges`] samples every edge of a checked [`Body`] exactly once, under a chordal
//! tolerance `t` and an angular tolerance `a` ([`EdgeTessParams`]), gives each body vertex and
//! each interior edge sample one shared mesh vertex index, and maps the samples into the face
//! chart of every coedge, so the two faces that meet at an edge are later meshed against the
//! same boundary points (REQ-002). [`sample_curve`] is the per-edge step on its own.
//!
//! # Pinned rules (value-affecting; `fixtures/geo/cad/tess/make_tess_fixtures.py` checks them
//! against an independent 60-digit closed-form oracle)
//!
//! **Points.** `P(s)` is the edge curve at `s` and `T(s)` its first derivative, both from the
//! GEO.NURBS evaluators. A NURBS evaluator that clamps `s` to its domain is an error
//! ([`SampleError::CurveEval`]), never a silent move.
//!
//! **Segment check.** A segment `(L, R)` with parameters `tl < tr` has the midpoint
//! `tm = (tl + tr) * 0.5` and the quarter points `tq1 = (tl + tm) * 0.5`,
//! `tq3 = (tm + tr) * 0.5` (the midpoints of its two halves). Its chordal error is the largest
//! distance from `P(tq1)`, `P(tm)`, `P(tq3)` to the chord *segment* `[P(tl), P(tr)]` (to the
//! point `P(tl)` when the chord has zero length, as for the first segment of a closed edge).
//! Its angular error is the largest of the angles between `T(tl)` and `T(tr)`, and between
//! `T(s)` and each of `T(tl)`, `T(tr)` for `s` in `tq1`, `tm`, `tq3`. The angle between two
//! tangents is `atan2(|A x B|, A . B)` (fid-num's KNP-1 `atan2`) after each tangent is scaled
//! by its largest absolute component; a tangent whose components are all zero (a degenerate
//! point) or that is not finite makes the angle infinite. The segment passes when its
//! chordal error is at most `t` and its angular error at most `a` (REQ-001).
//! The sufficiency of three check points is \[unverified\], as the spec says.
//!
//! **Bisection.** The edge starts as the single segment `(t0, t1)` at depth 0. Segments are
//! taken left to right from a stack. A segment that passes, or that is at depth `max_depth`
//! ([`MAX_DEPTH`] = 24 by default), or whose midpoint does not lie strictly between its ends
//! in binary64, is kept: its right parameter is appended to the sample list (which starts with
//! `t0`). Any other segment is replaced by its halves `(L, tm)` and `(tm, R)` at depth + 1, the
//! left half first (REQ-004). A kept segment that does not pass makes the edge out of
//! tolerance: [`EdgeReport::EdgeTolerance`] carries the largest chordal and angular errors of
//! all kept segments, and the samples are kept, so both faces still share them (REQ-005).
//!
//! **Vertex indices.** Edges are processed in ascending [`TopologyId`] order. Walking each
//! edge's samples in order, an end sample takes the index of its body vertex (a new index the
//! first time that vertex is met) and every interior sample a new index, so one body vertex
//! is one mesh vertex and the indices depend only on the IDs, never on the canonical order
//! (REQ-002, REQ-004). The mesh point of a body vertex is its stored point; the point of an
//! interior sample is `P(s)`, on the curve by construction. The distance between `P(t0)`,
//! `P(t1)` and the stored end vertices is checked against [`ON_CURVE_TOL_M`] = 1e-12 m and
//! reported as [`EdgeReport::VertexOffCurve`] when larger (REQ-003).
//!
//! **Surface gap.** For each distinct face of an edge's coedges (ascending face order) every
//! mesh point of the edge is projected onto the face surface: an analytic surface by its
//! closed form (the distance counts whether or not the foot is unique), a NURBS surface by
//! GEO.NURBS.INVERT. A certified answer is a measured distance; an uncertified one is its best
//! known distance, an upper bound, and sets `gap_upper_bound`. `max_surface_gap_m` is the
//! largest distance, or `None` when any distance could not be obtained (then
//! [`EdgeReport::GapUnmeasured`] names the face): a partial maximum is never reported as the
//! maximum (REQ-003).
//!
//! **Coedge uv.** A coedge walks its edge's samples in its own direction (reversed for a
//! `Reversed` coedge), and its `vertex_ix` is the edge's list in that order. Its uv points come
//! from its GEO.CAD.TOPO.PCURVE trim curve plus the chart offset of its loop: an imported pcurve
//! is evaluated at the sample parameter (same-parameter convention; a clamping NURBS pcurve is
//! [`UvError::PcurveEval`]); a derived polyline is interpolated linearly in `t`, an exact
//! parameter hit taking the first polyline vertex at that parameter. A seam edge has two
//! coedges in one face with their own trim curves, so its one sample list appears at both
//! sides of the chart. A coedge without a trim curve keeps its `vertex_ix`; only its uv is an
//! error.
//!
//! # Determinism (D0, KNP-1)
//!
//! Binary64 `+ - * /` written out one operation at a time (no fused multiply-add, comparisons
//! instead of the float minimum and maximum functions), `hypot` and `atan2` from `fid_num::f`,
//! curve and surface evaluation and inversion only inside GEO.NURBS. Fixed order everywhere,
//! a stack bounded by the depth limit, vectors only, every loop bounded by
//! [`EdgeTessParams::max_samples`]. Nothing panics.

use core::fmt;

use fid_geo_nurbs::{invert_surface, Curve, EvalError, InvertError, Status};

use crate::topo::geom::{Curve2, CurveDef};
use crate::topo::model::{Body, CoedgeIx, EdgeIx, FaceIx, Sense, TopologyId};
use crate::topo::pcurve::{dist3, hypot, larger, pcurve_point, PcurveError, Pcurves, Surf, TrimCurve};

/// REQ-004: bisection depth limit below the whole edge range.
pub const MAX_DEPTH: u32 = 24;
/// Default bound on the mesh vertices of one call (all edges together): 2^22.
pub const DEFAULT_MAX_SAMPLES: usize = 4_194_304;
/// REQ-003: an end vertex farther than this from its edge curve is reported, metres.
pub const ON_CURVE_TOL_M: f64 = 1e-12;

// ---------------------------------------------------------------- public types

/// Tolerances and limits (`EdgeTessParams` of the spec, plus the resource bound).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EdgeTessParams {
    /// Chordal tolerance `t`, metres; finite and positive.
    pub chordal_m: f64,
    /// Angular tolerance `a`, radians; finite, positive and at most pi.
    pub angular_rad: f64,
    /// Bisection depth limit, at most [`MAX_DEPTH`].
    pub max_depth: u32,
    /// Most mesh vertices in one call, at least 2; exceeding it is
    /// [`SampleError::TooManySamples`] (fail closed, nothing is returned).
    pub max_samples: usize,
}

impl EdgeTessParams {
    /// Checked parameters with depth limit [`MAX_DEPTH`] and [`DEFAULT_MAX_SAMPLES`].
    pub fn new(chordal_m: f64, angular_rad: f64) -> Result<EdgeTessParams, SampleError> {
        let p = EdgeTessParams { chordal_m, angular_rad, max_depth: MAX_DEPTH, max_samples: DEFAULT_MAX_SAMPLES };
        p.check()?;
        Ok(p)
    }

    /// Every field in range (the fields are public, so every entry point calls this).
    pub fn check(&self) -> Result<(), SampleError> {
        if !(self.chordal_m.is_finite() && self.chordal_m > 0.0) {
            return Err(SampleError::BadParams { what: "chordal_m" });
        }
        if !(self.angular_rad.is_finite() && self.angular_rad > 0.0 && self.angular_rad <= core::f64::consts::PI) {
            return Err(SampleError::BadParams { what: "angular_rad" });
        }
        if self.max_depth > MAX_DEPTH {
            return Err(SampleError::BadParams { what: "max_depth" });
        }
        if self.max_samples < 2 {
            return Err(SampleError::BadParams { what: "max_samples" });
        }
        Ok(())
    }
}

/// Why [`sample_curve`] (or the whole of [`tessellate_edges`]) has no result.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SampleError {
    /// A parameter is out of range (the field name).
    BadParams { what: &'static str },
    /// The range is not finite with `t0 < t1`.
    BadRange,
    /// The curve could not be evaluated at `t` (or was clamped, or a distance overflowed).
    CurveEval { t: f64 },
    /// More than `limit` samples would be produced.
    TooManySamples { limit: usize },
}

impl SampleError {
    /// Stable variant name.
    pub fn code(&self) -> &'static str {
        match self {
            SampleError::BadParams { .. } => "BadParams",
            SampleError::BadRange => "BadRange",
            SampleError::CurveEval { .. } => "CurveEval",
            SampleError::TooManySamples { .. } => "TooManySamples",
        }
    }
}

impl fmt::Display for SampleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SampleError::BadParams { what } => write!(f, "edge tessellation parameter {what} is out of range"),
            SampleError::BadRange => f.write_str("edge parameter range is not finite and increasing"),
            SampleError::CurveEval { t } => write!(f, "edge curve cannot be evaluated at t = {t:e}"),
            SampleError::TooManySamples { limit } => write!(f, "more than {limit} edge samples"),
        }
    }
}

impl core::error::Error for SampleError {}

/// Samples of one curve range ([`sample_curve`]).
#[derive(Debug, Clone, PartialEq)]
pub struct CurveSamples {
    /// Parameters, strictly increasing, from `t0` to `t1`.
    pub t: Vec<f64>,
    /// `P(t)` for every parameter.
    pub points: Vec<[f64; 3]>,
    /// Every kept segment passed both tolerances.
    pub within_tolerance: bool,
    /// Largest chordal error of the kept segments, metres.
    pub achieved_m: f64,
    /// Largest angular error of the kept segments, radians (infinite at a degenerate tangent).
    pub achieved_rad: f64,
}

/// The samples of one edge (`EdgeSamples` of the spec, with its gap report).
#[derive(Debug, Clone, PartialEq)]
pub struct EdgeSamples {
    pub edge: EdgeIx,
    /// Edge parameters, strictly increasing (just `[t0, t1]` for an edge whose curve is
    /// missing or cannot be evaluated; see [`EdgeTess::reports`]).
    pub t: Vec<f64>,
    /// Mesh vertex index of every sample; [`EdgeTess::points`] holds the points.
    pub vertex_ix: Vec<u64>,
    /// Largest chordal error of the kept segments, metres (infinite when not sampled).
    pub achieved_m: f64,
    /// Largest angular error of the kept segments, radians (infinite when not sampled).
    pub achieved_rad: f64,
    /// REQ-003: largest distance from a mesh point of the edge to an adjacent face surface;
    /// `None` when any distance could not be obtained.
    pub max_surface_gap_m: Option<f64>,
    /// Some distance in the gap is an uncertified NURBS inversion's upper bound.
    pub gap_upper_bound: bool,
}

/// Why a coedge has no uv points.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum UvError {
    /// GEO.CAD.TOPO.PCURVE gave this coedge no trim curve.
    Pcurve(PcurveError),
    /// The imported pcurve could not be evaluated at `t` (or clamped it).
    PcurveEval { t: f64 },
    /// `t` lies outside the derived polyline's parameters.
    OutsideTrim { t: f64 },
    /// An interpolated or offset uv is not finite.
    NonFinite { t: f64 },
}

impl UvError {
    /// Stable variant name.
    pub fn code(&self) -> &'static str {
        match self {
            UvError::Pcurve(_) => "Pcurve",
            UvError::PcurveEval { .. } => "PcurveEval",
            UvError::OutsideTrim { .. } => "OutsideTrim",
            UvError::NonFinite { .. } => "NonFinite",
        }
    }
}

/// The edge samples seen from one coedge (`CoedgeUv` of the spec, with its vertex indices).
#[derive(Debug, Clone, PartialEq)]
pub struct CoedgeUv {
    pub coedge: CoedgeIx,
    /// The edge's `vertex_ix` in the coedge's direction (reversed for `Reversed`).
    pub vertex_ix: Vec<u64>,
    /// The samples in the face's unwrapped chart, in the same order.
    pub uv: Result<Vec<[f64; 2]>, UvError>,
}

/// Non-fatal findings, in ascending edge-ID order (per edge: sampling, then vertices, then
/// gaps by ascending face).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum EdgeReport {
    /// REQ-005: the tolerances were not met within the depth limit; samples kept.
    EdgeTolerance { edge: EdgeIx, achieved_m: f64, achieved_rad: f64 },
    /// The edge has no 3-D curve: only its end vertices are used.
    NoEdgeCurve { edge: EdgeIx },
    /// The edge curve could not be evaluated: only its end vertices are used.
    CurveEval { edge: EdgeIx, t: f64 },
    /// REQ-003: an end vertex is farther than [`ON_CURVE_TOL_M`] from `P(t0)` or `P(t1)`
    /// (infinite when the distance overflowed).
    VertexOffCurve { edge: EdgeIx, gap_m: f64 },
    /// A distance to this face's surface could not be obtained.
    GapUnmeasured { edge: EdgeIx, face: FaceIx },
}

impl EdgeReport {
    /// Stable variant name.
    pub fn code(&self) -> &'static str {
        match self {
            EdgeReport::EdgeTolerance { .. } => "EdgeTolerance",
            EdgeReport::NoEdgeCurve { .. } => "NoEdgeCurve",
            EdgeReport::CurveEval { .. } => "CurveEval",
            EdgeReport::VertexOffCurve { .. } => "VertexOffCurve",
            EdgeReport::GapUnmeasured { .. } => "GapUnmeasured",
        }
    }

    /// The edge reported.
    pub fn edge(&self) -> EdgeIx {
        match *self {
            EdgeReport::EdgeTolerance { edge, .. }
            | EdgeReport::NoEdgeCurve { edge }
            | EdgeReport::CurveEval { edge, .. }
            | EdgeReport::VertexOffCurve { edge, .. }
            | EdgeReport::GapUnmeasured { edge, .. } => edge,
        }
    }
}

/// Why [`tessellate_edges`] has no result.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum EdgeTessError {
    /// The parameters are out of range, or the vertex bound was exceeded.
    Sample(SampleError),
    /// The [`Pcurves`] do not belong to this body (coedge, face or loop counts differ).
    PcurvesMismatch,
}

impl EdgeTessError {
    /// Stable variant name.
    pub fn code(&self) -> &'static str {
        match self {
            EdgeTessError::Sample(e) => e.code(),
            EdgeTessError::PcurvesMismatch => "PcurvesMismatch",
        }
    }
}

impl fmt::Display for EdgeTessError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EdgeTessError::Sample(e) => e.fmt(f),
            EdgeTessError::PcurvesMismatch => f.write_str("the pcurves do not belong to this body"),
        }
    }
}

impl core::error::Error for EdgeTessError {}

impl From<SampleError> for EdgeTessError {
    fn from(e: SampleError) -> Self {
        EdgeTessError::Sample(e)
    }
}

/// Result of [`tessellate_edges`].
#[derive(Debug, Clone, PartialEq)]
pub struct EdgeTess {
    /// Per edge, in canonical edge order.
    pub edges: Vec<EdgeSamples>,
    /// Per coedge, in canonical coedge order.
    pub coedges: Vec<CoedgeUv>,
    /// Mesh point of every vertex index.
    pub points: Vec<[f64; 3]>,
    /// Non-fatal findings (see [`EdgeReport`]).
    pub reports: Vec<EdgeReport>,
}

// ---------------------------------------------------------------- small geometry

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    (a[0] * b[0] + a[1] * b[1]) + a[2] * b[2]
}

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn abs(x: f64) -> f64 {
    if x < 0.0 {
        -x
    } else {
        x
    }
}

/// The tangent divided by its largest absolute component; `None` when degenerate.
fn scaled(d: [f64; 3]) -> Option<[f64; 3]> {
    if !(d[0].is_finite() && d[1].is_finite() && d[2].is_finite()) {
        return None;
    }
    let m = larger(larger(abs(d[0]), abs(d[1])), abs(d[2]));
    if m > 0.0 {
        Some([d[0] / m, d[1] / m, d[2] / m])
    } else {
        None
    }
}

/// Angle between two tangents (module docs); infinite when either is degenerate.
fn angle(a: [f64; 3], b: [f64; 3]) -> f64 {
    let (Some(a), Some(b)) = (scaled(a), scaled(b)) else {
        return f64::INFINITY;
    };
    let c = [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
    let n = match hypot(c[0], c[1]).and_then(|h| hypot(h, c[2])) {
        Some(n) => n,
        None => return f64::INFINITY,
    };
    match fid_num::f::atan2(n, dot(a, b)) {
        Ok(x) if x.is_finite() => x,
        _ => f64::INFINITY,
    }
}

/// Distance from `p` to the segment `[a, b]` (to `a` when the segment has zero length).
fn seg_dist(p: [f64; 3], a: [f64; 3], b: [f64; 3]) -> Option<f64> {
    let e = sub(b, a);
    let ee = dot(e, e);
    if !ee.is_finite() {
        return None;
    }
    if ee == 0.0 {
        return dist3(p, a);
    }
    let s = dot(sub(p, a), e) / ee;
    let s = if s < 0.0 {
        0.0
    } else if s > 1.0 {
        1.0
    } else {
        s
    };
    let foot = [a[0] + s * e[0], a[1] + s * e[1], a[2] + s * e[2]];
    dist3(p, foot)
}

// ---------------------------------------------------------------- one curve

#[derive(Clone, Copy)]
struct Pt {
    t: f64,
    p: [f64; 3],
    d: [f64; 3],
}

/// `P(t)` and `T(t)`; a NURBS evaluator that clamps `t` is an error.
fn point(c: &CurveDef, t: f64) -> Result<Pt, SampleError> {
    let r = if let Some(a) = c.as_analytic() {
        a.eval(t, 1)
    } else if let Some(n) = c.as_nurbs() {
        n.eval(t, 1)
    } else {
        Err(EvalError::Inconsistent)
    };
    let fail = SampleError::CurveEval { t };
    let d = r.map_err(|_| fail)?;
    if d.t.to_bits() != t.to_bits() {
        return Err(fail);
    }
    let d1 = d.d1.ok_or(fail)?;
    let all = [d.p[0], d.p[1], d.p[2], d1[0], d1[1], d1[2]];
    if !all.iter().all(|x| x.is_finite()) {
        return Err(fail);
    }
    Ok(Pt { t, p: d.p, d: d1 })
}

/// Chordal and angular errors of the segment `(l, r)` with check points `qs`.
fn errors(l: &Pt, r: &Pt, qs: [&Pt; 3]) -> Result<(f64, f64), SampleError> {
    let mut em = 0.0;
    let mut ea = angle(l.d, r.d);
    for q in qs {
        let d = seg_dist(q.p, l.p, r.p).ok_or(SampleError::CurveEval { t: q.t })?;
        em = larger(em, d);
        ea = larger(ea, angle(l.d, q.d));
        ea = larger(ea, angle(q.d, r.d));
    }
    Ok((em, ea))
}

/// Samples `range` of `curve` by the pinned bisection (module docs). Pure: the result
/// depends on the curve, the range and the parameters only.
pub fn sample_curve(curve: &CurveDef, range: [f64; 2], params: &EdgeTessParams) -> Result<CurveSamples, SampleError> {
    params.check()?;
    let [t0, t1] = range;
    if !(t0.is_finite() && t1.is_finite() && t0 < t1) {
        return Err(SampleError::BadRange);
    }
    let a = point(curve, t0)?;
    let b = point(curve, t1)?;
    let mut out =
        CurveSamples { t: vec![t0], points: vec![a.p], within_tolerance: true, achieved_m: 0.0, achieved_rad: 0.0 };
    let mut stack: Vec<(Pt, Pt, u32)> = vec![(a, b, 0)];
    while let Some((l, r, depth)) = stack.pop() {
        let tm = (l.t + r.t) * 0.5;
        let m = point(curve, tm)?;
        let q1 = point(curve, (l.t + tm) * 0.5)?;
        let q3 = point(curve, (tm + r.t) * 0.5)?;
        let (em, ea) = errors(&l, &r, [&q1, &m, &q3])?;
        let pass = em <= params.chordal_m && ea <= params.angular_rad;
        let splittable = l.t < tm && tm < r.t;
        if pass || depth >= params.max_depth || !splittable {
            if !pass {
                out.within_tolerance = false;
            }
            out.achieved_m = larger(out.achieved_m, em);
            out.achieved_rad = larger(out.achieved_rad, ea);
            if out.t.len() >= params.max_samples {
                return Err(SampleError::TooManySamples { limit: params.max_samples });
            }
            out.t.push(r.t);
            out.points.push(r.p);
        } else {
            stack.push((m, r, depth + 1));
            stack.push((l, m, depth + 1));
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------- surface gap

enum Gap {
    Measured(f64),
    UpperBound(f64),
    Unmeasured,
}

fn surface_gap(s: Surf<'_>, q: [f64; 3]) -> Gap {
    let g = match s {
        Surf::A(a) => match a.invert(q) {
            Ok(f) => Gap::Measured(f.distance),
            Err(_) => Gap::Unmeasured,
        },
        Surf::N(n) => match invert_surface(n, q) {
            Ok(i) => match i.status {
                Status::Certified { .. } => Gap::Measured(i.distance),
                Status::Unresolved { .. } => Gap::UpperBound(i.distance),
            },
            Err(InvertError::NonConvergence { best }) => Gap::UpperBound(best.distance),
            Err(_) => Gap::Unmeasured,
        },
    };
    match g {
        Gap::Measured(d) | Gap::UpperBound(d) if !(d.is_finite() && d >= 0.0) => Gap::Unmeasured,
        g => g,
    }
}

// ---------------------------------------------------------------- coedge uv

fn imported_uv(p: &Curve2, t: f64) -> Result<[f64; 2], UvError> {
    let fail = UvError::PcurveEval { t };
    if let Some(n) = p.as_nurbs() {
        let d = n.eval(t, 0).map_err(|_| fail)?;
        if d.t.to_bits() != t.to_bits() {
            return Err(fail);
        }
        return Ok([d.p[0], d.p[1]]);
    }
    pcurve_point(p, t).map_err(|_| fail)
}

/// Linear interpolation of a derived polyline at the samples `ts` (all in coedge order);
/// `sign` is 1 for a `Same` and -1 for a `Reversed` coedge, so `sign * t` never decreases.
fn derived_uv(uv: &[[f64; 2]], pt: &[f64], ts: &[f64], sign: f64) -> Result<Vec<[f64; 2]>, UvError> {
    let mut out = Vec::with_capacity(ts.len());
    let mut i = 0usize;
    for &t in ts {
        let s = sign * t;
        let outside = UvError::OutsideTrim { t };
        let first = pt.first().ok_or(outside)?;
        if s < sign * *first {
            return Err(outside);
        }
        while i + 1 < pt.len() && pt.get(i + 1).is_some_and(|&x| sign * x < s) {
            i += 1;
        }
        let (si, ui) = match (pt.get(i), uv.get(i)) {
            (Some(&x), Some(&u)) => (sign * x, u),
            _ => return Err(outside),
        };
        if si == s {
            out.push(ui);
            continue;
        }
        let (sj, uj) = match (pt.get(i + 1), uv.get(i + 1)) {
            (Some(&x), Some(&u)) => (sign * x, u),
            _ => return Err(outside),
        };
        if sj == s {
            out.push(uj);
            continue;
        }
        if !(si < s && s < sj) {
            return Err(outside);
        }
        let f = (s - si) / (sj - si);
        let p = [ui[0] + (uj[0] - ui[0]) * f, ui[1] + (uj[1] - ui[1]) * f];
        if !(p[0].is_finite() && p[1].is_finite()) {
            return Err(UvError::NonFinite { t });
        }
        out.push(p);
    }
    Ok(out)
}

fn coedge_uv(
    trim: &Result<TrimCurve, PcurveError>,
    ts: &[f64],
    sign: f64,
    off: [f64; 2],
) -> Result<Vec<[f64; 2]>, UvError> {
    let raw = match trim {
        Err(e) => return Err(UvError::Pcurve(*e)),
        Ok(TrimCurve::Imported(p)) => ts.iter().map(|&t| imported_uv(p, t)).collect::<Result<Vec<_>, _>>()?,
        Ok(TrimCurve::Derived { uv, t, .. }) => derived_uv(uv, t, ts, sign)?,
    };
    let mut out = Vec::with_capacity(raw.len());
    for (p, &t) in raw.into_iter().zip(ts) {
        let q = [p[0] + off[0], p[1] + off[1]];
        if !(q[0].is_finite() && q[1].is_finite()) {
            return Err(UvError::NonFinite { t });
        }
        out.push(q);
    }
    Ok(out)
}

// ---------------------------------------------------------------- the body

/// Samples every edge of `body` once and maps the samples into every coedge's chart (module
/// docs). `pcurves` must be [`derive_pcurves`](crate::derive_pcurves) of this body; the
/// caller chooses its tolerance.
pub fn tessellate_edges(body: &Body, pcurves: &Pcurves, params: &EdgeTessParams) -> Result<EdgeTess, EdgeTessError> {
    params.check()?;
    let n_edges = body.edges().len();
    if pcurves.curves.len() != body.coedges().len() || pcurves.charts.len() != body.faces().len() {
        return Err(EdgeTessError::PcurvesMismatch);
    }
    for (f, c) in body.faces().iter().zip(&pcurves.charts) {
        if f.loops().len() != c.loop_offsets.len() {
            return Err(EdgeTessError::PcurvesMismatch);
        }
    }

    // Coedges of every edge, ascending coedge order.
    let mut of_edge: Vec<Vec<CoedgeIx>> = vec![Vec::new(); n_edges];
    for (k, c) in body.coedges().iter().enumerate() {
        let slot = of_edge.get_mut(c.edge().index()).ok_or(EdgeTessError::PcurvesMismatch)?;
        slot.push(CoedgeIx::at(k));
    }

    let mut order: Vec<usize> = (0..n_edges).collect();
    let ids: Vec<&TopologyId> = body.edges().iter().map(|e| e.id()).collect();
    order.sort_by(|&a, &b| ids.get(a).cmp(&ids.get(b)));

    let mut vertex_of: Vec<Option<u64>> = vec![None; body.vertices().len()];
    let mut points: Vec<[f64; 3]> = Vec::new();
    let mut reports: Vec<EdgeReport> = Vec::new();
    let mut slots: Vec<Option<EdgeSamples>> = vec![None; n_edges];
    let limit = params.max_samples;
    let too_many = EdgeTessError::Sample(SampleError::TooManySamples { limit });

    for &e in &order {
        let edge = body.edges().get(e).ok_or(EdgeTessError::PcurvesMismatch)?;
        let eix = EdgeIx::at(e);
        let range = edge.param_range();
        let (vs, ve) = (edge.start(), edge.end());
        let pv = |v: crate::topo::model::VertexIx| body.vertex(v).map(|x| x.point());
        let (ps, pe) = match (pv(vs), pv(ve)) {
            (Some(a), Some(b)) => (a, b),
            _ => return Err(EdgeTessError::PcurvesMismatch),
        };

        let sampled = match edge.curve() {
            None => {
                reports.push(EdgeReport::NoEdgeCurve { edge: eix });
                None
            }
            Some(c) => match sample_curve(c, range, params) {
                Ok(s) => Some(s),
                Err(SampleError::CurveEval { t }) => {
                    reports.push(EdgeReport::CurveEval { edge: eix, t });
                    None
                }
                Err(err) => return Err(err.into()),
            },
        };
        let (ts, curve_pts, achieved_m, achieved_rad) = match &sampled {
            Some(s) => {
                if !s.within_tolerance {
                    reports.push(EdgeReport::EdgeTolerance {
                        edge: eix,
                        achieved_m: s.achieved_m,
                        achieved_rad: s.achieved_rad,
                    });
                }
                let first = s.points.first().copied();
                let last = s.points.last().copied();
                let gap = match (first, last) {
                    (Some(a), Some(b)) => match (dist3(a, ps), dist3(b, pe)) {
                        (Some(x), Some(y)) => larger(x, y),
                        _ => f64::INFINITY,
                    },
                    _ => f64::INFINITY,
                };
                if gap > ON_CURVE_TOL_M {
                    reports.push(EdgeReport::VertexOffCurve { edge: eix, gap_m: gap });
                }
                (s.t.clone(), s.points.clone(), s.achieved_m, s.achieved_rad)
            }
            None => (vec![range[0], range[1]], vec![ps, pe], f64::INFINITY, f64::INFINITY),
        };

        // Vertex indices: ends share the body vertex, interior samples get new indices.
        let n = ts.len();
        let mut vix: Vec<u64> = Vec::with_capacity(n);
        let mut mesh_pts: Vec<[f64; 3]> = Vec::with_capacity(n);
        for (k, p) in curve_pts.iter().enumerate() {
            let end = if k == 0 {
                Some((vs, ps))
            } else if k + 1 == n {
                Some((ve, pe))
            } else {
                None
            };
            let ix = match end {
                Some((v, vp)) => {
                    let slot = vertex_of.get_mut(v.index()).ok_or(EdgeTessError::PcurvesMismatch)?;
                    match *slot {
                        Some(ix) => ix,
                        None => {
                            if points.len() >= limit {
                                return Err(too_many);
                            }
                            let ix = points.len() as u64;
                            points.push(vp);
                            *slot = Some(ix);
                            ix
                        }
                    }
                }
                None => {
                    if points.len() >= limit {
                        return Err(too_many);
                    }
                    let ix = points.len() as u64;
                    points.push(*p);
                    ix
                }
            };
            mesh_pts.push(match end {
                Some((_, vp)) => vp,
                None => *p,
            });
            vix.push(ix);
        }

        // Surface gap against every distinct face of the edge's coedges.
        let mut faces: Vec<FaceIx> = Vec::new();
        for &c in of_edge.get(e).map(Vec::as_slice).unwrap_or(&[]) {
            let f = body.coedge(c).and_then(|c| body.r#loop(c.r#loop())).map(|l| l.face());
            if let Some(f) = f {
                if !faces.contains(&f) {
                    faces.push(f);
                }
            }
        }
        faces.sort();
        let mut max_gap: Option<f64> = Some(0.0);
        let mut upper = false;
        for &f in &faces {
            let surf = body.face(f).and_then(|x| Surf::of(x.surface()));
            let mut ok = surf.is_some();
            if let Some(s) = surf {
                for &q in &mesh_pts {
                    match surface_gap(s, q) {
                        Gap::Measured(d) => max_gap = max_gap.map(|m| larger(m, d)),
                        Gap::UpperBound(d) => {
                            upper = true;
                            max_gap = max_gap.map(|m| larger(m, d));
                        }
                        Gap::Unmeasured => ok = false,
                    }
                }
            }
            if !ok {
                max_gap = None;
                reports.push(EdgeReport::GapUnmeasured { edge: eix, face: f });
            }
        }
        if faces.is_empty() {
            max_gap = None;
        }

        let slot = slots.get_mut(e).ok_or(EdgeTessError::PcurvesMismatch)?;
        *slot = Some(EdgeSamples {
            edge: eix,
            t: ts,
            vertex_ix: vix,
            achieved_m,
            achieved_rad,
            max_surface_gap_m: max_gap,
            gap_upper_bound: upper,
        });
    }
    let edges: Vec<EdgeSamples> =
        slots.into_iter().collect::<Option<Vec<_>>>().ok_or(EdgeTessError::PcurvesMismatch)?;

    // Coedge uv in each face's chart.
    let mut coedges: Vec<CoedgeUv> = Vec::with_capacity(body.coedges().len());
    for (k, (c, trim)) in body.coedges().iter().zip(&pcurves.curves).enumerate() {
        let es = edges.get(c.edge().index()).ok_or(EdgeTessError::PcurvesMismatch)?;
        let reversed = c.sense() == Sense::Reversed;
        let (mut ts, mut vix) = (es.t.clone(), es.vertex_ix.clone());
        if reversed {
            ts.reverse();
            vix.reverse();
        }
        let l = body.r#loop(c.r#loop()).ok_or(EdgeTessError::PcurvesMismatch)?;
        let face = body.face(l.face()).ok_or(EdgeTessError::PcurvesMismatch)?;
        let pos = face.loops().iter().position(|&x| x == c.r#loop()).ok_or(EdgeTessError::PcurvesMismatch)?;
        let off = pcurves
            .charts
            .get(l.face().index())
            .and_then(|ch| ch.loop_offsets.get(pos))
            .copied()
            .ok_or(EdgeTessError::PcurvesMismatch)?;
        let sign = if reversed { -1.0 } else { 1.0 };
        let uv = coedge_uv(trim, &ts, sign, off);
        coedges.push(CoedgeUv { coedge: CoedgeIx::at(k), vertex_ix: vix, uv });
    }

    Ok(EdgeTess { edges, coedges, points, reports })
}
