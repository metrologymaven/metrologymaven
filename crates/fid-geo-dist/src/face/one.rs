//! Closest point on a single trimmed CAD face. Spec node `GEO.DIST.FACE.ONE`.
//!
//! [`FaceProjector::new`] prepares one face of a [`Body`] (its [`TrimIndex`], its boundary
//! edges and vertices, and for a NURBS face its Bézier patches); [`FaceProjector::project`]
//! returns the closest point of the trimmed face to a query, or [`FaceProjection::NoHit`]
//! beyond a maximum distance. [`project_face`] does both for one query. Only the exact surface,
//! the exact edge curves (or pcurves on the exact surface) and the trimming data are read,
//! never a tessellation (REQ-005).
//!
//! # Candidates (REQ-001), visited in this fixed order (REQ-006)
//!
//! 1. **Boundary edges** in ascending edge ID. An edge used twice by the face (a seam) and an
//!    edge whose image is a single point (a pole: no 3-D curve, both ends the same vertex,
//!    and its pcurve's surface image within [`DEFAULT_TOL_M`] of that vertex at five
//!    parameters) are not boundary and are skipped. The closest point on the edge over its
//!    parameter range: a line by projection, a circle by GEO.NURBS.ANALYTIC inversion shifted
//!    into the range (the distance along a circle has one minimum, so outside the range the
//!    minimum is an end), an ellipse arc, a NURBS curve or an edge given only by its pcurve
//!    (`S o c`) by the Lipschitz branch and bound of `curve` (module docs there). A foot at an
//!    end of an open edge is left to the vertex candidate; a closed edge (start vertex = end
//!    vertex) has no corner, and its feet are always the edge's.
//! 2. **Boundary vertices**: the ends of open boundary edges, in ascending vertex ID.
//! 3. **NURBS seams and poles** (points of the face, reported as Interior): on a NURBS
//!    face, where the surface may be only C0 across a seam or singular at a pole (so the
//!    closest point need not be a stationary point of any patch), every seam edge is a curve
//!    candidate over its whole range (ascending edge ID), and every pole is a point
//!    candidate: the vertex of a pole edge, and the image of a degenerate boundary line of
//!    the parameter domain (all its control points equal within `2^-40` of the largest
//!    coordinate) that a trimming segment ends on. On analytic faces the closed forms below
//!    already cover seams and poles.
//! 4. **Interior stationary points** that could still win (lower distance, or a tie the
//!    interior wins), kept when the face contains them (REQ-002, see *Trim test*). Analytic
//!    surfaces in closed form, in the surface's placement with `rho = hypot(x, y)`,
//!    `phi = atan2(y, x)`: plane 1 (`(x, y)`); cylinder 2 (`phi`, `phi + pi` at `v = z`);
//!    sphere 2 (the point on the ray through the query and its antipode); cone 2 plus the apex
//!    (the foot on each of the two generator lines in the query's meridian plane, both
//!    nappes); torus 4 (near and far point of the tube circle on each side of the axis).
//!    Where the stationary points form a circle or a sphere (query on the axis, at a sphere
//!    centre or a tube centre), 16 points of it (and 7 latitudes on a sphere) are taken: the
//!    distance is the same on the whole set, and if none of the samples lies in the face the
//!    set meets the boundary, where an edge foot is at least as close. NURBS surfaces: the
//!    branch and bound of `patch` (module docs there), pruned by the best boundary distance
//!    and the maximum distance.
//!
//! # Trim test
//!
//! [`classify_uv`] at `topo_tol_m`: Inside keeps the candidate, Outside drops it. In the
//! boundary band the side is decided on the exact boundary instead, so a candidate just
//! outside the trim is not taken for the face (the band is `1e-8 m` by default, ten times the
//! comparison tolerance of REQ-002): the nearest boundary point `c` of the candidate foot is
//! found on the exact edges and corners; nearest to a corner, with no edge, or within
//! `1e-12 m` of the edge the candidate is kept (the band rule); otherwise the parameters
//! `uv_c + s (uv - uv_c)` with `s |x - c| = 4, 16, 64 tol_m` are classified and the first
//! Inside or Outside answer decides (three band answers keep it). When the band search
//! exceeds its budget ([`TrimError::Budget`], next to a sphere pole, whose coedge is a single
//! point) and the candidate is farther than `2 tol_m` from every edge and corner, the
//! classification is repeated with half that distance as the band; only Outside drops it.
//!
//! # Choice
//!
//! The smallest distance; within [`FACE_TIE_WINDOW_M`] of each other, Interior before Edge
//! before Vertex, then the lower entity ID (edges and vertices are visited in ID order, so
//! the earlier candidate wins). The surface parameters of an edge or vertex foot are found
//! by inverting the foot onto the face surface (GEO.NURBS.ANALYTIC or GEO.NURBS.INVERT), or
//! from the pcurve for an edge given by its pcurve.
//!
//! # Determinism (D0, KNP-1)
//!
//! Binary64 `+ - * /` written one operation at a time (no FMA, no `f64::min`/`max`),
//! `sqrt`, `hypot`, `atan2`, `sin`, `cos`, `tan` only from `fid_num::f` (directly or inside
//! GEO.NURBS), vectors, sorted lists and binary heaps keyed by bit patterns and sequence
//! numbers, every loop bounded. Nothing panics.

mod curve;
mod patch;

use core::f64::consts::{FRAC_PI_2, PI, TAU};
use core::fmt;

use fid_geo_cad::{
    classify_uv, derive_pcurves, Body, Curve2, EdgeIx, FaceIx, PcurveError, Pcurves, TopologyId, TrimClass, TrimError,
    TrimIndex, VertexIx, DEFAULT_TOL_M,
};
use fid_geo_nurbs::{
    invert_surface, AnalyticCurve, AnalyticSurface, Circle, Curve, InvertError, Line, NurbsSurface, Surface,
};

use crate::FACE_TIE_WINDOW_M;

pub use patch::{BUDGET as PATCH_BUDGET, MAX_DEPTH as PATCH_MAX_DEPTH};

/// Inflation of every Lipschitz bound (`1 + 2^-20`).
const LIPSCHITZ_INFLATE: f64 = 1.0 + 1.0 / 1_048_576.0;
/// Relative on-axis test of the analytic closed forms (GEO.NURBS.ANALYTIC `AXIS_REL`).
const AXIS_REL: f64 = 1e-14;
/// An interior candidate in the trim band this close to the exact boundary is kept (metres).
const SIDE_FLOOR_M: f64 = 1e-12;
/// Samples of a circle of stationary points.
const RING: usize = 16;

// ---------------------------------------------------------------- public types

/// Query options (Design: `FaceQueryOptions { max_distance, topo_tol_m }`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FaceQueryOptions {
    /// REQ-004: no hit when the closest point is farther than this (metres).
    pub max_distance: Option<f64>,
    /// The boundary band of the trim classification (metres).
    pub topo_tol_m: f64,
}

impl Default for FaceQueryOptions {
    /// No maximum distance; the GEO.CAD.TOPO default tolerance (1e-8 m).
    fn default() -> Self {
        FaceQueryOptions { max_distance: None, topo_tol_m: DEFAULT_TOL_M }
    }
}

/// Where on the face the foot lies.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FaceFeature {
    /// A stationary point of the distance inside the trimmed region (or within the topology
    /// tolerance of its boundary).
    Interior,
    /// A point of this boundary edge, not at a corner.
    Edge(TopologyId),
    /// This boundary vertex (a corner).
    Vertex(TopologyId),
}

/// The closest point of a trimmed face.
#[derive(Debug, Clone, PartialEq)]
pub struct FaceHit {
    /// The closest point, on the exact surface.
    pub foot: [f64; 3],
    /// `|p - foot|`, metres.
    pub distance: f64,
    /// The face's topology ID.
    pub face: TopologyId,
    /// Surface parameters of the foot.
    pub uv: [f64; 2],
    pub feature: FaceFeature,
}

/// Result of one query.
#[derive(Debug, Clone, PartialEq)]
pub enum FaceProjection {
    Hit(FaceHit),
    /// REQ-004: nothing within the maximum distance.
    NoHit,
}

/// Why a face cannot be projected onto.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unsupported {
    /// A NURBS face surface with an interior knot of multiplicity at least the degree (a
    /// possible crease, where the closest point need not be a stationary point).
    Crease,
    /// The knot vector could not be decomposed into Bézier patches.
    KnotLayout,
    /// The face surface is neither analytic nor NURBS.
    Surface,
}

/// Errors of this leaf.
#[derive(Debug, Clone, PartialEq)]
pub enum FaceError {
    /// The query has a NaN or infinite coordinate.
    NonFiniteQuery,
    /// `topo_tol_m` is not finite and positive, or `max_distance` is NaN or negative.
    BadOptions,
    /// The face index is not in the body.
    UnknownFace,
    /// [`project_face`]: the pcurves could not be derived.
    Pcurve(PcurveError),
    /// The trim index could not be built or a classification failed.
    Trim(TrimError),
    /// A boundary edge has neither a 3-D curve nor a pcurve on this face.
    NoEdgeGeometry { edge: TopologyId },
    /// The face is outside what this leaf handles.
    Unsupported(Unsupported),
    /// An evaluation failed or overflowed.
    NonFinite,
    /// The NURBS search could not resolve every part of the face that might hold a closer
    /// point (a continuum of closest points, or the node budget); `best` is the best found.
    NonConvergence { best: Option<FaceHit> },
}

impl FaceError {
    /// Stable variant name.
    pub fn code(&self) -> &'static str {
        match self {
            FaceError::NonFiniteQuery => "NonFiniteQuery",
            FaceError::BadOptions => "BadOptions",
            FaceError::UnknownFace => "UnknownFace",
            FaceError::Pcurve(_) => "Pcurve",
            FaceError::Trim(_) => "Trim",
            FaceError::NoEdgeGeometry { .. } => "NoEdgeGeometry",
            FaceError::Unsupported(_) => "Unsupported",
            FaceError::NonFinite => "NonFinite",
            FaceError::NonConvergence { .. } => "NonConvergence",
        }
    }
}

impl fmt::Display for FaceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FaceError::NonFiniteQuery => f.write_str("query point is not finite"),
            FaceError::BadOptions => f.write_str("bad options: topology tolerance or maximum distance"),
            FaceError::UnknownFace => f.write_str("face is not in the body"),
            FaceError::Pcurve(e) => write!(f, "pcurves: {e}"),
            FaceError::Trim(e) => write!(f, "trim: {e}"),
            FaceError::NoEdgeGeometry { edge } => write!(f, "edge {edge} has no curve and no pcurve"),
            FaceError::Unsupported(u) => write!(f, "unsupported face: {u:?}"),
            FaceError::NonFinite => f.write_str("evaluation failed or overflowed"),
            FaceError::NonConvergence { .. } => f.write_str("the face search did not converge"),
        }
    }
}

impl core::error::Error for FaceError {}

impl From<TrimError> for FaceError {
    fn from(e: TrimError) -> Self {
        FaceError::Trim(e)
    }
}

impl From<Unsupported> for FaceError {
    fn from(e: Unsupported) -> Self {
        FaceError::Unsupported(e)
    }
}

// ---------------------------------------------------------------- small helpers

fn sub3(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn dot3(a: [f64; 3], b: [f64; 3]) -> f64 {
    (a[0] * b[0] + a[1] * b[1]) + a[2] * b[2]
}

fn hyp(a: f64, b: f64) -> Result<f64, FaceError> {
    fid_num::f::hypot(a, b).map_err(|_| FaceError::NonFinite)
}

/// `hypot(hypot(a0, a1), a2)`.
fn norm3(a: [f64; 3]) -> Result<f64, FaceError> {
    let d = hyp(hyp(a[0], a[1])?, a[2])?;
    if d.is_finite() {
        Ok(d)
    } else {
        Err(FaceError::NonFinite)
    }
}

/// `b` if `b > a`, else `a` (no `f64::max`).
fn fmax(a: f64, b: f64) -> f64 {
    if b > a {
        b
    } else {
        a
    }
}

/// A key whose unsigned order is the numeric order of finite `x`.
fn ord_key(x: f64) -> u64 {
    let b = x.to_bits();
    if b >> 63 == 1 {
        !b
    } else {
        b | (1 << 63)
    }
}

fn num(r: Result<f64, fid_num::NumError>) -> Result<f64, FaceError> {
    r.map_err(|_| FaceError::NonFinite)
}

/// `x` moved into `[0, 2 pi)` by at most one period.
fn wrap(x: f64) -> f64 {
    if x < 0.0 {
        x + TAU
    } else if x >= TAU {
        x - TAU
    } else {
        x
    }
}

// ---------------------------------------------------------------- prepared face

#[derive(Debug, Clone, Copy)]
enum SurfRef<'a> {
    Analytic(&'a AnalyticSurface),
    Nurbs(&'a NurbsSurface),
}

impl<'a> SurfRef<'a> {
    fn dyn_surface(&self) -> &'a dyn Surface {
        match *self {
            SurfRef::Analytic(s) => s,
            SurfRef::Nurbs(s) => s,
        }
    }
}

#[derive(Clone, Copy)]
enum Geom<'a> {
    Line(&'a Line),
    Circle(&'a Circle),
    Curve(&'a dyn Curve),
    OnSurface(&'a Curve2),
}

#[derive(Clone)]
struct BEdge<'a> {
    id: TopologyId,
    geom: Geom<'a>,
    range: [f64; 2],
    closed: bool,
    lip: f64,
}

/// The closest point on an edge: foot and (for an edge given by its pcurve) the surface
/// parameters.
#[derive(Debug, Clone, Copy)]
struct EdgeFoot {
    foot: [f64; 3],
    uv: Option<[f64; 2]>,
}

/// One face of a body, prepared for repeated queries.
pub struct FaceProjector<'a> {
    face_id: TopologyId,
    trim: TrimIndex,
    surf: SurfRef<'a>,
    patches: Option<patch::Patches>,
    edges: Vec<BEdge<'a>>,
    vertices: Vec<(TopologyId, [f64; 3])>,
    /// NURBS face: seam edges (points of the face, not boundary), ascending edge ID.
    seams: Vec<BEdge<'a>>,
    /// NURBS face: pole points of the face with their parameters.
    poles: Vec<Pole>,
}

impl fmt::Debug for FaceProjector<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FaceProjector")
            .field("face", &self.face_id)
            .field("edges", &self.edges.iter().map(|e| &e.id).collect::<Vec<_>>())
            .field("vertices", &self.vertices.iter().map(|v| &v.0).collect::<Vec<_>>())
            .field("patches", &self.patches.as_ref().map(patch::Patches::len))
            .finish()
    }
}

/// Lipschitz bound `2 p (w_max / w_min) diam / dmin` of a NURBS curve or of one direction of
/// a NURBS surface (`diam` the diagonal of the control points' box, `dmin` the least positive
/// `knot[i + p] - knot[i]`).
fn nurbs_lip(degree: usize, knots: &[f64], ctrl: &[[f64; 3]], weights: Option<&[f64]>) -> Result<f64, FaceError> {
    let mut lo = [f64::INFINITY; 3];
    let mut hi = [f64::NEG_INFINITY; 3];
    for c in ctrl {
        for k in 0..3 {
            if c[k] < lo[k] {
                lo[k] = c[k];
            }
            if c[k] > hi[k] {
                hi[k] = c[k];
            }
        }
    }
    let diam = norm3(sub3(hi, lo))?;
    let mut dmin = f64::INFINITY;
    for i in 0..knots.len().saturating_sub(degree) {
        let (Some(&a), Some(&b)) = (knots.get(i), knots.get(i + degree)) else { continue };
        let d = b - a;
        if d > 0.0 && d < dmin {
            dmin = d;
        }
    }
    let (mut wmin, mut wmax) = (1.0, 1.0);
    if let Some(ws) = weights {
        wmin = f64::INFINITY;
        wmax = 0.0;
        for &w in ws {
            if w < wmin {
                wmin = w;
            }
            wmax = fmax(wmax, w);
        }
    }
    let l = ((2.0 * degree as f64) * (wmax / wmin)) * (diam / dmin);
    if l.is_finite() {
        Ok(l * LIPSCHITZ_INFLATE)
    } else {
        Err(FaceError::NonFinite)
    }
}

/// `(Lu, Lv)` with `|S_u| <= Lu`, `|S_v| <= Lv` where `v` lies in `[vlo, vhi]`.
fn surface_lip(s: SurfRef<'_>, vlo: f64, vhi: f64) -> Result<(f64, f64), FaceError> {
    Ok(match s {
        SurfRef::Analytic(a) => match a {
            AnalyticSurface::Plane(_) => (1.0, 1.0),
            AnalyticSurface::Cylinder(c) => (c.radius(), 1.0),
            AnalyticSurface::Cone(c) => {
                let t = num(fid_num::f::tan(c.half_angle()))?;
                let r0 = c.radius();
                let lu = fmax((r0 + vlo * t).abs(), (r0 + vhi * t).abs());
                (lu, hyp(1.0, t)?)
            }
            AnalyticSurface::Sphere(sp) => (sp.radius(), sp.radius()),
            AnalyticSurface::Torus(t) => {
                let (big, small) = t.radii();
                (big + small, small)
            }
        },
        SurfRef::Nurbs(n) => {
            let w = n.weights();
            (
                nurbs_lip(n.degree_u(), n.knots_u().as_slice(), n.ctrl(), w)?,
                nurbs_lip(n.degree_v(), n.knots_v().as_slice(), n.ctrl(), w)?,
            )
        }
    })
}

/// Pcurve position and derivatives at `t`.
fn pcurve_eval(c: &Curve2, t: f64) -> Result<[[f64; 2]; 3], FaceError> {
    if let Some((o, d)) = c.as_line() {
        return Ok([[o[0] + t * d[0], o[1] + t * d[1]], d, [0.0, 0.0]]);
    }
    let n = c.as_nurbs().ok_or(FaceError::NonFinite)?;
    let e = n.eval(t, 2).map_err(|_| FaceError::NonFinite)?;
    let (Some(d1), Some(d2)) = (e.d1, e.d2) else { return Err(FaceError::NonFinite) };
    Ok([[e.p[0], e.p[1]], [d1[0], d1[1]], [d2[0], d2[1]]])
}

/// `S o c` and its first two derivatives at `t`.
fn on_surface_eval(s: &dyn Surface, c: &Curve2, t: f64) -> Result<curve::Eval3, FaceError> {
    let [uv, d, dd] = pcurve_eval(c, t)?;
    let e = s.eval(uv[0], uv[1], 2).map_err(|_| FaceError::NonFinite)?;
    let (Some(su), Some(sv), Some(suu), Some(suv), Some(svv)) = (e.su, e.sv, e.suu, e.suv, e.svv) else {
        return Err(FaceError::NonFinite);
    };
    let c1 = core::array::from_fn(|k| d[0] * su[k] + d[1] * sv[k]);
    let c2 = core::array::from_fn(|k| {
        let a = (d[0] * d[0]) * suu[k] + (2.0 * (d[0] * d[1])) * suv[k];
        let b = (d[1] * d[1]) * svv[k];
        ((a + b) + dd[0] * su[k]) + dd[1] * sv[k]
    });
    Ok([e.p, c1, c2])
}

/// Lipschitz bound of `S o c` over `[t0, t1]`.
fn on_surface_lip(s: SurfRef<'_>, c: &Curve2, t0: f64, t1: f64) -> Result<f64, FaceError> {
    let (cu, cv, vlo, vhi) = if let Some((o, d)) = c.as_line() {
        let va = o[1] + t0 * d[1];
        let vb = o[1] + t1 * d[1];
        let (lo, hi) = if va < vb { (va, vb) } else { (vb, va) };
        (d[0].abs(), d[1].abs(), lo, hi)
    } else {
        let n = c.as_nurbs().ok_or(FaceError::NonFinite)?;
        let l = nurbs_lip(n.degree(), n.knots().as_slice(), n.ctrl(), n.weights())?;
        let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
        for p in n.ctrl() {
            if p[1] < lo {
                lo = p[1];
            }
            if p[1] > hi {
                hi = p[1];
            }
        }
        (l, l, lo, hi)
    };
    let (lu, lv) = surface_lip(s, vlo, vhi)?;
    let l = lu * cu + lv * cv;
    if l.is_finite() {
        Ok(l * LIPSCHITZ_INFLATE)
    } else {
        Err(FaceError::NonFinite)
    }
}

/// A pole point of a NURBS face and its surface parameters.
type Pole = ([f64; 3], [f64; 2]);

/// A boundary line of a NURBS domain: direction of the fixed parameter, its value, the range
/// of the other parameter and the control points on the line.
type DomainLine = (usize, f64, [f64; 2], Vec<Option<[f64; 3]>>);

/// Pole points of a NURBS face: a boundary line of the parameter domain whose control
/// points all coincide (within `2^-40` of the largest coordinate) is a single point of the
/// surface; it belongs to the face when a trimming segment ends on that line (within
/// `2^-40` of the domain width). Returned with its parameters (the line's midpoint).
fn pole_lines(n: &NurbsSurface, trim: &TrimIndex) -> Result<Vec<Pole>, FaceError> {
    const REL: f64 = 1.0 / 1_099_511_627_776.0;
    let (n_u, n_v) = n.shape();
    let ((u0, u1), (v0, v1)) = n.domain();
    let ctrl = n.ctrl();
    let mut scale = 0.0;
    for c in ctrl {
        for x in c {
            scale = fmax(scale, x.abs());
        }
    }
    let at = |i: usize, j: usize| -> Option<[f64; 3]> { ctrl.get(i * n_v + j).copied() };
    let mut out = Vec::new();
    // (direction of the fixed parameter, its value, the other range, control points)
    let lines: [DomainLine; 4] = [
        (0, u0, [v0, v1], (0..n_v).map(|j| at(0, j)).collect()),
        (0, u1, [v0, v1], (0..n_v).map(|j| at(n_u.saturating_sub(1), j)).collect()),
        (1, v0, [u0, u1], (0..n_u).map(|i| at(i, 0)).collect()),
        (1, v1, [u0, u1], (0..n_u).map(|i| at(i, n_v.saturating_sub(1))).collect()),
    ];
    for (dir, value, other, pts) in lines {
        let Some(Some(first)) = pts.first().copied() else { continue };
        let mut degenerate = true;
        for pt in &pts {
            let Some(pt) = pt else { return Err(FaceError::NonFinite) };
            for (a, b) in pt.iter().zip(first.iter()) {
                if (a - b).abs() > REL * scale {
                    degenerate = false;
                }
            }
        }
        if !degenerate {
            continue;
        }
        let width = if dir == 0 { u1 - u0 } else { v1 - v0 };
        let touched = trim
            .segments()
            .iter()
            .any(|g| (g.a[dir] - value).abs() <= REL * width || (g.b[dir] - value).abs() <= REL * width);
        if touched {
            let m = other[0] + (other[1] - other[0]) * 0.5;
            let uv = if dir == 0 { [value, m] } else { [m, value] };
            let pt = n.eval(uv[0], uv[1], 0).map_err(|_| FaceError::NonFinite)?.p;
            out.push((pt, uv));
        }
    }
    Ok(out)
}

impl<'a> FaceProjector<'a> {
    /// Prepares `face` of `body`, whose pcurves [`derive_pcurves`] gave as `pcurves`.
    pub fn new(body: &'a Body, pcurves: &Pcurves, face: FaceIx) -> Result<FaceProjector<'a>, FaceError> {
        let f = body.face(face).ok_or(FaceError::UnknownFace)?;
        let trim = TrimIndex::build(body, pcurves, face)?;
        let surf = match (f.surface().as_analytic(), f.surface().as_nurbs()) {
            (Some(a), _) => SurfRef::Analytic(a),
            (None, Some(n)) => SurfRef::Nurbs(n),
            (None, None) => return Err(Unsupported::Surface.into()),
        };
        let patches = match surf {
            SurfRef::Nurbs(n) => Some(patch::Patches::new(n)?),
            SurfRef::Analytic(_) => None,
        };
        // Uses of each edge by this face, with the first coedge's pcurve.
        let mut uses: Vec<(EdgeIx, usize, Option<&'a Curve2>)> = Vec::new();
        for &lx in f.loops() {
            let lp = body.r#loop(lx).ok_or(FaceError::UnknownFace)?;
            for &cx in lp.coedges() {
                let co = body.coedge(cx).ok_or(FaceError::UnknownFace)?;
                match uses.iter_mut().find(|u| u.0 == co.edge()) {
                    Some(u) => u.1 += 1,
                    None => uses.push((co.edge(), 1, co.pcurve())),
                }
            }
        }
        let s_dyn = surf.dyn_surface();
        let mut edges: Vec<BEdge<'a>> = Vec::new();
        let mut corner_ix: Vec<VertexIx> = Vec::new();
        let nurbs = matches!(surf, SurfRef::Nurbs(_));
        let mut seams: Vec<BEdge<'a>> = Vec::new();
        let mut poles: Vec<Pole> = Vec::new();
        for &(ex, count, pc) in &uses {
            let e = body.edge(ex).ok_or(FaceError::UnknownFace)?;
            let range = e.param_range();
            let closed = e.start() == e.end();
            let id = e.id().clone();
            let (geom, lip) = match e.curve() {
                Some(cd) => match (cd.as_analytic(), cd.as_nurbs()) {
                    (Some(AnalyticCurve::Line(l)), _) => (Geom::Line(l), 1.0),
                    (Some(AnalyticCurve::Circle(c)), _) => (Geom::Circle(c), c.radius()),
                    (Some(AnalyticCurve::Ellipse(el)), _) => {
                        let (a, b) = el.semi_axes();
                        (Geom::Curve(el as &dyn Curve), fmax(a, b) * LIPSCHITZ_INFLATE)
                    }
                    (None, Some(n)) => {
                        let l = nurbs_lip(n.degree(), n.knots().as_slice(), n.ctrl(), n.weights())?;
                        (Geom::Curve(n as &dyn Curve), l)
                    }
                    (None, None) => return Err(FaceError::NoEdgeGeometry { edge: id }),
                },
                None => {
                    let pc = pc.ok_or_else(|| FaceError::NoEdgeGeometry { edge: id.clone() })?;
                    if closed && count < 2 {
                        let vp = body.vertex(e.start()).ok_or(FaceError::UnknownFace)?.point();
                        let mut pole = true;
                        for k in 0..5 {
                            let t = range[0] + ((range[1] - range[0]) * k as f64) / 4.0;
                            let c = on_surface_eval(s_dyn, pc, t)?[0];
                            if norm3(sub3(c, vp))? > DEFAULT_TOL_M {
                                pole = false;
                            }
                        }
                        if pole {
                            // A pole: a single point, not boundary; on a NURBS face a point
                            // candidate (where the surface is not smooth).
                            if nurbs {
                                poles.push((vp, pcurve_eval(pc, range[0])?[0]));
                            }
                            continue;
                        }
                    }
                    (Geom::OnSurface(pc), on_surface_lip(surf, pc, range[0], range[1])?)
                }
            };
            if count >= 2 {
                // A seam: crossed, not boundary; on a NURBS face (which may be only C0 there)
                // a curve candidate over its whole range.
                if nurbs {
                    seams.push(BEdge { id, geom, range, closed: true, lip });
                }
                continue;
            }
            if !closed {
                for v in [e.start(), e.end()] {
                    if !corner_ix.contains(&v) {
                        corner_ix.push(v);
                    }
                }
            }
            edges.push(BEdge { id, geom, range, closed, lip });
        }
        seams.sort_by(|a, b| a.id.cmp(&b.id));
        if let SurfRef::Nurbs(n) = surf {
            for (pt, uv) in pole_lines(n, &trim)? {
                let mut dup = false;
                for (o, _) in &poles {
                    if norm3(sub3(*o, pt))? <= DEFAULT_TOL_M {
                        dup = true;
                    }
                }
                if !dup {
                    poles.push((pt, uv));
                }
            }
        }
        edges.sort_by(|a, b| a.id.cmp(&b.id));
        let mut vertices: Vec<(TopologyId, [f64; 3])> = Vec::with_capacity(corner_ix.len());
        for vx in corner_ix {
            let v = body.vertex(vx).ok_or(FaceError::UnknownFace)?;
            vertices.push((v.id().clone(), v.point()));
        }
        vertices.sort_by(|a, b| a.0.cmp(&b.0));
        Ok(FaceProjector { face_id: f.id().clone(), trim, surf, patches, edges, vertices, seams, poles })
    }

    /// The face's topology ID.
    pub fn face_id(&self) -> &TopologyId {
        &self.face_id
    }

    /// IDs of the boundary edges, ascending.
    pub fn boundary_edges(&self) -> Vec<&TopologyId> {
        self.edges.iter().map(|e| &e.id).collect()
    }

    /// IDs of the boundary vertices (corners), ascending.
    pub fn boundary_vertices(&self) -> Vec<&TopologyId> {
        self.vertices.iter().map(|v| &v.0).collect()
    }

    /// Number of Bézier patches of a NURBS face (`None` for an analytic face).
    pub fn patch_count(&self) -> Option<usize> {
        self.patches.as_ref().map(patch::Patches::len)
    }

    /// Trim test of an interior candidate at `uv` with foot `x`: Inside is kept, Outside
    /// dropped, OnBoundary decided by [`Self::band_side`].
    ///
    /// When the band search exceeds its budget ([`TrimError::Budget`], e.g. next to a sphere
    /// pole, whose coedge is part of the trim boundary of record but is a single point) and
    /// the candidate is farther than `2 tol` from every boundary edge and corner, the
    /// classification is repeated with the band half that distance: Inside or OnBoundary
    /// (the band then meets only a boundary that is not an edge, a pole) keep it.
    fn inside(&self, uv: [f64; 2], x: [f64; 3], tol: f64) -> Result<bool, FaceError> {
        match classify_uv(&self.trim, uv[0], uv[1], tol) {
            Ok(TrimClass::Inside) => Ok(true),
            Ok(TrimClass::Outside) => Ok(false),
            Ok(TrimClass::OnBoundary) => self.band_side(uv, x, tol),
            Err(TrimError::Budget) => {
                let d = self.nearest_boundary(x)?.map_or(f64::INFINITY, |n| n.0);
                if d.is_nan() || d <= 2.0 * tol {
                    return Err(TrimError::Budget.into());
                }
                let wide = if d.is_finite() { d * 0.5 } else { 1.0 };
                Ok(!matches!(classify_uv(&self.trim, uv[0], uv[1], wide)?, TrimClass::Outside))
            }
            Err(e) => Err(e.into()),
        }
    }

    /// The nearest boundary point of `x`: distance, and the edge foot unless a corner is at
    /// least as near (`None` without edges and corners).
    fn nearest_boundary(&self, x: [f64; 3]) -> Result<Option<(f64, Option<EdgeFoot>)>, FaceError> {
        let mut near: Option<(f64, Option<EdgeFoot>)> = None;
        for e in &self.edges {
            if let Some(f) = self.edge_foot(e, x)? {
                let d = norm3(sub3(f.foot, x))?;
                if near.as_ref().map_or(true, |n| d < n.0) {
                    near = Some((d, Some(f)));
                }
            }
        }
        for (_, pt) in &self.vertices {
            let d = norm3(sub3(*pt, x))?;
            if near.as_ref().map_or(true, |n| d <= n.0) {
                near = Some((d, None));
            }
        }
        Ok(near)
    }

    /// Closest point on a boundary edge; `None` at an end of an open edge.
    fn edge_foot(&self, e: &BEdge<'a>, q: [f64; 3]) -> Result<Option<EdgeFoot>, FaceError> {
        let [t0, t1] = e.range;
        let at_end = |t: f64| !e.closed && (t <= t0 || t >= t1);
        let (foot, uv) = match e.geom {
            Geom::Line(l) => {
                let t = l.placement().to_local(q).map_err(|_| FaceError::NonFinite)?[0];
                if at_end(t) {
                    return Ok(None);
                }
                (l.eval(t, 0).map_err(|_| FaceError::NonFinite)?.p, None)
            }
            Geom::Circle(c) => {
                let cf = c.invert(q).map_err(|_| FaceError::NonFinite)?;
                // Shift into [t0, t1] by whole periods.
                let n = -((cf.t - t0) / TAU).floor();
                let mut t = cf.t + n * TAU;
                if t < t0 {
                    t += TAU;
                }
                if t > t1 {
                    if e.closed {
                        t = t1;
                    } else {
                        return Ok(None);
                    }
                }
                if at_end(t) {
                    return Ok(None);
                }
                (cf.foot, None)
            }
            Geom::Curve(c) => {
                let ev = |t: f64| -> Result<curve::Eval3, FaceError> {
                    let d = c.eval(t, 2).map_err(|_| FaceError::NonFinite)?;
                    let (Some(d1), Some(d2)) = (d.d1, d.d2) else { return Err(FaceError::NonFinite) };
                    Ok([d.p, d1, d2])
                };
                let cf = curve::closest(&ev, e.lip, t0, t1, q)?;
                if at_end(cf.t) {
                    return Ok(None);
                }
                (cf.foot, None)
            }
            Geom::OnSurface(pc) => {
                let s = self.surf.dyn_surface();
                let ev = |t: f64| on_surface_eval(s, pc, t);
                let cf = curve::closest(&ev, e.lip, t0, t1, q)?;
                if at_end(cf.t) {
                    return Ok(None);
                }
                let uv = pcurve_eval(pc, cf.t)?[0];
                (cf.foot, Some(uv))
            }
        };
        Ok(Some(EdgeFoot { foot, uv }))
    }

    /// Whether an interior candidate at `uv` (foot `x`) that the trim classification puts in
    /// the boundary band lies inside the face (REQ-002 refinement, module docs). The nearest
    /// boundary point `c` of `x` is found on the exact edges and corners. Nearest to a corner,
    /// with no edge, or within [`SIDE_FLOOR_M`] of the edge, the candidate is kept (band rule).
    /// Otherwise the parameter `uv_c + s (uv - uv_c)` is classified for `s` with
    /// `s |x - c| = 4 tol, 16 tol, 64 tol` (periodic differences reduced to half a period):
    /// the first Inside or Outside answer decides; three band answers keep the candidate.
    fn band_side(&self, uv: [f64; 2], x: [f64; 3], tol: f64) -> Result<bool, FaceError> {
        let Some((d, Some(f))) = self.nearest_boundary(x)? else { return Ok(true) };
        if d <= SIDE_FLOOR_M {
            return Ok(true);
        }
        let uc = match f.uv {
            Some(uv) => uv,
            None => self.uv_of(f.foot)?,
        };
        let periods = self.periods();
        let w: [f64; 2] = core::array::from_fn(|i| {
            let mut dw = uv[i] - uc[i];
            if let Some(pd) = periods[i] {
                let k = (dw / pd + 0.5).floor();
                dw -= k * pd;
            }
            dw
        });
        let mut reach = 4.0 * tol;
        for _ in 0..3 {
            let sc = reach / d;
            let y = [uc[0] + sc * w[0], uc[1] + sc * w[1]];
            match classify_uv(&self.trim, y[0], y[1], tol) {
                Ok(TrimClass::Inside) => return Ok(true),
                Ok(TrimClass::Outside) => return Ok(false),
                Ok(TrimClass::OnBoundary) | Err(TrimError::OutsideDomain { .. }) => {}
                Err(e) => return Err(e.into()),
            }
            reach *= 4.0;
        }
        Ok(true)
    }

    /// Periods of the surface parameters (`None` where not periodic).
    fn periods(&self) -> [Option<f64>; 2] {
        match self.surf {
            SurfRef::Analytic(a) => match a {
                AnalyticSurface::Plane(_) => [None, None],
                AnalyticSurface::Torus(_) => [Some(TAU), Some(TAU)],
                _ => [Some(TAU), None],
            },
            SurfRef::Nurbs(n) => {
                let ((u0, u1), (v0, v1)) = n.domain();
                [n.periodic_u().then_some(u1 - u0), n.periodic_v().then_some(v1 - v0)]
            }
        }
    }

    /// Surface parameters of a point on the face surface.
    fn uv_of(&self, foot: [f64; 3]) -> Result<[f64; 2], FaceError> {
        match self.surf {
            SurfRef::Analytic(s) => {
                let f = s.invert(foot).map_err(|_| FaceError::NonFinite)?;
                Ok([f.u, f.v])
            }
            SurfRef::Nurbs(s) => match invert_surface(s, foot) {
                Ok(i) => Ok([i.u, i.v]),
                Err(InvertError::NonConvergence { best }) => Ok([best.u, best.v]),
                Err(_) => Err(FaceError::NonFinite),
            },
        }
    }

    /// Stationary points of the distance on an analytic surface (module docs).
    fn analytic_points(s: &AnalyticSurface, q: [f64; 3]) -> Result<Vec<[f64; 2]>, FaceError> {
        let local = |pl: &fid_geo_nurbs::Placement| pl.to_local(q).map_err(|_| FaceError::NonFinite);
        let ring = |v: f64, out: &mut Vec<[f64; 2]>| {
            for k in 0..RING {
                out.push([(TAU * k as f64) / RING as f64, v]);
            }
        };
        let atan2 = |y: f64, x: f64| num(fid_num::f::atan2(y, x));
        let mut out = Vec::new();
        match s {
            AnalyticSurface::Plane(p) => {
                let l = local(p.placement())?;
                out.push([l[0], l[1]]);
            }
            AnalyticSurface::Cylinder(c) => {
                let [x, y, z] = local(c.placement())?;
                let rho = hyp(x, y)?;
                let dn = hyp(rho, z)?;
                if rho > AXIS_REL * dn {
                    let phi = wrap(atan2(y, x)?);
                    out.push([phi, z]);
                    out.push([wrap(phi + PI), z]);
                } else {
                    ring(z, &mut out);
                }
            }
            AnalyticSurface::Sphere(sp) => {
                let [x, y, z] = local(sp.placement())?;
                let rho = hyp(x, y)?;
                let dn = hyp(rho, z)?;
                if dn == 0.0 {
                    for j in -3i32..=3 {
                        ring((FRAC_PI_2 * j as f64) / 4.0, &mut out);
                    }
                    out.push([0.0, FRAC_PI_2]);
                    out.push([0.0, -FRAC_PI_2]);
                } else if rho > AXIS_REL * dn {
                    let phi = wrap(atan2(y, x)?);
                    out.push([phi, atan2(z, rho)?]);
                    out.push([wrap(phi + PI), atan2(-z, rho)?]);
                } else {
                    out.push([0.0, FRAC_PI_2]);
                    out.push([0.0, -FRAC_PI_2]);
                }
            }
            AnalyticSurface::Cone(c) => {
                let [x, y, z] = local(c.placement())?;
                let t = num(fid_num::f::tan(c.half_angle()))?;
                let r0 = c.radius();
                let tt = 1.0 + t * t;
                let rho = hyp(x, y)?;
                let dn = hyp(rho, z)?;
                if rho > AXIS_REL * dn {
                    let phi = wrap(atan2(y, x)?);
                    out.push([phi, (z + t * (rho - r0)) / tt]);
                    out.push([wrap(phi + PI), (z + t * (-rho - r0)) / tt]);
                } else {
                    ring((z - t * r0) / tt, &mut out);
                }
                out.push([0.0, -r0 / t]);
            }
            AnalyticSurface::Torus(tr) => {
                let [x, y, z] = local(tr.placement())?;
                let (big, _) = tr.radii();
                let rho = hyp(x, y)?;
                let dn = hyp(rho, z)?;
                let tube = |a: f64, u: Option<f64>, out: &mut Vec<[f64; 2]>| -> Result<(), FaceError> {
                    let w = hyp(a - big, z)?;
                    let vs: Vec<f64> = if w > AXIS_REL * (big + dn) {
                        let vn = wrap(atan2(z, a - big)?);
                        vec![vn, wrap(vn + PI)]
                    } else {
                        (0..RING).map(|k| (TAU * k as f64) / RING as f64).collect()
                    };
                    for v in vs {
                        match u {
                            Some(u) => out.push([u, v]),
                            None => ring(v, out),
                        }
                    }
                    Ok(())
                };
                if rho > AXIS_REL * dn {
                    let phi = wrap(atan2(y, x)?);
                    tube(rho, Some(phi), &mut out)?;
                    tube(-rho, Some(wrap(phi + PI)), &mut out)?;
                } else {
                    tube(0.0, None, &mut out)?;
                }
            }
        }
        Ok(out)
    }

    /// The closest point of the trimmed face to `p` (REQ-001 to REQ-006).
    pub fn project(&self, p: [f64; 3], opts: FaceQueryOptions) -> Result<FaceProjection, FaceError> {
        if !p.iter().all(|x| x.is_finite()) {
            return Err(FaceError::NonFiniteQuery);
        }
        let tol = opts.topo_tol_m;
        if !(tol.is_finite() && tol > 0.0) {
            return Err(FaceError::BadOptions);
        }
        if let Some(m) = opts.max_distance {
            if m.is_nan() || m < 0.0 {
                return Err(FaceError::BadOptions);
            }
        }
        let mut best: Option<Cand> = None;
        let offer = |best: &mut Option<Cand>, c: Cand| {
            let take = match best {
                None => true,
                Some(b) => c.better_than(b),
            };
            if take {
                *best = Some(c);
            }
        };
        for (k, e) in self.edges.iter().enumerate() {
            if let Some(f) = self.edge_foot(e, p)? {
                let c = Cand { distance: norm3(sub3(f.foot, p))?, foot: f.foot, uv: f.uv, feat: Feat::Edge(k) };
                offer(&mut best, c);
            }
        }
        for (k, (_, pt)) in self.vertices.iter().enumerate() {
            let c = Cand { distance: norm3(sub3(*pt, p))?, foot: *pt, uv: None, feat: Feat::Vertex(k) };
            offer(&mut best, c);
        }
        // NURBS seams and poles: points of the face (Interior), not boundary.
        for e in &self.seams {
            if let Some(f) = self.edge_foot(e, p)? {
                let c = Cand { distance: norm3(sub3(f.foot, p))?, foot: f.foot, uv: f.uv, feat: Feat::Interior };
                offer(&mut best, c);
            }
        }
        for (pt, uv) in &self.poles {
            let c = Cand { distance: norm3(sub3(*pt, p))?, foot: *pt, uv: Some(*uv), feat: Feat::Interior };
            offer(&mut best, c);
        }
        let mut unresolved = false;
        match self.surf {
            SurfRef::Analytic(s) => {
                for uv in Self::analytic_points(s, p)? {
                    let foot = s.eval(uv[0], uv[1], 0).map_err(|_| FaceError::NonFinite)?.p;
                    let c = Cand { distance: norm3(sub3(foot, p))?, foot, uv: Some(uv), feat: Feat::Interior };
                    // Only a point that could win is classified.
                    let could_win = best.as_ref().map_or(true, |b| c.better_than(b));
                    if could_win && self.inside(uv, foot, tol)? {
                        offer(&mut best, c);
                    }
                }
            }
            SurfRef::Nurbs(s) => {
                let mut bound = best.as_ref().map_or(f64::INFINITY, |b| b.distance);
                if let Some(m) = opts.max_distance {
                    if m < bound {
                        bound = m;
                    }
                }
                if let Some(pt) = &self.patches {
                    let mut accept = |uv: [f64; 2], x: [f64; 3]| self.inside(uv, x, tol);
                    let found = pt.search(s, p, bound + FACE_TIE_WINDOW_M, &mut accept)?;
                    unresolved = found.unresolved;
                    for f in found.found {
                        offer(
                            &mut best,
                            Cand { distance: f.distance, foot: f.foot, uv: Some(f.uv), feat: Feat::Interior },
                        );
                    }
                }
            }
        }
        let Some(b) = best else { return Ok(FaceProjection::NoHit) };
        let uv = match b.uv {
            Some(uv) => uv,
            None => self.uv_of(b.foot)?,
        };
        let feature = match b.feat {
            Feat::Interior => FaceFeature::Interior,
            Feat::Edge(k) => FaceFeature::Edge(self.edges.get(k).map(|e| e.id.clone()).ok_or(FaceError::NonFinite)?),
            Feat::Vertex(k) => {
                FaceFeature::Vertex(self.vertices.get(k).map(|v| v.0.clone()).ok_or(FaceError::NonFinite)?)
            }
        };
        let hit = FaceHit { foot: b.foot, distance: b.distance, face: self.face_id.clone(), uv, feature };
        let beyond = opts.max_distance.is_some_and(|m| hit.distance > m);
        if unresolved {
            return Err(FaceError::NonConvergence { best: if beyond { None } else { Some(hit) } });
        }
        if beyond {
            return Ok(FaceProjection::NoHit);
        }
        Ok(FaceProjection::Hit(hit))
    }
}

/// One candidate foot.
#[derive(Debug, Clone)]
struct Cand {
    distance: f64,
    foot: [f64; 3],
    uv: Option<[f64; 2]>,
    feat: Feat,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Feat {
    Interior,
    /// Position in the ascending edge list.
    Edge(usize),
    /// Position in the ascending vertex list.
    Vertex(usize),
}

impl Feat {
    fn rank(self) -> (u8, usize) {
        match self {
            Feat::Interior => (0, 0),
            Feat::Edge(k) => (1, k),
            Feat::Vertex(k) => (2, k),
        }
    }
}

impl Cand {
    /// Tie rules: lower distance; within the tie window Interior < Edge < Vertex, then the
    /// lower ID; otherwise the earlier candidate stays.
    fn better_than(&self, b: &Cand) -> bool {
        if self.distance + FACE_TIE_WINDOW_M < b.distance {
            return true;
        }
        if b.distance + FACE_TIE_WINDOW_M < self.distance {
            return false;
        }
        let (ra, rb) = (self.feat.rank(), b.feat.rank());
        if ra != rb {
            return ra < rb;
        }
        self.distance < b.distance
    }
}

/// Derives the pcurves of `body` at [`DEFAULT_TOL_M`], prepares `face` and projects `p`
/// (the Build prompt's `project_face`). Use [`FaceProjector`] for many queries.
pub fn project_face(
    body: &Body,
    face: FaceIx,
    p: [f64; 3],
    opts: FaceQueryOptions,
) -> Result<FaceProjection, FaceError> {
    if !p.iter().all(|x| x.is_finite()) {
        return Err(FaceError::NonFiniteQuery);
    }
    let pc = derive_pcurves(body, DEFAULT_TOL_M).map_err(FaceError::Pcurve)?;
    FaceProjector::new(body, &pc, face)?.project(p, opts)
}
