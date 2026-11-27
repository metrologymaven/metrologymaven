//! Direct mapping of analytic faces, edges and vertices to nominal elements. Spec node
//! `GEO.CAD.NOMINAL.DIRECT`.
//!
//! # Rules
//!
//! - **Parameters** (REQ-001, REQ-002): the element takes the CAD definition's given
//!   [`SurfaceParams`] or [`CurveParams`] unchanged (frame, radii, half-angle), so they are
//!   bit-identical to the definition; `method = Direct`, `max_deviation_m = 0`. A vertex gives a
//!   point at its stored position.
//! - **Material** (REQ-003): the natural normal `Su x Sv` of every analytic surface points to its
//!   outer side (see [`Material`]); a face with sense `Same` has its out-of-material normal there
//!   ([`Material::Outside`]), a `Reversed` face the other way ([`Material::Inside`]). So a
//!   reversed cylinder face (a hole) is `Inside`. Edges and vertices carry no material side.
//! - **Refusals** (REQ-004): a NURBS face or edge gives [`DirectResult::NotAnalytic`], an
//!   ellipse edge [`DirectResult::UnsupportedKind`]; nothing is approximated.
//! - **Extents** (REQ-005): in the element's orthonormalised placement (GEO.NURBS.ANALYTIC
//!   `Placement`), local coordinates `l = ((q - O) . X, (q - O) . Y, (q - O) . Z)` as computed by
//!   `Placement::to_local`. A plane face's extent is the box of `l.x`, `l.y`, a cylinder or cone
//!   face's the range of `l.z`, over the face's boundary: every vertex of every coedge of every
//!   loop, plus the interior extremes of its circle and ellipse edges. On such an edge
//!   `l.k(t) = c.k + A cos t + B sin t` with `c` the local centre, `A = (Xc . D) a` and
//!   `B = (Yc . D) b` (`D` the world direction of local axis `k`, `Xc`, `Yc` the curve's
//!   orthonormalised frame, `a`, `b` its semi-axes, both the radius for a circle), so the
//!   extremes are `c.k +- hypot(A, B)` at `t* = atan2(B, A)` and `t* + pi`, kept when that angle
//!   (plus whole turns) lies in the edge's parameter range (every angle when the range spans a
//!   full turn). Line edges reach their extremes at their vertices. The extremes of these
//!   coordinate functions over the face lie on its boundary (they are linear on planes,
//!   cylinders and cones), so the boundary gives the extent exactly up to rounding. Every value
//!   is computed in the fixed order written here with `fid_num::f` for `hypot` and `atan2` and
//!   the total-order min and max (KNP-1: no FMA, no `f64::min` or `f64::max`), so the result is
//!   D0. A face without loops is [`Extent::Unbounded`]; sphere and torus faces (whose height
//!   can peak inside the face) and points are [`Extent::NotDefined`]; a line edge's extent is
//!   its parameter range ([`Extent::Axial`]), a circle edge's its angle range
//!   ([`Extent::Angular`]), both as recorded on the edge.
//! - **Failing closed:** an extent that needs a boundary edge with no curve
//!   ([`DirectError::EdgeWithoutCurve`]) or a NURBS boundary edge, whose extremes have no closed
//!   form ([`DirectError::NurbsBoundary`]), is refused rather than estimated from vertices.

use core::f64::consts::{PI, TAU};
use core::fmt;

use fid_geo_nurbs::{AnalyticCurve, AnalyticSurface, Placement};

use super::types::{DirectResult, Extent, Material, NominalElement, NominalParams, UnsupportedKind};
use crate::topo::{Body, CurveParams, EdgeIx, EntityKind, Face, FaceIx, Sense, SurfaceParams, VertexIx};

/// Why an entity could not be mapped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DirectError {
    /// The index (or an entity the face refers to) is not in this body.
    UnknownEntity { kind: EntityKind, index: usize },
    /// The edge has no curve: it cannot be mapped, and a face it bounds has no computable extent.
    EdgeWithoutCurve { edge: EdgeIx },
    /// A boundary edge of an analytic plane, cylinder or cone face is NURBS; its extremes have
    /// no closed form, so the face extent is refused rather than estimated.
    NurbsBoundary { edge: EdgeIx },
    /// A local coordinate or an extreme overflowed.
    NonFinite,
}

impl DirectError {
    /// Stable variant name.
    pub fn code(&self) -> &'static str {
        match self {
            DirectError::UnknownEntity { .. } => "UnknownEntity",
            DirectError::EdgeWithoutCurve { .. } => "EdgeWithoutCurve",
            DirectError::NurbsBoundary { .. } => "NurbsBoundary",
            DirectError::NonFinite => "NonFinite",
        }
    }
}

impl fmt::Display for DirectError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DirectError::UnknownEntity { kind, index } => write!(f, "{} {index} is not in the body", kind.as_str()),
            DirectError::EdgeWithoutCurve { edge } => write!(f, "edge {} has no curve", edge.index()),
            DirectError::NurbsBoundary { edge } => {
                write!(f, "boundary edge {} is NURBS: the face extent has no closed form", edge.index())
            }
            DirectError::NonFinite => f.write_str("a local coordinate or an extreme overflowed"),
        }
    }
}

impl core::error::Error for DirectError {}

fn unknown(kind: EntityKind, index: usize) -> DirectError {
    DirectError::UnknownEntity { kind, index }
}

/// Maps one face (REQ-001, REQ-003, REQ-004, REQ-005).
pub fn map_face(body: &Body, face: FaceIx) -> Result<DirectResult, DirectError> {
    let f = body.face(face).ok_or(unknown(EntityKind::Face, face.index()))?;
    let (params, surface) = match (f.surface().params(), f.surface().as_analytic()) {
        (Some(p), Some(s)) => (*p, s),
        _ => return Ok(DirectResult::NotAnalytic),
    };
    let material = match f.sense() {
        Sense::Same => Material::Outside,
        Sense::Reversed => Material::Inside,
    };
    let (nominal, extent) = match params {
        SurfaceParams::Plane { frame } => {
            let extent = match boundary_ranges(body, f, placement_of(surface))? {
                Some(r) => Extent::Planar { u: r[0], v: r[1] },
                None => Extent::Unbounded,
            };
            (NominalParams::Plane { frame }, extent)
        }
        SurfaceParams::Cylinder { frame, radius } => {
            (NominalParams::Cylinder { frame, radius }, axial(body, f, placement_of(surface))?)
        }
        SurfaceParams::Cone { frame, radius, half_angle } => {
            (NominalParams::Cone { frame, radius, half_angle }, axial(body, f, placement_of(surface))?)
        }
        SurfaceParams::Sphere { frame, radius } => (NominalParams::Sphere { frame, radius }, Extent::NotDefined),
        SurfaceParams::Torus { frame, major_radius, minor_radius } => {
            (NominalParams::Torus { frame, major_radius, minor_radius }, Extent::NotDefined)
        }
    };
    Ok(DirectResult::Element(NominalElement::direct(nominal, extent, Some(material), f.id().clone())))
}

/// Maps one edge (REQ-002, REQ-004).
pub fn map_edge(body: &Body, edge: EdgeIx) -> Result<DirectResult, DirectError> {
    let e = body.edge(edge).ok_or(unknown(EntityKind::Edge, edge.index()))?;
    let curve = e.curve().ok_or(DirectError::EdgeWithoutCurve { edge })?;
    let params = match curve.params() {
        Some(p) => *p,
        None => return Ok(DirectResult::NotAnalytic),
    };
    let [t0, t1] = e.param_range();
    let (nominal, extent) = match params {
        CurveParams::Line { frame } => (NominalParams::Line { frame }, Extent::Axial { h_min: t0, h_max: t1 }),
        CurveParams::Circle { frame, radius } => {
            (NominalParams::Circle { frame, radius }, Extent::Angular { a0: t0, a1: t1 })
        }
        CurveParams::Ellipse { .. } => return Ok(DirectResult::UnsupportedKind { kind: UnsupportedKind::Ellipse }),
    };
    Ok(DirectResult::Element(NominalElement::direct(nominal, extent, None, e.id().clone())))
}

/// Maps one vertex to a point (REQ-002).
pub fn map_vertex(body: &Body, vertex: VertexIx) -> Result<DirectResult, DirectError> {
    let v = body.vertex(vertex).ok_or(unknown(EntityKind::Vertex, vertex.index()))?;
    let nominal = NominalParams::Point { point: v.point() };
    Ok(DirectResult::Element(NominalElement::direct(nominal, Extent::NotDefined, None, v.id().clone())))
}

fn placement_of(surface: &AnalyticSurface) -> &Placement {
    match surface {
        AnalyticSurface::Plane(s) => s.placement(),
        AnalyticSurface::Cylinder(s) => s.placement(),
        AnalyticSurface::Cone(s) => s.placement(),
        AnalyticSurface::Sphere(s) => s.placement(),
        AnalyticSurface::Torus(s) => s.placement(),
    }
}

fn axial(body: &Body, f: &Face, pl: &Placement) -> Result<Extent, DirectError> {
    Ok(match boundary_ranges(body, f, pl)? {
        Some(r) => Extent::Axial { h_min: r[2][0], h_max: r[2][1] },
        None => Extent::Unbounded,
    })
}

/// `(a0 b0 + a1 b1) + a2 b2`, one rounding per operation.
fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    let s = a[0] * b[0] + a[1] * b[1];
    s + a[2] * b[2]
}

fn num<T>(r: Result<T, fid_num::NumError>) -> Result<T, DirectError> {
    r.map_err(|_| DirectError::NonFinite)
}

/// Running `[min, max]` of each local coordinate.
struct Ranges {
    r: [Option<[f64; 2]>; 3],
}

impl Ranges {
    fn take(&mut self, k: usize, x: f64) -> Result<(), DirectError> {
        if !x.is_finite() {
            return Err(DirectError::NonFinite);
        }
        if let Some(slot) = self.r.get_mut(k) {
            *slot = Some(match *slot {
                None => [x, x],
                Some([lo, hi]) => [num(fid_num::f::total_min(lo, x))?, num(fid_num::f::total_max(hi, x))?],
            });
        }
        Ok(())
    }

    fn take3(&mut self, l: [f64; 3]) -> Result<(), DirectError> {
        for (k, x) in l.into_iter().enumerate() {
            self.take(k, x)?;
        }
        Ok(())
    }

    /// `None` when nothing was taken. Every edge's vertices are taken (all three coordinates)
    /// before its extremes, so the axes are either all empty or all set; anything else is
    /// refused rather than reported as unbounded.
    fn finish(self) -> Result<Option<[[f64; 2]; 3]>, DirectError> {
        match self.r {
            [None, None, None] => Ok(None),
            [Some(x), Some(y), Some(z)] => Ok(Some([x, y, z])),
            _ => Err(DirectError::NonFinite),
        }
    }
}

/// Whether angle `t` in `[0, TAU)`, plus some whole number of turns, lies in `[t0, t1]`.
fn in_range(t: f64, t0: f64, t1: f64) -> bool {
    if t1 - t0 >= TAU {
        return true;
    }
    let k = ((t0 - t) / TAU).ceil();
    t + k * TAU <= t1
}

/// `a` reduced to `[0, TAU)` for `a` in `[-PI, 2 PI)`.
fn wrap(a: f64) -> f64 {
    if a < 0.0 {
        a + TAU
    } else if a >= TAU {
        a - TAU
    } else {
        a
    }
}

/// Local coordinate ranges over the face boundary (module documentation), or `None` for a face
/// without loops.
fn boundary_ranges(body: &Body, f: &Face, pl: &Placement) -> Result<Option<[[f64; 2]; 3]>, DirectError> {
    let axes = [pl.ref_dir(), pl.y_dir(), pl.axis()];
    let local = |q: [f64; 3]| pl.to_local(q).map_err(|_| DirectError::NonFinite);
    let mut ranges = Ranges { r: [None; 3] };
    for &li in f.loops() {
        let lp = body.r#loop(li).ok_or(unknown(EntityKind::Loop, li.index()))?;
        for &ci in lp.coedges() {
            let co = body.coedge(ci).ok_or(unknown(EntityKind::Coedge, ci.index()))?;
            let ei = co.edge();
            let e = body.edge(ei).ok_or(unknown(EntityKind::Edge, ei.index()))?;
            for vi in [e.start(), e.end()] {
                let v = body.vertex(vi).ok_or(unknown(EntityKind::Vertex, vi.index()))?;
                ranges.take3(local(v.point())?)?;
            }
            let curve = e.curve().ok_or(DirectError::EdgeWithoutCurve { edge: ei })?;
            let (cpl, a, b) = match curve.as_analytic() {
                None => return Err(DirectError::NurbsBoundary { edge: ei }),
                Some(AnalyticCurve::Line(_)) => continue,
                Some(AnalyticCurve::Circle(c)) => (c.placement(), c.radius(), c.radius()),
                Some(AnalyticCurve::Ellipse(c)) => {
                    let (a, b) = c.semi_axes();
                    (c.placement(), a, b)
                }
            };
            let centre = local(cpl.origin())?;
            let [t0, t1] = e.param_range();
            for (k, d) in axes.into_iter().enumerate() {
                let big_a = dot(cpl.ref_dir(), d) * a;
                let big_b = dot(cpl.y_dir(), d) * b;
                let m = num(fid_num::f::hypot(big_a, big_b))?;
                if m == 0.0 {
                    continue;
                }
                let t_max = wrap(num(fid_num::f::atan2(big_b, big_a))?);
                let t_min = wrap(t_max + PI);
                let c = centre.get(k).copied().ok_or(DirectError::NonFinite)?;
                if in_range(t_max, t0, t1) {
                    ranges.take(k, c + m)?;
                }
                if in_range(t_min, t0, t1) {
                    ranges.take(k, c - m)?;
                }
            }
        }
    }
    ranges.finish()
}
