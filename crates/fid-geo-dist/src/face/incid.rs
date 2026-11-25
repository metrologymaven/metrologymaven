//! Incident normals and angles for the signed distance to a B-rep body. Spec node
//! `GEO.DIST.FACE.INCID`.
//!
//! GEO.DIST.SIGN defines one pseudo-normal rule for meshes and CAD: the sign of
//! `(p - foot) . N`, with `N = sum angle_i normal_i` over the faces incident at the foot
//! ([`cad_sign`]). This module computes those inputs from the topology of a [`Body`] for a
//! [`FaceHit`] of GEO.DIST.FACE.ONE and signs it. [`Incid::new`] prepares a body once (its
//! [`Adjacency`] and its [`Pcurves`]); [`Incid::incident_at`] returns the incident normals and
//! angles; [`Incid::signed`] the signed distance.
//!
//! # Incident faces (REQ-001 to REQ-004)
//!
//! - **Interior foot:** the hit face's outward normal at the hit's `uv` (unit surface normal
//!   `Su x Sv / |Su x Sv|`, negated for a face of sense `Reversed`), angle `2 pi`.
//! - **Edge foot:** the edge must be used by exactly two coedges (two faces, or one face twice
//!   across a seam); one use is [`SignUndefined::Boundary`], more than two
//!   [`SignUndefined::NonManifold`] (REQ-004). The foot's parameter `t` on the edge comes from
//!   the edge's 3-D curve (GEO.NURBS.ANALYTIC closed form, or the certified GEO.NURBS.INVERT
//!   curve inversion), brought into the edge range; each coedge maps `t` to its face through
//!   its trim curve (GEO.CAD.TOPO.PCURVE: the imported pcurve at `t`, or the derived polyline
//!   interpolated linearly in `t`). Each face's outward normal there, angle `pi` each, in
//!   ascending (face ID, coedge ID) order.
//! - **Vertex foot:** every face around the vertex in ascending face ID order (REQ-003), with
//!   its outward normal at the vertex and its interior angle: the sum over the face's corners
//!   at the vertex (a loop's coedge ending at the vertex followed by the next coedge, which
//!   starts there) of the angle from the outgoing tangent `a` to the reversed incoming tangent
//!   `b`, both projected into the tangent plane, measured counter-clockwise about the outward
//!   normal `n`: `atan2(n . (a x b), a . b)`, moved into `(0, 2 pi]` (loops run with the face
//!   on their left seen from the outward normal, so a square's corner is `pi / 2` and a
//!   reflex corner more than `pi`). Every edge at the vertex must have two coedges, and the
//!   corners must link all of them into one fan, else the sign is undefined
//!   ([`SignUndefined::Boundary`], [`SignUndefined::NonManifold`],
//!   [`SignUndefined::NonManifoldVertex`]).
//!
//! **Pole coedges** (no 3-D curve, start vertex = end vertex, and the trim curve's surface
//! image at five parameters within [`DEFAULT_TOL_M`] of that vertex: the degenerate boundary
//! line of a sphere or NURBS pole) are not boundary: they are passed over when corners are
//! formed and are not counted as edges of the vertex.
//!
//! **Tangents** of a coedge at a vertex: the edge's 3-D curve derivative (times `-1` for a
//! coedge of sense `Reversed`), used when the curve passes within [`MAP_TOL_M`] of the vertex;
//! otherwise `Su du + Sv dv` from the trim curve's derivative. A tangent that is zero, or a
//! corner angle that is not certainly in `(0, 2 pi]`, is [`SignUndefined::Degenerate`].
//!
//! # Parameters on a face (fail closed)
//!
//! A `uv` taken from a trim curve or from the hit is accepted only when the surface point at
//! it lies within [`MAP_TOL_M`] of the foot (or vertex). Otherwise, or when no trim curve or
//! edge parameter is available (an edge without a 3-D curve, a coedge whose pcurve could not
//! be derived), the foot is inverted onto the face surface: closed form for analytic
//! surfaces, GEO.NURBS.INVERT for NURBS surfaces, accepted only when certified; an
//! uncertified answer is [`IncidError::Inversion`], and an inverted `uv` still farther than
//! [`MAP_TOL_M`] is [`IncidError::Mapping`]. No normal is ever taken at an unverified place.
//!
//! # Degenerate normals (surface poles and apexes)
//!
//! Where `Su x Sv` is degenerate (GEO.NURBS.EVAL `Regularity::Degenerate`: a collapsed NURBS
//! row, a cone apex), the normal is taken at a parameter offset of [`FALLBACK_REL`] = 1e-9 of
//! the domain width (`1e-9 max(1, |x|)` for an unbounded parameter) into the face, the first
//! regular one of: for a vertex foot, the outgoing coedge's trim curve stepped `1e-9` of its
//! edge range along the coedge, then the incoming coedge's stepped back; then, for every foot,
//! `(u, v + dv)`, `(u, v - dv)`, `(u + du, v)`, `(u - du, v)`, skipping any outside a bounded
//! domain (a sphere's latitude, a non-periodic NURBS direction). [`Incident::fallback`]
//! records the fallback. No regular normal among them is [`SignUndefined::Degenerate`].
//!
//! # Determinism (D0, KNP-1)
//!
//! Binary64 `+ - * /` one operation at a time (no FMA, no `f64::min`/`max`), `hypot` and
//! `atan2` from `fid_num::f`, evaluation and inversion inside GEO.NURBS, IDs in their total
//! order, vectors only (no hash containers), every loop bounded by the body's size. Nothing
//! panics.

use core::f64::consts::{FRAC_PI_2, PI, TAU};
use core::fmt;

use fid_geo_cad::{
    derive_pcurves, AdjError, Adjacency, Body, Coedge, CoedgeIx, Edge, EdgeIx, EntityIx, FaceIx, PcurveError, Pcurves,
    Sense, TopologyId, TrimCurve, VertexIx, DEFAULT_TOL_M,
};
use fid_geo_nurbs::{invert_curve, invert_surface, AnalyticSurface, Curve, NurbsSurface, Surface};

use super::one::{FaceFeature, FaceHit};
use crate::sign::{cad_sign, SignError, SignStatus, SignUndefined, Signed};
use crate::FACE_REF_FOOT_TOL_M;

type V3 = [f64; 3];

/// Accuracy of a foot from GEO.DIST.FACE.ONE (its REQ-002, 1e-9 m \[unverified\]), passed to
/// [`cad_sign`] as the foot error: a query closer to the surface than about this is
/// [`SignStatus::Undecidable`] rather than guessed.
pub const FOOT_ERROR_M: f64 = FACE_REF_FOOT_TOL_M;

/// A `uv` is used for a normal only when its surface point lies within this distance of the
/// foot or vertex (metres) \[unverified\]; a derived pcurve is within 1e-8 m of its edge.
pub const MAP_TOL_M: f64 = 1e-6;

/// Parameter offset of the degenerate-normal fallback, relative to the domain width.
pub const FALLBACK_REL: f64 = 1e-9;

/// Parameters at which a pole coedge's trim curve is checked against its vertex.
const POLE_SAMPLES: usize = 5;

// ---------------------------------------------------------------- public types

/// One incident face at the foot: its outward unit normal and its weight (incident angle).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Incident {
    /// Outward unit normal (surface normal times face sense).
    pub normal: [f64; 3],
    /// Incident angle in radians, `0 < angle <= 2 pi`: `2 pi` interior, `pi` at an edge, the
    /// face's interior angle at a vertex.
    pub angle: f64,
    /// The normal came from the degenerate-normal fallback.
    pub fallback: bool,
}

/// Why the incident normals could not be computed.
#[derive(Debug, Clone, PartialEq)]
pub enum IncidError {
    /// [`Incid::new`]: GEO.CAD.TOPO.PCURVE refused the body.
    Pcurve(PcurveError),
    /// [`Incid::from_parts`]: the pcurves do not belong to the adjacency's body.
    BadPcurves,
    /// The hit has a NaN or infinite foot, `uv` or distance, or a negative distance.
    NonFiniteHit,
    /// The hit's face is not a face of the body.
    UnknownFace { face: TopologyId },
    /// The hit's edge or vertex is not an edge or vertex of the hit's face.
    NotOnFace { entity: TopologyId },
    /// An adjacency query refused an ID.
    Adjacency(AdjError),
    /// The foot could not be inverted onto the face surface with a certified answer.
    Inversion { face: TopologyId },
    /// No `uv` on the face reproduces the foot (or vertex) within [`MAP_TOL_M`].
    Mapping { face: TopologyId, deviation_m: f64 },
    /// A surface or curve evaluation failed or overflowed.
    Eval,
    /// A loop of the face does not continue at the vertex (unreachable for a built body).
    BrokenLoop { face: TopologyId },
    /// [`cad_sign`] refused its inputs.
    Sign(SignError),
}

impl IncidError {
    /// Stable variant name.
    pub fn code(&self) -> &'static str {
        match self {
            IncidError::Pcurve(_) => "Pcurve",
            IncidError::BadPcurves => "BadPcurves",
            IncidError::NonFiniteHit => "NonFiniteHit",
            IncidError::UnknownFace { .. } => "UnknownFace",
            IncidError::NotOnFace { .. } => "NotOnFace",
            IncidError::Adjacency(_) => "Adjacency",
            IncidError::Inversion { .. } => "Inversion",
            IncidError::Mapping { .. } => "Mapping",
            IncidError::Eval => "Eval",
            IncidError::BrokenLoop { .. } => "BrokenLoop",
            IncidError::Sign(_) => "Sign",
        }
    }
}

impl fmt::Display for IncidError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            IncidError::Pcurve(e) => write!(f, "pcurves: {e}"),
            IncidError::BadPcurves => write!(f, "pcurves do not belong to the body"),
            IncidError::NonFiniteHit => write!(f, "hit has a non-finite or negative value"),
            IncidError::UnknownFace { face } => write!(f, "{face} is not a face of the body"),
            IncidError::NotOnFace { entity } => write!(f, "{entity} is not on the hit face"),
            IncidError::Adjacency(e) => write!(f, "adjacency: {e}"),
            IncidError::Inversion { face } => write!(f, "foot could not be inverted onto face {face}"),
            IncidError::Mapping { face, deviation_m } => {
                write!(f, "no parameters on face {face} reproduce the foot (deviation {deviation_m:e} m)")
            }
            IncidError::Eval => write!(f, "evaluation failed"),
            IncidError::BrokenLoop { face } => write!(f, "a loop of face {face} does not continue at the vertex"),
            IncidError::Sign(e) => write!(f, "cad_sign: {e}"),
        }
    }
}

impl std::error::Error for IncidError {}

impl From<AdjError> for IncidError {
    fn from(e: AdjError) -> Self {
        IncidError::Adjacency(e)
    }
}

/// Why [`Incid::incident_at`] gives no normals: the sign is not defined there (a flag, not
/// an error, REQ-004), or the computation failed.
#[derive(Debug, Clone, PartialEq)]
pub enum Undefined {
    /// The rule defines no sign at this foot: the reason and the entity that causes it.
    Sign { reason: SignUndefined, entity: TopologyId },
    /// The computation failed.
    Error(IncidError),
}

impl From<IncidError> for Undefined {
    fn from(e: IncidError) -> Self {
        Undefined::Error(e)
    }
}

impl From<AdjError> for Undefined {
    fn from(e: AdjError) -> Self {
        Undefined::Error(IncidError::Adjacency(e))
    }
}

// ---------------------------------------------------------------- small helpers

fn sub3(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn dot3(a: V3, b: V3) -> f64 {
    (a[0] * b[0] + a[1] * b[1]) + a[2] * b[2]
}

fn cross3(a: V3, b: V3) -> V3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

fn scale3(a: V3, s: f64) -> V3 {
    [a[0] * s, a[1] * s, a[2] * s]
}

fn finite3(a: V3) -> bool {
    a.iter().all(|x| x.is_finite())
}

/// `hypot(hypot(a0, a1), a2)`, `None` when not finite.
fn norm3(a: V3) -> Option<f64> {
    let h = fid_num::f::hypot(a[0], a[1]).ok()?;
    let n = fid_num::f::hypot(h, a[2]).ok()?;
    n.is_finite().then_some(n)
}

/// `a / |a|`, `None` for a zero or non-finite vector.
fn unit3(a: V3) -> Option<V3> {
    let n = norm3(a)?;
    if n > 0.0 {
        let u = [a[0] / n, a[1] / n, a[2] / n];
        finite3(u).then_some(u)
    } else {
        None
    }
}

/// `x` limited to `[lo, hi]` (no `f64::min`/`max`).
fn clamp(x: f64, lo: f64, hi: f64) -> f64 {
    if x < lo {
        lo
    } else if x > hi {
        hi
    } else {
        x
    }
}

fn magnitude(x: f64) -> f64 {
    let a = x.abs();
    if a > 1.0 {
        a
    } else {
        1.0
    }
}

/// Start and end vertex of a coedge in its own direction.
fn ends(co: &Coedge, e: &Edge) -> (VertexIx, VertexIx) {
    match co.sense() {
        Sense::Same => (e.start(), e.end()),
        Sense::Reversed => (e.end(), e.start()),
    }
}

/// Edge parameters at the start and end of a coedge, and its direction in `t` (`+1`, `-1`).
fn coedge_params(co: &Coedge, e: &Edge) -> (f64, f64, f64) {
    let [t0, t1] = e.param_range();
    match co.sense() {
        Sense::Same => (t0, t1, 1.0),
        Sense::Reversed => (t1, t0, -1.0),
    }
}

// ---------------------------------------------------------------- face surface

#[derive(Clone, Copy)]
enum Surf<'a> {
    Analytic(&'a AnalyticSurface),
    Nurbs(&'a NurbsSurface),
}

impl<'a> Surf<'a> {
    fn dynamic(&self) -> &'a dyn Surface {
        match *self {
            Surf::Analytic(s) => s,
            Surf::Nurbs(s) => s,
        }
    }

    /// Parameters moved into the evaluable domain: a sphere's latitude clamped to
    /// `[-pi/2, pi/2]`; a NURBS direction wrapped by its period when periodic, else clamped.
    /// Analytic periodic parameters are evaluated as given.
    fn prep(&self, uv: [f64; 2]) -> [f64; 2] {
        match *self {
            Surf::Analytic(AnalyticSurface::Sphere(_)) => [uv[0], clamp(uv[1], -FRAC_PI_2, FRAC_PI_2)],
            Surf::Analytic(_) => uv,
            Surf::Nurbs(n) => {
                let ((u0, u1), (v0, v1)) = n.domain();
                let fit = |x: f64, lo: f64, hi: f64, periodic: bool| -> f64 {
                    if periodic {
                        let w = hi - lo;
                        let y = x - w * ((x - lo) / w).floor();
                        clamp(y, lo, hi)
                    } else {
                        clamp(x, lo, hi)
                    }
                };
                [fit(uv[0], u0, u1, n.periodic_u()), fit(uv[1], v0, v1, n.periodic_v())]
            }
        }
    }

    /// Whether `uv` lies in a bounded direction's domain (fallback candidates only).
    fn in_domain(&self, uv: [f64; 2]) -> bool {
        match *self {
            Surf::Analytic(AnalyticSurface::Sphere(_)) => (-FRAC_PI_2..=FRAC_PI_2).contains(&uv[1]),
            Surf::Analytic(_) => true,
            Surf::Nurbs(n) => {
                let ((u0, u1), (v0, v1)) = n.domain();
                (n.periodic_u() || (u0..=u1).contains(&uv[0])) && (n.periodic_v() || (v0..=v1).contains(&uv[1]))
            }
        }
    }

    fn point(&self, uv: [f64; 2]) -> Result<V3, IncidError> {
        let uv = self.prep(uv);
        let p = self.dynamic().eval(uv[0], uv[1], 0).map_err(|_| IncidError::Eval)?.p;
        if finite3(p) {
            Ok(p)
        } else {
            Err(IncidError::Eval)
        }
    }

    /// `(S, Su, Sv)` at `uv`.
    fn first(&self, uv: [f64; 2]) -> Result<[V3; 3], IncidError> {
        let uv = self.prep(uv);
        let d = self.dynamic().eval(uv[0], uv[1], 1).map_err(|_| IncidError::Eval)?;
        match (d.su, d.sv) {
            (Some(su), Some(sv)) if finite3(su) && finite3(sv) => Ok([d.p, su, sv]),
            _ => Err(IncidError::Eval),
        }
    }

    /// Unit surface normal, `None` where degenerate.
    fn normal(&self, uv: [f64; 2]) -> Result<Option<V3>, IncidError> {
        let uv = self.prep(uv);
        let n = self.dynamic().normal(uv[0], uv[1]).map_err(|_| IncidError::Eval)?;
        Ok(n.filter(|n| finite3(*n)))
    }

    /// Certified surface parameters of a point on (or next to) the surface.
    fn invert(&self, q: V3, face: &TopologyId) -> Result<[f64; 2], IncidError> {
        let fail = || IncidError::Inversion { face: face.clone() };
        match *self {
            Surf::Analytic(s) => s.invert(q).map(|f| [f.u, f.v]).map_err(|_| fail()),
            // GEO.NURBS.INVERT returns `Ok` only for a certified answer (fail closed).
            Surf::Nurbs(s) => invert_surface(s, q).map(|i| [i.u, i.v]).map_err(|_| fail()),
        }
    }

    /// Offsets of the degenerate-normal fallback, in the fixed order.
    fn offsets(&self, uv: [f64; 2]) -> [[f64; 2]; 4] {
        let ((u0, u1), (v0, v1)) = self.dynamic().domain();
        let step = |x: f64, lo: f64, hi: f64| -> f64 {
            let w = hi - lo;
            if w.is_finite() {
                FALLBACK_REL * w
            } else {
                FALLBACK_REL * magnitude(x)
            }
        };
        let (du, dv) = (step(uv[0], u0, u1), step(uv[1], v0, v1));
        [[uv[0], uv[1] + dv], [uv[0], uv[1] - dv], [uv[0] + du, uv[1]], [uv[0] - du, uv[1]]]
    }
}

// ---------------------------------------------------------------- the prepared body

/// A body prepared for incident-normal and sign queries: its adjacency index and its
/// pcurves (GEO.CAD.TOPO.ADJ, GEO.CAD.TOPO.PCURVE).
#[derive(Debug, Clone)]
pub struct Incid<'b> {
    adj: Adjacency<'b>,
    pcurves: Pcurves,
}

/// A face's outward normal and whether the fallback gave it.
type Outward = Option<(V3, bool)>;

impl<'b> Incid<'b> {
    /// Builds the adjacency of `body` and derives its pcurves at [`DEFAULT_TOL_M`]. A coedge
    /// whose pcurve cannot be derived is not an error here: its face parameters come from
    /// inverting the foot onto the face surface instead.
    pub fn new(body: &'b Body) -> Result<Incid<'b>, IncidError> {
        let pcurves = derive_pcurves(body, DEFAULT_TOL_M).map_err(IncidError::Pcurve)?;
        Incid::from_parts(Adjacency::build(body), pcurves)
    }

    /// From an adjacency index and the pcurves of the same body (shared with
    /// GEO.DIST.FACE.ONE); [`IncidError::BadPcurves`] when the counts do not match the body.
    pub fn from_parts(adj: Adjacency<'b>, pcurves: Pcurves) -> Result<Incid<'b>, IncidError> {
        let body = adj.body();
        if pcurves.curves.len() != body.coedges().len() || pcurves.charts.len() != body.faces().len() {
            return Err(IncidError::BadPcurves);
        }
        Ok(Incid { adj, pcurves })
    }

    /// The body.
    pub fn body(&self) -> &'b Body {
        self.adj.body()
    }

    /// The adjacency index.
    pub fn adjacency(&self) -> &Adjacency<'b> {
        &self.adj
    }

    /// The pcurves.
    pub fn pcurves(&self) -> &Pcurves {
        &self.pcurves
    }

    /// The signed distance of `p` for its closest point `hit` (module docs): [`cad_sign`] with
    /// the incident normals and [`FOOT_ERROR_M`]. Where the sign is not defined (REQ-004) the
    /// result has status [`SignStatus::Undefined`] and the unsigned distance; that is not an
    /// error.
    pub fn signed(&self, p: [f64; 3], hit: &FaceHit) -> Result<Signed, IncidError> {
        match self.incident_at(hit) {
            Ok(list) => {
                let pairs: Vec<(V3, f64)> = list.iter().map(|i| (i.normal, i.angle)).collect();
                cad_sign(p, hit.foot, FOOT_ERROR_M, &pairs).map_err(IncidError::Sign)
            }
            Err(Undefined::Error(e)) => Err(e),
            Err(Undefined::Sign { reason, .. }) => {
                // The distance and its bound do not depend on the normals: take them from the
                // same rule with a placeholder normal, then withhold the sign.
                let s = cad_sign(p, hit.foot, FOOT_ERROR_M, &[([0.0, 0.0, 1.0], PI)]).map_err(IncidError::Sign)?;
                Ok(Signed {
                    distance: s.unsigned,
                    unsigned: s.unsigned,
                    status: SignStatus::Undefined(reason),
                    bound: s.bound,
                })
            }
        }
    }

    /// The faces incident at the foot of `hit`, their outward normals and angles, in the
    /// order of REQ-002 and REQ-003 (module docs), or why there are none.
    pub fn incident_at(&self, hit: &FaceHit) -> Result<Vec<Incident>, Undefined> {
        if !(finite3(hit.foot) && hit.uv.iter().all(|x| x.is_finite()) && hit.distance.is_finite())
            || hit.distance < 0.0
        {
            return Err(IncidError::NonFiniteHit.into());
        }
        let face = self.face_ix(&hit.face)?;
        match &hit.feature {
            FaceFeature::Interior => self.interior(face, &hit.face, hit),
            FaceFeature::Edge(id) => self.at_edge(face, id, hit.foot),
            FaceFeature::Vertex(id) => self.at_vertex(&hit.face, id),
        }
    }

    // ------------------------------------------------------------ lookups

    fn face_ix(&self, id: &TopologyId) -> Result<FaceIx, IncidError> {
        match self.body().lookup(id) {
            Some(EntityIx::Face(f)) => Ok(f),
            _ => Err(IncidError::UnknownFace { face: id.clone() }),
        }
    }

    fn coedge_ix(&self, id: &TopologyId) -> Result<CoedgeIx, IncidError> {
        match self.body().lookup(id) {
            Some(EntityIx::Coedge(c)) => Ok(c),
            _ => Err(IncidError::Eval),
        }
    }

    /// The coedge, its edge and its face.
    fn coedge(&self, c: CoedgeIx) -> Result<(&'b Coedge, &'b Edge, FaceIx), IncidError> {
        let body = self.body();
        let co = body.coedge(c).ok_or(IncidError::Eval)?;
        let e = body.edge(co.edge()).ok_or(IncidError::Eval)?;
        let f = body.r#loop(co.r#loop()).ok_or(IncidError::Eval)?.face();
        Ok((co, e, f))
    }

    fn surface(&self, f: FaceIx) -> Result<(Surf<'b>, Sense, &'b TopologyId), IncidError> {
        let face = self.body().face(f).ok_or(IncidError::Eval)?;
        let s = match (face.surface().as_analytic(), face.surface().as_nurbs()) {
            (Some(a), _) => Surf::Analytic(a),
            (None, Some(n)) => Surf::Nurbs(n),
            (None, None) => return Err(IncidError::Eval),
        };
        Ok((s, face.sense(), face.id()))
    }

    fn vertex_point(&self, v: VertexIx) -> Result<V3, IncidError> {
        self.body().vertex(v).map(|x| x.point()).ok_or(IncidError::Eval)
    }

    // ------------------------------------------------------------ trim curves

    /// The coedge's trim curve at edge parameter `t`: `uv` and `duv/dt` (when defined), or
    /// `None` when the coedge has no usable trim curve.
    fn trim_at(&self, c: CoedgeIx, t: f64) -> Option<([f64; 2], Option<[f64; 2]>)> {
        match self.pcurves.curves.get(c.index())? {
            Ok(TrimCurve::Imported(curve)) => {
                if let Some((o, d)) = curve.as_line() {
                    let uv = [o[0] + t * d[0], o[1] + t * d[1]];
                    return Some((uv, Some(d)));
                }
                let n = curve.as_nurbs()?;
                let e = n.eval(t, 1).ok()?;
                let d = e.d1.map(|d| [d[0], d[1]]);
                Some(([e.p[0], e.p[1]], d))
            }
            Ok(TrimCurve::Derived { uv, t: ts, .. }) => interpolate(uv, ts, t),
            Err(_) => None,
        }
    }

    /// Whether coedge `c` is a pole coedge (module docs).
    fn is_pole(&self, c: CoedgeIx) -> Result<bool, IncidError> {
        let (_, e, f) = self.coedge(c)?;
        if e.curve().is_some() || e.start() != e.end() {
            return Ok(false);
        }
        let (surf, _, _) = self.surface(f)?;
        let at = self.vertex_point(e.start())?;
        let [t0, t1] = e.param_range();
        for k in 0..POLE_SAMPLES {
            let t = if k + 1 == POLE_SAMPLES { t1 } else { t0 + ((t1 - t0) * k as f64) / (POLE_SAMPLES - 1) as f64 };
            let Some((uv, _)) = self.trim_at(c, t) else { return Ok(false) };
            let d = norm3(sub3(surf.point(uv)?, at)).ok_or(IncidError::Eval)?;
            if d > DEFAULT_TOL_M {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// The parameter of `foot` on edge `e`, brought into the edge range; `None` when the edge
    /// has no 3-D curve or its curve does not pass within [`MAP_TOL_M`] of the foot.
    fn edge_param(&self, e: &Edge, foot: V3) -> Option<f64> {
        let curve = e.curve()?;
        let [t0, t1] = e.param_range();
        let (t, d) = if let Some(a) = curve.as_analytic() {
            let f = a.invert(foot).ok()?;
            let (lo, hi) = a.domain();
            let periodic = (hi - lo).is_finite();
            let t = if periodic { into_range(f.t, hi - lo, t0, t1) } else { f.t };
            (t, f.distance)
        } else {
            // GEO.NURBS.INVERT: `Ok` only for a certified answer (fail closed); a tie at the
            // joint of a closed curve gives the same point.
            let i = invert_curve(curve.as_nurbs()?, foot).ok()?;
            (i.t, i.distance)
        };
        (d <= MAP_TOL_M).then_some(clamp(t, t0, t1))
    }

    /// Verified parameters of `point` on the face of coedge `c`: the trim curve at `t` when
    /// given, else (or when that misses) the certified inversion (module docs).
    fn uv_on(&self, c: CoedgeIx, t: Option<f64>, point: V3) -> Result<[f64; 2], IncidError> {
        let (_, _, f) = self.coedge(c)?;
        let (surf, _, id) = self.surface(f)?;
        if let Some((uv, _)) = t.and_then(|t| self.trim_at(c, t)) {
            if deviation(&surf, uv, point)? <= MAP_TOL_M {
                return Ok(uv);
            }
        }
        self.verified_inversion(&surf, id, point)
    }

    fn verified_inversion(&self, surf: &Surf<'_>, id: &TopologyId, point: V3) -> Result<[f64; 2], IncidError> {
        let uv = surf.invert(point, id)?;
        let dev = deviation(surf, uv, point)?;
        if dev <= MAP_TOL_M {
            Ok(uv)
        } else {
            Err(IncidError::Mapping { face: id.clone(), deviation_m: dev })
        }
    }

    // ------------------------------------------------------------ normals

    /// The outward normal of face `f` at `uv`, else at the fallback parameters: first
    /// `along` (in order), then the fixed domain offsets.
    fn outward(&self, f: FaceIx, uv: [f64; 2], along: &[[f64; 2]]) -> Result<Outward, IncidError> {
        let (surf, sense, _) = self.surface(f)?;
        let orient = |n: V3| match sense {
            Sense::Same => n,
            Sense::Reversed => [-n[0], -n[1], -n[2]],
        };
        if let Some(n) = surf.normal(uv)? {
            return Ok(Some((orient(n), false)));
        }
        for &w in along.iter().chain(surf.offsets(uv).iter()) {
            if !surf.in_domain(w) {
                continue;
            }
            if let Some(n) = surf.normal(w)? {
                return Ok(Some((orient(n), true)));
            }
        }
        Ok(None)
    }

    // ------------------------------------------------------------ the three cases

    fn interior(&self, f: FaceIx, id: &TopologyId, hit: &FaceHit) -> Result<Vec<Incident>, Undefined> {
        let (surf, _, _) = self.surface(f)?;
        let uv = if deviation(&surf, hit.uv, hit.foot)? <= MAP_TOL_M {
            hit.uv
        } else {
            self.verified_inversion(&surf, id, hit.foot)?
        };
        match self.outward(f, uv, &[])? {
            Some((normal, fallback)) => Ok(vec![Incident { normal, angle: TAU, fallback }]),
            None => Err(Undefined::Sign { reason: SignUndefined::Degenerate, entity: id.clone() }),
        }
    }

    /// The coedges of edge `id` sorted by (face ID, coedge ID), checked to be exactly two.
    fn two_uses(&self, id: &TopologyId) -> Result<Vec<(TopologyId, CoedgeIx)>, Undefined> {
        let ids = self.adj.coedges_of_edge(id)?;
        if ids.len() != 2 {
            let reason = if ids.len() < 2 { SignUndefined::Boundary } else { SignUndefined::NonManifold };
            return Err(Undefined::Sign { reason, entity: id.clone() });
        }
        let mut uses = Vec::with_capacity(2);
        for cid in &ids {
            let c = self.coedge_ix(cid)?;
            let (_, _, f) = self.coedge(c)?;
            let fid = self.body().face(f).ok_or(IncidError::Eval)?.id().clone();
            uses.push((fid, cid.clone(), c));
        }
        uses.sort_by(|a, b| (&a.0, &a.1).cmp(&(&b.0, &b.1)));
        Ok(uses.into_iter().map(|(f, _, c)| (f, c)).collect())
    }

    fn at_edge(&self, face: FaceIx, id: &TopologyId, foot: V3) -> Result<Vec<Incident>, Undefined> {
        let not_on = || Undefined::Error(IncidError::NotOnFace { entity: id.clone() });
        let e = match self.body().lookup(id) {
            Some(EntityIx::Edge(e)) => e,
            _ => return Err(not_on()),
        };
        // The edge must bound the hit face.
        let mut on_face = false;
        for cid in self.adj.coedges_of_edge(id)? {
            let (_, _, f) = self.coedge(self.coedge_ix(&cid)?)?;
            on_face |= f == face;
        }
        if !on_face {
            return Err(not_on());
        }
        let uses = self.two_uses(id)?;
        let edge = self.body().edge(e).ok_or(IncidError::Eval)?;
        let t = self.edge_param(edge, foot);
        let mut out = Vec::with_capacity(2);
        for (fid, c) in uses {
            let uv = self.uv_on(c, t, foot)?;
            let (_, _, f) = self.coedge(c)?;
            match self.outward(f, uv, &[])? {
                Some((normal, fallback)) => out.push(Incident { normal, angle: PI, fallback }),
                None => return Err(Undefined::Sign { reason: SignUndefined::Degenerate, entity: fid }),
            }
        }
        Ok(out)
    }

    fn at_vertex(&self, face_id: &TopologyId, id: &TopologyId) -> Result<Vec<Incident>, Undefined> {
        let not_on = || Undefined::Error(IncidError::NotOnFace { entity: id.clone() });
        let v = match self.body().lookup(id) {
            Some(EntityIx::Vertex(v)) => v,
            _ => return Err(not_on()),
        };
        let faces = self.adj.faces_of_vertex(id)?;
        if !faces.contains(face_id) {
            return Err(not_on());
        }
        let at = self.vertex_point(v)?;
        // Edges of the vertex that count (pole edges do not), each with exactly two uses.
        let mut edges: Vec<EdgeIx> = Vec::new();
        for eid in self.adj.edges_of_vertex(id)? {
            let e = match self.body().lookup(&eid) {
                Some(EntityIx::Edge(e)) => e,
                _ => return Err(IncidError::Eval.into()),
            };
            let mut all_pole = true;
            for cid in self.adj.coedges_of_edge(&eid)? {
                all_pole &= self.is_pole(self.coedge_ix(&cid)?)?;
            }
            if all_pole {
                continue;
            }
            self.two_uses(&eid)?;
            edges.push(e);
        }
        let mut fan = Fan::new(edges.len());
        let mut out = Vec::with_capacity(faces.len());
        for fid in &faces {
            let f = self.face_ix(fid)?;
            let corners = self.corners(f, fid, v)?;
            let mut face_normal: Option<(V3, bool)> = None;
            let mut angle = 0.0;
            for &(c_in, c_out) in &corners {
                let (co_out, e_out, _) = self.coedge(c_out)?;
                let (co_in, e_in, _) = self.coedge(c_in)?;
                let (ts_out, _, dir_out) = coedge_params(co_out, e_out);
                let (_, te_in, dir_in) = coedge_params(co_in, e_in);
                let n = match face_normal {
                    Some(n) => n,
                    None => {
                        let uv = self.uv_on(c_out, Some(ts_out), at)?;
                        let [a0, a1] = e_out.param_range();
                        let [b0, b1] = e_in.param_range();
                        let mut along = Vec::with_capacity(2);
                        let step_out = ts_out + dir_out * (FALLBACK_REL * (a1 - a0));
                        if let Some((w, _)) = self.trim_at(c_out, step_out) {
                            along.push(w);
                        }
                        let step_in = te_in - dir_in * (FALLBACK_REL * (b1 - b0));
                        if let Some((w, _)) = self.trim_at(c_in, step_in) {
                            along.push(w);
                        }
                        match self.outward(f, uv, &along)? {
                            Some(n) => {
                                face_normal = Some(n);
                                n
                            }
                            None => {
                                return Err(Undefined::Sign { reason: SignUndefined::Degenerate, entity: fid.clone() })
                            }
                        }
                    }
                };
                let d_out = self.tangent(c_out, ts_out, at)?;
                let d_in = self.tangent(c_in, te_in, at)?;
                let theta = match (d_out, d_in) {
                    (Some(a), Some(b)) => corner_angle(n.0, b, a)?,
                    _ => None,
                };
                let Some(theta) = theta else {
                    return Err(Undefined::Sign { reason: SignUndefined::Degenerate, entity: id.clone() });
                };
                angle += theta;
                let ia = edges.iter().position(|&x| x == co_in.edge());
                let ib = edges.iter().position(|&x| x == co_out.edge());
                if let (Some(ia), Some(ib)) = (ia, ib) {
                    fan.join(ia, ib);
                }
            }
            if let Some((normal, fallback)) = face_normal {
                let valid = angle > 0.0 && angle <= TAU;
                if !valid {
                    return Err(Undefined::Sign { reason: SignUndefined::Degenerate, entity: fid.clone() });
                }
                out.push(Incident { normal, angle, fallback });
            }
        }
        if !fan.single() {
            return Err(Undefined::Sign { reason: SignUndefined::NonManifoldVertex, entity: id.clone() });
        }
        if out.is_empty() {
            return Err(IncidError::BrokenLoop { face: face_id.clone() }.into());
        }
        Ok(out)
    }

    /// The corners of face `f` at vertex `v`: `(incoming, outgoing)` coedge pairs, pole
    /// coedges passed over, in loop order.
    fn corners(&self, f: FaceIx, fid: &TopologyId, v: VertexIx) -> Result<Vec<(CoedgeIx, CoedgeIx)>, IncidError> {
        let body = self.body();
        let face = body.face(f).ok_or(IncidError::Eval)?;
        let broken = || IncidError::BrokenLoop { face: fid.clone() };
        let mut out = Vec::new();
        for &lx in face.loops() {
            let cs = body.r#loop(lx).ok_or(IncidError::Eval)?.coedges();
            let m = cs.len();
            let mut pole = Vec::with_capacity(m);
            for &c in cs {
                pole.push(self.is_pole(c)?);
            }
            for i in 0..m {
                let (Some(&ci), Some(&pi)) = (cs.get(i), pole.get(i)) else { return Err(IncidError::Eval) };
                let (co, e, _) = self.coedge(ci)?;
                if pi || ends(co, e).1 != v {
                    continue;
                }
                let mut next = None;
                for step in 1..=m {
                    let k = (i + step) % m;
                    let (Some(&ck), Some(&pk)) = (cs.get(k), pole.get(k)) else { return Err(IncidError::Eval) };
                    let (cok, ek, _) = self.coedge(ck)?;
                    if ends(cok, ek).0 != v {
                        return Err(broken());
                    }
                    if !pk {
                        next = Some(ck);
                        break;
                    }
                }
                out.push((ci, next.ok_or_else(broken)?));
            }
        }
        Ok(out)
    }

    /// The tangent of coedge `c` in its direction at edge parameter `t` (an end at vertex
    /// point `at`): the 3-D curve derivative when the curve passes within [`MAP_TOL_M`] of
    /// `at`, else `Su du + Sv dv` from the trim curve; `None` when zero; an error when neither
    /// reproduces the vertex.
    fn tangent(&self, c: CoedgeIx, t: f64, at: V3) -> Result<Option<V3>, IncidError> {
        let (co, e, f) = self.coedge(c)?;
        let sign = match co.sense() {
            Sense::Same => 1.0,
            Sense::Reversed => -1.0,
        };
        if let Some(curve) = e.curve() {
            let d = match (curve.as_analytic(), curve.as_nurbs()) {
                (Some(a), _) => a.eval(t, 1),
                (None, Some(n)) => n.eval(t, 1),
                (None, None) => return Err(IncidError::Eval),
            }
            .map_err(|_| IncidError::Eval)?;
            let near = norm3(sub3(d.p, at)).is_some_and(|x| x <= MAP_TOL_M);
            if near {
                let d1 = d.d1.ok_or(IncidError::Eval)?;
                let tan = scale3(d1, sign);
                if !finite3(tan) {
                    return Err(IncidError::Eval);
                }
                if tan.iter().any(|x| *x != 0.0) {
                    return Ok(Some(tan));
                }
            }
        }
        let (surf, _, id) = self.surface(f)?;
        let Some((uv, Some(duv))) = self.trim_at(c, t) else {
            return Err(IncidError::Mapping { face: id.clone(), deviation_m: f64::INFINITY });
        };
        let [p, su, sv] = surf.first(uv)?;
        let dev = norm3(sub3(p, at)).ok_or(IncidError::Eval)?;
        if dev > MAP_TOL_M {
            return Err(IncidError::Mapping { face: id.clone(), deviation_m: dev });
        }
        let tan: V3 = core::array::from_fn(|k| sign * (duv[0] * su[k] + duv[1] * sv[k]));
        if !finite3(tan) {
            return Err(IncidError::Eval);
        }
        Ok(tan.iter().any(|x| *x != 0.0).then_some(tan))
    }
}

/// Distance from the surface point at `uv` to `point`.
fn deviation(surf: &Surf<'_>, uv: [f64; 2], point: V3) -> Result<f64, IncidError> {
    norm3(sub3(surf.point(uv)?, point)).ok_or(IncidError::Eval)
}

/// `t` moved by whole periods `w` into `[t0, t1]`; outside by less than a period, the nearer
/// end (cyclically).
fn into_range(t: f64, w: f64, t0: f64, t1: f64) -> f64 {
    let s = t - w * ((t - t0) / w).floor();
    if s <= t1 {
        return clamp(s, t0, t1);
    }
    // `s` lies in (t1, t0 + w): past the end by `s - t1`, before the start by `t0 + w - s`.
    if s - t1 <= (t0 + w) - s {
        t1
    } else {
        t0
    }
}

/// A derived polyline (vertices `uv` at monotone edge parameters `ts`) at `t`: the segment
/// containing `t` (clamped to the ends), interpolated linearly; `duv/dt` of that segment.
fn interpolate(uv: &[[f64; 2]], ts: &[f64], t: f64) -> Option<([f64; 2], Option<[f64; 2]>)> {
    let n = uv.len();
    if n == 0 || ts.len() != n {
        return None;
    }
    if n == 1 {
        return Some((*uv.first()?, None));
    }
    let first = *ts.first()?;
    let last = *ts.last()?;
    let rising = last >= first;
    // Index of the first vertex past `t` in the polyline's direction (binary search).
    let past = |x: f64| if rising { x > t } else { x < t };
    let (mut lo, mut hi) = (0usize, n);
    while lo < hi {
        let mid = lo + (hi - lo) / 2;
        if past(*ts.get(mid)?) {
            hi = mid;
        } else {
            lo = mid + 1;
        }
    }
    let k = if lo == 0 {
        0
    } else if lo >= n {
        n - 2
    } else {
        lo - 1
    };
    let (a, b) = (*uv.get(k)?, *uv.get(k + 1)?);
    let (ta, tb) = (*ts.get(k)?, *ts.get(k + 1)?);
    let dt = tb - ta;
    if dt == 0.0 {
        return Some((a, None));
    }
    let s = clamp((t - ta) / dt, 0.0, 1.0);
    let at = [a[0] + s * (b[0] - a[0]), a[1] + s * (b[1] - a[1])];
    let d = [(b[0] - a[0]) / dt, (b[1] - a[1]) / dt];
    Some((at, Some(d)))
}

/// Interior angle at a corner: from the outgoing tangent `out` to the reversed incoming
/// tangent `-inc`, both projected into the plane of the unit normal `n`, counter-clockwise
/// about `n`, in `(0, 2 pi]`; `None` when a projected tangent vanishes or the angle is zero.
fn corner_angle(n: V3, inc: V3, out: V3) -> Result<Option<f64>, IncidError> {
    let project = |d: V3| sub3(d, scale3(n, dot3(d, n)));
    let (Some(a), Some(b)) = (unit3(project(out)), unit3(project([-inc[0], -inc[1], -inc[2]]))) else {
        return Ok(None);
    };
    let y = dot3(n, cross3(a, b));
    let x = dot3(a, b);
    let mut theta = fid_num::f::atan2(y, x).map_err(|_| IncidError::Eval)?;
    if theta < 0.0 {
        theta += TAU;
    }
    Ok((theta > 0.0 && theta <= TAU).then_some(theta))
}

/// Union-find over the edges of a vertex, joined by the corners between them.
struct Fan {
    parent: Vec<usize>,
}

impl Fan {
    fn new(n: usize) -> Fan {
        Fan { parent: (0..n).collect() }
    }

    fn root(&self, mut i: usize) -> usize {
        // Every step moves to a smaller index, so at most `n` steps.
        for _ in 0..self.parent.len() {
            match self.parent.get(i) {
                Some(&p) if p != i => i = p,
                _ => break,
            }
        }
        i
    }

    fn join(&mut self, a: usize, b: usize) {
        let (ra, rb) = (self.root(a), self.root(b));
        let (lo, hi) = if ra < rb { (ra, rb) } else { (rb, ra) };
        if let Some(p) = self.parent.get_mut(hi) {
            *p = lo;
        }
    }

    /// True when every edge is in one fan (or there are no edges).
    fn single(&self) -> bool {
        (0..self.parent.len()).all(|i| self.root(i) == 0)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;

    #[test]
    fn corner_angles() {
        let z = [0.0, 0.0, 1.0];
        // Square, counter-clockwise about +z: in along +x, out along +y.
        let a = corner_angle(z, [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]).unwrap().unwrap();
        assert!((a - FRAC_PI_2).abs() < 1e-15);
        // Reflex corner: in along +x, out along -y.
        let a = corner_angle(z, [1.0, 0.0, 0.0], [0.0, -1.0, 0.0]).unwrap().unwrap();
        assert!((a - 3.0 * FRAC_PI_2).abs() < 1e-15);
        // Straight through: pi.
        let a = corner_angle(z, [0.0, 1.0, 0.0], [0.0, 1.0, 0.0]).unwrap().unwrap();
        assert!((a - PI).abs() < 1e-15);
        // A cusp (out reverses in) has no angle; a normal-only tangent neither.
        assert_eq!(corner_angle(z, [1.0, 0.0, 0.0], [-1.0, 0.0, 0.0]).unwrap(), None);
        assert_eq!(corner_angle(z, [0.0, 0.0, 1.0], [0.0, 1.0, 0.0]).unwrap(), None);
    }

    #[test]
    fn range_shift() {
        assert_eq!(into_range(0.5, TAU, 0.0, TAU), 0.5);
        assert!((into_range(0.5 - TAU, TAU, 0.0, TAU) - 0.5).abs() < 1e-15);
        assert!((into_range(0.5, TAU, TAU, 2.0 * TAU) - (0.5 + TAU)).abs() < 1e-14);
        // Range [0, 1]: 1.1 is past the end, 6.2 is just before the start.
        assert_eq!(into_range(1.1, TAU, 0.0, 1.0), 1.0);
        assert_eq!(into_range(6.2, TAU, 0.0, 1.0), 0.0);
    }

    #[test]
    fn polyline_interpolation() {
        let uv = [[0.0, 0.0], [1.0, 0.0], [1.0, 2.0]];
        let ts = [0.0, 1.0, 3.0];
        let (p, d) = interpolate(&uv, &ts, 2.0).unwrap();
        assert_eq!(p, [1.0, 1.0]);
        assert_eq!(d, Some([0.0, 1.0]));
        assert_eq!(interpolate(&uv, &ts, -1.0).unwrap().0, [0.0, 0.0]);
        assert_eq!(interpolate(&uv, &ts, 9.0).unwrap().0, [1.0, 2.0]);
        // Decreasing parameters (a reversed coedge).
        let ts = [3.0, 2.0, 0.0];
        let (p, _) = interpolate(&uv, &ts, 1.0).unwrap();
        assert_eq!(p, [1.0, 1.0]);
    }

    #[test]
    fn fan_components() {
        let mut f = Fan::new(3);
        assert!(!f.single());
        f.join(2, 1);
        f.join(1, 0);
        assert!(f.single());
        let mut g = Fan::new(4);
        g.join(0, 1);
        g.join(2, 3);
        assert!(!g.single());
        assert!(Fan::new(0).single());
    }
}
