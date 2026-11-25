//! Face-set and body projection targets. Spec node `GEO.DIST.FACE.BODY`.
//!
//! [`FaceTarget`], [`FaceSetTarget`], [`BodyTarget`] and [`InstanceTarget`] implement
//! GEO.DIST.BATCH's [`ProjectionTarget`] for CAD, so [`crate::project_batch`] works on a
//! B-rep body unchanged. Each target prepares its faces once (GEO.DIST.FACE.ONE
//! [`FaceProjector`]s, GEO.DIST.FACE.INCID [`Incid`], one bounding box per face and a
//! `fid-geo-index` [`PrimitiveBvh`] over the boxes) and then answers queries without state.
//!
//! # Face bounds (REQ-004: exact geometry, never a tessellation)
//!
//! A face's box is built from its boundary and its surface's closed-form extent, then
//! padded (below). The box of a face holds every point the face projector can return.
//!
//! - **Boundary.** Each boundary edge (every edge of the face's loops) contributes its box:
//!   a line segment its two ends (evaluated at the edge range), a circle `c +- r hypot(X_k,
//!   Y_k)` per axis (the whole circle), an ellipse `c +- hypot(a X_k, b Y_k)`, a NURBS curve
//!   its control points (convex hull, weights are positive). An edge without a 3-D curve, and
//!   every imported pcurve (an imported pcurve is used unchanged by GEO.CAD.TOPO.PCURVE, so
//!   it may stray from its edge), contributes the image of its parameter rectangle (line
//!   pcurve: its two ends; NURBS pcurve: its control points) on the surface: plane the four
//!   corners; cylinder and cone the axis segment over the `v` range widened by the largest
//!   radius; sphere and torus the whole surface; NURBS the control points. Vertices add their
//!   points.
//! - **Surface.** The largest and smallest `x_k` over a compact face lie on its boundary or at
//!   an interior critical point of `x_k` on the surface (where the normal is `+-e_k`). These
//!   are added in closed form: plane none (a planar region lies in the convex hull of its
//!   boundary); cylinder, only when `|Z_k| < 2^-20`, the generator values `O_k +- r hypot(X_k,
//!   Y_k)` (otherwise there is no critical point); cone the apex (every critical generator
//!   passes through it), unless the trim classifies the apex `Outside` and no axis has
//!   `| |Z_k| - sin a | < 2^-20`; sphere `c_k +- r`, each unless the trim classifies that point
//!   `Outside`; torus the four critical points `(u, v)` with `u = atan2(Y_k, X_k)` or `+ pi` and
//!   `(cos v, sin v) = +-(+-beta, Z_k) / hypot`, each unless `Outside` (when `beta = hypot(X_k,
//!   Y_k) < 2^-20` the whole torus extent on that axis); NURBS all control points.
//!   Classification is [`classify_uv`] at the target's topology tolerance; an error keeps the
//!   point (fail safe).
//! - **Unbounded faces.** A plane, cylinder or cone face is checked to be bounded: a parameter
//!   point beyond its boundary (`2 m + 1` with `m` the largest local coordinate of the
//!   boundary box corners, plane `(far, 0)`, cylinder and cone `(0, +-far)`) must classify
//!   `Outside`. Otherwise, or when the trim cannot be built or a box cannot be computed, the
//!   face gets the **whole-space box** `[-2^250, 2^250]^3`: it is never pruned and always
//!   projected, so its own projection (or its error) decides, never a guessed bound.
//! - **Padding.** Every box is widened by [`PAD_ABS_M`] (or [`PAD_TOL_FACTOR`] times the
//!   topology tolerance, whichever is larger) plus `2^-40` of its largest coordinate
//!   magnitude, which covers the trim band of GEO.DIST.FACE.ONE (interior candidates within
//!   the topology tolerance of the boundary are kept), the 1e-8 m deviation of derived
//!   pcurves and every rounding above, also far from the origin.
//!
//! # Closest point (REQ-001 to REQ-003)
//!
//! [`PrimitiveBvh::ordered_visit`] over the face boxes (each box stored as the triangle
//! `[lo, hi, hi]`, whose box is exactly `[lo, hi]`) yields leaves in ascending lower bound;
//! inside a leaf the faces are visited in ascending (own box lower bound, face position).
//! The lower bound of a box is `sqrt(d2 (1 - 2^-48) - 2^-1022) (1 - 2^-48)`, `d2` the squared
//! distance to the box, so it never exceeds the computed distance to any point in the box.
//! A leaf or face is skipped only when its lower bound is strictly greater than
//! `min(best + 1e-15, max_distance)`, `best` the smallest distance found so far: such a face
//! can neither be the minimum nor tie with it (REQ-002). Every other face is projected with
//! [`FaceProjector::project`] (with the query's `max_distance`), and any error of such a face
//! is returned (fail closed: a face that might hold the answer and cannot be projected makes
//! the query fail, never a silently wrong answer).
//!
//! **Choice (REQ-003):** with `dmin` the smallest distance over the projected faces, the
//! answer is the face of lowest [`TopologyId`] among those with `distance <= dmin + 1e-15`.
//! This rule does not depend on the visiting order, so the pruned search returns exactly the
//! hit of the brute-force loop over every face ([`FaceSetTarget::project_brute`], AC-2). The
//! sign comes from GEO.DIST.FACE.INCID on the whole body ([`Incid::signed`]): the
//! [`HitSign::Cad`] status, and the signed distance `+d` outside, `-d` inside, else `d`.
//!
//! # Projection along a direction
//!
//! The direction `v` is scaled by exact powers of two until `1 <= max |w_k| < 2` (the line is
//! unchanged). Candidate faces are `PrimitiveBvh::ray(p, w, -2^252, 2^252)` (conservative), in
//! ascending face position. Each candidate's surface is intersected with the line in closed
//! form in the surface placement (`l = local(p)`, `m = local(w)`): plane `l_z + s m_z = 0`;
//! cylinder, sphere and cone the quadratic `a s^2 + 2 h s + c = 0` (cylinder `a = m_x^2 +
//! m_y^2`, `h = l_x m_x + l_y m_y`, `c = (rho - r)(rho + r)`; sphere `a = m.m`, `h = l.m`, `c =
//! (|l| - r)(|l| + r)`; cone with `k = tan a`, `g = r0 + k l_z`: `a = m_x^2 + m_y^2 - k^2
//! m_z^2`, `h = l_x m_x + l_y m_y - g k m_z`, `c = (rho - g)(rho + g)`, both nappes), solved
//! as `q = -(h + sign(h) sqrt(h^2 - a c))`, `s = q / a` and `c / q`. A root is kept when the
//! surface parameters of `p + s w` (GEO.NURBS.ANALYTIC inversion) classify `Inside` or
//! `OnBoundary`. A line lying in the surface (`a = h = c = 0`, or a plane with `l_z = m_z =
//! 0`) is [`BodyError::AlongDegenerate`]; a torus or NURBS candidate face is
//! [`BodyError::AlongUnsupported`] (fail closed; see NODE.md Open questions). The answer is the
//! hit of smallest `|s| |w|`; within 1e-15 m the lowest face ID, then `s >= 0`. Its distance
//! is `|s| |w|`, its signed distance `s |w|` with [`HitSign::Along`], its foot `p + s w`.
//!
//! # Instances (REQ-005)
//!
//! [`InstanceTarget`] maps the query into the definition frame with the inverse of the
//! instance-to-query transform (GEO.FRAME.XFORM), projects onto the definition's
//! [`BodyTarget`], maps the foot back with the forward transform and multiplies distances by
//! the similarity scale `s` (a direction is mapped by the rotation only, so along-distances
//! scale the same way). A maximum distance `d` is passed on as `(d / s)(1 + 2^-50)` and checked
//! again on the scaled result.
//!
//! # Determinism (D0, KNP-1, REQ-006)
//!
//! Each query is a pure function of the point, the target and the options: no state is
//! shared between queries, so thread count and batch partitioning cannot change a result.
//! Binary64 `+ - * /` one operation at a time (no FMA, no `f64::min`/`max`), `sqrt`,
//! `hypot`, `atan2`, `sin`, `tan` from `fid_num::f`, vectors and sorts with total keys, no
//! hash containers, every loop bounded. Nothing panics.

// Per-axis loops index several 3-arrays at once, as in the formulas above.
#![allow(clippy::needless_range_loop)]

use core::f64::consts::TAU;
use core::fmt;
use core::ops::ControlFlow;

use fid_geo_cad::{
    classify_uv, pcurve_disagreement, Body, Curve2, EntityIx, Face, FaceIx, InstanceView, Pcurves, TopologyId,
    TrimClass, TrimError, TrimIndex, DEFAULT_TOL_M,
};
use fid_geo_frame::{FrameError, FrameId, FrameTree, Transform, XformError};
use fid_geo_index::{BvhError, PrimitiveBvh};
use fid_geo_nurbs::{AnalyticCurve, AnalyticSurface, Curve, Placement};
use fid_geo_types::{Aabb, FrameRef};

use super::incid::{Incid, IncidError};
use super::one::{cone_nappe, FaceError, FaceHit, FaceProjection, FaceProjector, FaceQueryOptions};
use crate::batch::{HitSign, ProjectError, Projection, ProjectionHit, ProjectionOptions, ProjectionTarget};
use crate::mesh::COORD_LIMIT;
use crate::sign::SignStatus;
use crate::FACE_TIE_WINDOW_M;

type V3 = [f64; 3];
type Bx = [[f64; 3]; 2];

/// Smallest absolute padding of a face box, metres \[unverified\].
pub const PAD_ABS_M: f64 = 1e-6;
/// The absolute padding is at least this many topology tolerances.
pub const PAD_TOL_FACTOR: f64 = 64.0;
/// Relative padding of a face box: `2^-40` of its largest coordinate magnitude.
const PAD_REL: f64 = 1.0 / 1_099_511_627_776.0;
/// Shrink factor of every lower bound: `1 - 2^-48`.
const LB_SHRINK: f64 = 1.0 - 1.0 / 281_474_976_710_656.0;
/// Near-axis test of the closed-form critical points (`2^-20`).
const NEAR: f64 = 1.0 / 1_048_576.0;
/// Half-width of the whole-space box: the query coordinate limit `2^250`.
const WHOLE: f64 = COORD_LIMIT;
/// Ray parameter range of the directional candidate search (`2^252`).
const ALONG_T: f64 = 4.0 * COORD_LIMIT;

// ---------------------------------------------------------------- errors

/// Every error of this module. Errors are values; nothing panics.
#[derive(Debug, Clone, PartialEq)]
pub enum BodyError {
    /// A query coordinate is NaN or infinite.
    NonFiniteQuery,
    /// A query coordinate exceeds `2^250` in magnitude.
    QueryOutOfRange,
    /// The target has no face.
    EmptyTarget,
    /// A face index or ID is not a face of the body.
    UnknownFace,
    /// A face was named twice.
    DuplicateFace {
        /// The face.
        face: TopologyId,
    },
    /// The topology tolerance is not finite and positive.
    BadTolerance,
    /// `max_distance` is NaN, infinite or negative.
    InvalidMaxDistance,
    /// The direction is non-finite, beyond `2^250` or zero.
    InvalidDirection,
    /// The subset option was built for another face count.
    SubsetMismatch,
    /// Preparing the body's adjacency and pcurves, or signing a hit, failed.
    Incid(IncidError),
    /// A face that might hold the answer could not be projected (fail closed).
    Face {
        /// The face.
        face: TopologyId,
        /// Its error.
        error: FaceError,
    },
    /// A trim classification needed by a directional hit failed.
    Trim {
        /// The face.
        face: TopologyId,
        /// Its error.
        error: TrimError,
    },
    /// Directional projection meets a face whose surface is not intersected in closed form
    /// here (torus or NURBS).
    AlongUnsupported {
        /// The face.
        face: TopologyId,
    },
    /// The line lies in the surface of this face (a continuum of intersections).
    AlongDegenerate {
        /// The face.
        face: TopologyId,
    },
    /// A surface evaluation or inversion failed or overflowed.
    Eval {
        /// The face.
        face: TopologyId,
    },
    /// The face hierarchy refused a build or query.
    Bvh(BvhError),
    /// The frame tree could not resolve the instance transform.
    Frame(FrameError),
    /// A transform could not be inverted or applied.
    Transform(XformError),
    /// An internal invariant failed (never expected).
    Inconsistent,
}

impl BodyError {
    /// A stable machine-readable code.
    pub fn code(&self) -> &'static str {
        match self {
            BodyError::NonFiniteQuery => "non_finite_query",
            BodyError::QueryOutOfRange => "query_out_of_range",
            BodyError::EmptyTarget => "empty_target",
            BodyError::UnknownFace => "unknown_face",
            BodyError::DuplicateFace { .. } => "duplicate_face",
            BodyError::BadTolerance => "bad_tolerance",
            BodyError::InvalidMaxDistance => "invalid_max_distance",
            BodyError::InvalidDirection => "invalid_direction",
            BodyError::SubsetMismatch => "subset_mismatch",
            BodyError::Incid(_) => "incid",
            BodyError::Face { .. } => "face",
            BodyError::Trim { .. } => "trim",
            BodyError::AlongUnsupported { .. } => "along_unsupported",
            BodyError::AlongDegenerate { .. } => "along_degenerate",
            BodyError::Eval { .. } => "eval",
            BodyError::Bvh(_) => "bvh",
            BodyError::Frame(_) => "frame",
            BodyError::Transform(_) => "transform",
            BodyError::Inconsistent => "inconsistent",
        }
    }
}

impl fmt::Display for BodyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BodyError::NonFiniteQuery => f.write_str("query point is not finite"),
            BodyError::QueryOutOfRange => f.write_str("query point exceeds 2^250 m"),
            BodyError::EmptyTarget => f.write_str("the target has no face"),
            BodyError::UnknownFace => f.write_str("face is not in the body"),
            BodyError::DuplicateFace { face } => write!(f, "face {face} named twice"),
            BodyError::BadTolerance => f.write_str("topology tolerance must be finite and positive"),
            BodyError::InvalidMaxDistance => f.write_str("maximum distance must be finite and >= 0"),
            BodyError::InvalidDirection => f.write_str("direction must be finite, within 2^250 and not zero"),
            BodyError::SubsetMismatch => f.write_str("subset built for another face count"),
            BodyError::Incid(e) => write!(f, "incident normals: {e}"),
            BodyError::Face { face, error } => write!(f, "face {face}: {error}"),
            BodyError::Trim { face, error } => write!(f, "face {face}: trim: {error}"),
            BodyError::AlongUnsupported { face } => {
                write!(f, "face {face}: directional projection supports plane, cylinder, sphere and cone only")
            }
            BodyError::AlongDegenerate { face } => write!(f, "face {face}: the line lies in the surface"),
            BodyError::Eval { face } => write!(f, "face {face}: evaluation failed"),
            BodyError::Bvh(e) => write!(f, "face hierarchy: {e}"),
            BodyError::Frame(e) => write!(f, "frame: {e}"),
            BodyError::Transform(e) => write!(f, "transform: {e}"),
            BodyError::Inconsistent => f.write_str("internal invariant failed"),
        }
    }
}

impl std::error::Error for BodyError {}

impl From<BvhError> for BodyError {
    fn from(e: BvhError) -> Self {
        BodyError::Bvh(e)
    }
}

impl From<XformError> for BodyError {
    fn from(e: XformError) -> Self {
        BodyError::Transform(e)
    }
}

// ---------------------------------------------------------------- small helpers

fn finite3(a: &V3) -> bool {
    a.iter().all(|x| x.is_finite())
}

fn dot3(a: V3, b: V3) -> f64 {
    (a[0] * b[0] + a[1] * b[1]) + a[2] * b[2]
}

fn sqrt(x: f64) -> Option<f64> {
    fid_num::f::sqrt(x).ok()
}

fn hypot(a: f64, b: f64) -> Option<f64> {
    fid_num::f::hypot(a, b).ok()
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

fn check_point(p: &V3) -> Result<(), BodyError> {
    if !finite3(p) {
        return Err(BodyError::NonFiniteQuery);
    }
    if !p.iter().all(|x| (-COORD_LIMIT..=COORD_LIMIT).contains(x)) {
        return Err(BodyError::QueryOutOfRange);
    }
    Ok(())
}

fn check_max(m: Option<f64>) -> Result<(), BodyError> {
    match m {
        Some(x) if !(x.is_finite() && x >= 0.0) => Err(BodyError::InvalidMaxDistance),
        _ => Ok(()),
    }
}

/// A conservative lower bound of `sqrt(d2)` (module docs).
fn lb_of_sq(d2: f64) -> f64 {
    let s = d2 * LB_SHRINK - f64::MIN_POSITIVE;
    if s.is_nan() || s <= 0.0 {
        return 0.0;
    }
    match sqrt(s) {
        Some(r) => r * LB_SHRINK,
        None => 0.0,
    }
}

/// Lower bound of the distance from `p` to box `b`.
fn box_lower_bound(p: &V3, b: &Bx) -> f64 {
    let mut g = [0.0; 3];
    for k in 0..3 {
        if p[k] < b[0][k] {
            g[k] = b[0][k] - p[k];
        } else if p[k] > b[1][k] {
            g[k] = p[k] - b[1][k];
        }
    }
    lb_of_sq((g[0] * g[0] + g[1] * g[1]) + g[2] * g[2])
}

/// `min(best + window, max)`: faces with a larger lower bound cannot matter.
fn threshold(best: f64, max: Option<f64>) -> f64 {
    let t = best + FACE_TIE_WINDOW_M;
    match max {
        Some(m) if m < t => m,
        _ => t,
    }
}

// ---------------------------------------------------------------- face bounds

/// Per-axis intervals that only grow.
struct Acc {
    lo: V3,
    hi: V3,
    set: [bool; 3],
    ok: bool,
}

impl Acc {
    fn new() -> Acc {
        Acc { lo: [0.0; 3], hi: [0.0; 3], set: [false; 3], ok: true }
    }

    fn value(&mut self, k: usize, x: f64) {
        if !x.is_finite() {
            self.ok = false;
            return;
        }
        if !self.set[k] {
            self.lo[k] = x;
            self.hi[k] = x;
            self.set[k] = true;
        } else if x < self.lo[k] {
            self.lo[k] = x;
        } else if x > self.hi[k] {
            self.hi[k] = x;
        }
    }

    fn point(&mut self, p: V3) {
        for k in 0..3 {
            self.value(k, p[k]);
        }
    }

    fn range(&mut self, k: usize, c: f64, e: f64) {
        self.value(k, c - e);
        self.value(k, c + e);
    }

    fn complete(&self) -> bool {
        self.ok && self.set.iter().all(|&s| s)
    }
}

/// The box of one face (module docs), or `None` for the whole-space box.
fn face_box(body: &Body, face: &Face, trim: Option<&TrimIndex>, tol: f64) -> Option<Bx> {
    let mut acc = Acc::new();
    boundary(body, face, &mut acc)?;
    let surf = face.surface();
    if let Some(n) = surf.as_nurbs() {
        for &c in n.ctrl() {
            acc.point(c);
        }
    } else {
        let a = surf.as_analytic()?;
        let classify =
            |uv: [f64; 2]| -> Option<TrimClass> { trim.and_then(|t| classify_uv(t, uv[0], uv[1], tol).ok()) };
        // Kept unless the trim classifies it Outside.
        let keep = |q: V3| -> bool {
            match a.invert(q) {
                Ok(f) => classify([f.u, f.v]) != Some(TrimClass::Outside),
                Err(_) => true,
            }
        };
        match a {
            AnalyticSurface::Plane(s) => {
                bounded(&acc, s.placement(), trim, tol, false, None)?;
            }
            AnalyticSurface::Cylinder(s) => {
                let pl = s.placement();
                bounded(&acc, pl, trim, tol, true, None)?;
                let (o, x, y, z) = (pl.origin(), pl.ref_dir(), pl.y_dir(), pl.axis());
                for k in 0..3 {
                    if z[k].abs() < NEAR {
                        acc.range(k, o[k], s.radius() * hypot(x[k], y[k])?);
                    }
                }
            }
            AnalyticSurface::Cone(s) => {
                let pl = s.placement();
                let ta = fid_num::f::tan(s.half_angle()).ok()?;
                let sa = fid_num::f::sin(s.half_angle()).ok()?;
                let va = -(s.radius() / ta);
                let nappe = trim.and_then(|t| cone_nappe(t, s.radius(), s.half_angle()));
                bounded(&acc, pl, trim, tol, true, Some(nappe.unwrap_or((va, 0.0))))?;
                let (o, z) = (pl.origin(), pl.axis());
                let apex = [o[0] + va * z[0], o[1] + va * z[1], o[2] + va * z[2]];
                let critical = (0..3).any(|k| (z[k].abs() - sa).abs() < NEAR);
                if critical || classify([0.0, va]) != Some(TrimClass::Outside) {
                    acc.point(apex);
                }
            }
            AnalyticSurface::Sphere(s) => {
                let (c, r) = (s.placement().origin(), s.radius());
                for k in 0..3 {
                    for sg in [1.0, -1.0] {
                        let mut q = c;
                        q[k] = c[k] + sg * r;
                        if keep(q) {
                            acc.value(k, q[k]);
                        }
                    }
                }
            }
            AnalyticSurface::Torus(s) => {
                let pl = s.placement();
                let (big, small) = s.radii();
                let (o, x, y, z) = (pl.origin(), pl.ref_dir(), pl.y_dir(), pl.axis());
                for k in 0..3 {
                    let beta = hypot(x[k], y[k])?;
                    if beta < NEAR {
                        acc.range(k, o[k], (big + small) * beta + small * z[k].abs());
                        continue;
                    }
                    let m = hypot(beta, z[k])?;
                    // Unit radial direction towards +e_k.
                    let rho = [
                        (x[k] * x[0] + y[k] * y[0]) / beta,
                        (x[k] * x[1] + y[k] * y[1]) / beta,
                        (x[k] * x[2] + y[k] * y[2]) / beta,
                    ];
                    for sg in [1.0, -1.0] {
                        for tau in [1.0, -1.0] {
                            let cv = tau * (sg * beta) / m;
                            let sv = tau * z[k] / m;
                            let w = big + small * cv;
                            let q = [
                                (o[0] + w * (sg * rho[0])) + (small * sv) * z[0],
                                (o[1] + w * (sg * rho[1])) + (small * sv) * z[1],
                                (o[2] + w * (sg * rho[2])) + (small * sv) * z[2],
                            ];
                            if keep(q) {
                                acc.value(k, q[k]);
                            }
                        }
                    }
                }
            }
        }
    }
    if !acc.complete() {
        return None;
    }
    let pad_abs = {
        let t = PAD_TOL_FACTOR * tol;
        if t > PAD_ABS_M {
            t
        } else {
            PAD_ABS_M
        }
    };
    let mut mag = 0.0;
    for k in 0..3 {
        for v in [acc.lo[k].abs(), acc.hi[k].abs()] {
            if v > mag {
                mag = v;
            }
        }
    }
    let pad = pad_abs + PAD_REL * mag;
    let mut b = [[0.0; 3]; 2];
    for k in 0..3 {
        b[0][k] = acc.lo[k] - pad;
        b[1][k] = acc.hi[k] + pad;
        if !b[0][k].is_finite() || !b[1][k].is_finite() || b[0][k] < -WHOLE || b[1][k] > WHOLE {
            return None;
        }
    }
    Some(b)
}

/// The boundary contributions: every edge of the face's loops, its vertices, and the surface
/// image of every imported pcurve. `None` when an edge has no usable geometry.
fn boundary(body: &Body, face: &Face, acc: &mut Acc) -> Option<()> {
    let surf = face.surface();
    let mut seen: Vec<usize> = Vec::new();
    for &lx in face.loops() {
        let lp = body.r#loop(lx)?;
        for &cx in lp.coedges() {
            let co = body.coedge(cx)?;
            let ex = co.edge();
            let e = body.edge(ex)?;
            let range = e.param_range();
            if let Some(pc) = co.pcurve() {
                // An imported pcurve may stray from its edge unless measured close to it.
                let close = pcurve_disagreement(body, cx).is_ok_and(|d| d.is_finite() && d <= PAD_ABS_M * 0.25);
                if !close || e.curve().is_none() {
                    pcurve_image(surf.as_analytic(), surf.as_nurbs().map(|n| n.ctrl()), pc, range, acc)?;
                }
            } else if e.curve().is_none() {
                return None;
            }
            if seen.contains(&ex.index()) {
                continue;
            }
            seen.push(ex.index());
            for v in [e.start(), e.end()] {
                acc.point(body.vertex(v)?.point());
            }
            let Some(cd) = e.curve() else { continue };
            match (cd.as_analytic(), cd.as_nurbs()) {
                (Some(AnalyticCurve::Line(l)), _) => {
                    for t in range {
                        acc.point(l.eval(t, 0).ok()?.p);
                    }
                }
                (Some(AnalyticCurve::Circle(c)), _) => {
                    let pl = c.placement();
                    let (o, x, y) = (pl.origin(), pl.ref_dir(), pl.y_dir());
                    for k in 0..3 {
                        acc.range(k, o[k], c.radius() * hypot(x[k], y[k])?);
                    }
                }
                (Some(AnalyticCurve::Ellipse(el)), _) => {
                    let pl = el.placement();
                    let (o, x, y) = (pl.origin(), pl.ref_dir(), pl.y_dir());
                    let (a, b) = el.semi_axes();
                    for k in 0..3 {
                        acc.range(k, o[k], hypot(a * x[k], b * y[k])?);
                    }
                }
                (None, Some(n)) => {
                    for &c in n.ctrl() {
                        acc.point(c);
                    }
                }
                (None, None) => return None,
            }
        }
    }
    Some(())
}

/// The surface image of a pcurve's parameter rectangle (module docs).
fn pcurve_image(
    analytic: Option<&AnalyticSurface>,
    nurbs_ctrl: Option<&[[f64; 3]]>,
    pc: &Curve2,
    range: [f64; 2],
    acc: &mut Acc,
) -> Option<()> {
    if let Some(ctrl) = nurbs_ctrl {
        for &c in ctrl {
            acc.point(c);
        }
        return Some(());
    }
    // uv rectangle.
    let mut uv: Vec<[f64; 2]> = Vec::new();
    if let Some((o, d)) = pc.as_line() {
        for t in range {
            uv.push([o[0] + t * d[0], o[1] + t * d[1]]);
        }
    } else if let Some(n) = pc.as_nurbs() {
        for c in n.ctrl() {
            uv.push([c[0], c[1]]);
        }
    } else {
        return None;
    }
    let first = uv.first()?;
    let (mut lo, mut hi) = (*first, *first);
    for q in &uv {
        for i in 0..2 {
            if !q[i].is_finite() {
                return None;
            }
            if q[i] < lo[i] {
                lo[i] = q[i];
            }
            if q[i] > hi[i] {
                hi[i] = q[i];
            }
        }
    }
    match analytic? {
        AnalyticSurface::Plane(s) => {
            let pl = s.placement();
            let (o, x, y) = (pl.origin(), pl.ref_dir(), pl.y_dir());
            for u in [lo[0], hi[0]] {
                for v in [lo[1], hi[1]] {
                    acc.point(core::array::from_fn(|k| (o[k] + u * x[k]) + v * y[k]));
                }
            }
        }
        AnalyticSurface::Cylinder(s) => {
            let pl = s.placement();
            axis_segment(pl, lo[1], hi[1], s.radius(), acc)?;
        }
        AnalyticSurface::Cone(s) => {
            let pl = s.placement();
            let ta = fid_num::f::tan(s.half_angle()).ok()?;
            let r0 = (s.radius() + lo[1] * ta).abs();
            let r1 = (s.radius() + hi[1] * ta).abs();
            axis_segment(pl, lo[1], hi[1], if r1 > r0 { r1 } else { r0 }, acc)?;
        }
        AnalyticSurface::Sphere(s) => {
            let c = s.placement().origin();
            for k in 0..3 {
                acc.range(k, c[k], s.radius());
            }
        }
        AnalyticSurface::Torus(s) => {
            let pl = s.placement();
            let (big, small) = s.radii();
            let (o, x, y, z) = (pl.origin(), pl.ref_dir(), pl.y_dir(), pl.axis());
            for k in 0..3 {
                acc.range(k, o[k], (big + small) * hypot(x[k], y[k])? + small * z[k].abs());
            }
        }
    }
    Some(())
}

/// The box of the axis segment `O + v Z`, `v` in `[v0, v1]`, widened by radius `r`.
fn axis_segment(pl: &Placement, v0: f64, v1: f64, r: f64, acc: &mut Acc) -> Option<()> {
    let (o, x, y, z) = (pl.origin(), pl.ref_dir(), pl.y_dir(), pl.axis());
    for k in 0..3 {
        let e = r * hypot(x[k], y[k])?;
        for v in [v0, v1] {
            acc.range(k, o[k] + v * z[k], e);
        }
    }
    Some(())
}

/// Checks that a plane (`axial = false`) or cylinder or cone face is bounded: a parameter
/// point beyond its boundary must classify `Outside` (module docs). `None` otherwise. For a
/// cone, `nappe` is `(v_apex, side)` with side `+-1` for a face on one nappe (only that end
/// is probed) or `0` (both ends).
fn bounded(
    acc: &Acc,
    pl: &Placement,
    trim: Option<&TrimIndex>,
    tol: f64,
    axial: bool,
    nappe: Option<(f64, f64)>,
) -> Option<()> {
    let trim = trim?;
    if !acc.complete() {
        return None;
    }
    let mut m = 0.0;
    for c in 0..8 {
        let q = [
            if c & 1 == 0 { acc.lo[0] } else { acc.hi[0] },
            if c & 2 == 0 { acc.lo[1] } else { acc.hi[1] },
            if c & 4 == 0 { acc.lo[2] } else { acc.hi[2] },
        ];
        let l = pl.to_local(q).ok()?;
        let vals: &[f64] = if axial { &l[2..3] } else { &l[0..2] };
        for v in vals {
            if v.abs() > m {
                m = v.abs();
            }
        }
    }
    if let Some((e, _)) = nappe {
        if e.abs() > m {
            m = e.abs();
        }
    }
    let far = 2.0 * m + 1.0;
    if !far.is_finite() {
        return None;
    }
    let probes: Vec<[f64; 2]> = match (axial, nappe) {
        (false, _) => vec![[far, 0.0]],
        // A cone face on one nappe: only that nappe's far end (the other is dropped by the
        // nappe rule of GEO.DIST.FACE.ONE).
        (true, Some((_, side))) if side != 0.0 => vec![[0.0, side * far]],
        (true, _) => vec![[0.0, far], [0.0, -far]],
    };
    for q in &probes {
        if classify_uv(trim, q[0], q[1], tol).ok()? != TrimClass::Outside {
            return None;
        }
    }
    Some(())
}

/// The boxes of a target's faces, in face-position order (Design: `FaceBounds`).
#[derive(Debug, Clone, PartialEq)]
pub struct FaceBounds {
    boxes: Vec<Aabb>,
    whole: Vec<bool>,
}

impl FaceBounds {
    /// The boxes, one per face of the target in ascending face index.
    pub fn boxes(&self) -> &[Aabb] {
        &self.boxes
    }

    /// Per face: true when the face got the whole-space box (its bound could not be
    /// certified; it is never pruned).
    pub fn whole(&self) -> &[bool] {
        &self.whole
    }
}

// ---------------------------------------------------------------- the target

/// One face of a target.
struct Member<'b> {
    ix: FaceIx,
    id: TopologyId,
    face: &'b Face,
    projector: Result<FaceProjector<'b>, FaceError>,
    trim: Result<TrimIndex, TrimError>,
    /// Cone face on one nappe: `(v_apex, side)`.
    nappe: Option<(f64, f64)>,
    bx: Bx,
}

/// A set of faces of one body, prepared for projection (Design: `FaceSetTarget`).
pub struct FaceSetTarget<'b> {
    body: &'b Body,
    frame: FrameRef,
    tol: f64,
    incid: Incid<'b>,
    members: Vec<Member<'b>>,
    bounds: FaceBounds,
    bvh: PrimitiveBvh,
}

impl fmt::Debug for FaceSetTarget<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FaceSetTarget")
            .field("frame", &self.frame)
            .field("tol", &self.tol)
            .field("faces", &self.members.iter().map(|m| &m.id).collect::<Vec<_>>())
            .finish()
    }
}

fn face_ix_of(body: &Body, id: &TopologyId) -> Result<FaceIx, BodyError> {
    match body.lookup(id) {
        Some(EntityIx::Face(f)) => Ok(f),
        _ => Err(BodyError::UnknownFace),
    }
}

fn all_faces(body: &Body) -> Result<Vec<FaceIx>, BodyError> {
    body.faces().iter().map(|f| face_ix_of(body, f.id())).collect()
}

/// The faces of a body that matter for one query: hits by face position.
type Hits = Vec<(usize, FaceHit)>;

impl<'b> FaceSetTarget<'b> {
    /// The faces `faces` of `body` (any order, no repeats), whose coordinates are in `frame`,
    /// at the GEO.CAD.TOPO default tolerance.
    pub fn new(body: &'b Body, frame: FrameRef, faces: &[FaceIx]) -> Result<FaceSetTarget<'b>, BodyError> {
        FaceSetTarget::with_tolerance(body, frame, faces, DEFAULT_TOL_M)
    }

    /// [`FaceSetTarget::new`] with the faces given by topology ID.
    pub fn from_ids(body: &'b Body, frame: FrameRef, ids: &[TopologyId]) -> Result<FaceSetTarget<'b>, BodyError> {
        let faces: Vec<FaceIx> = ids.iter().map(|id| face_ix_of(body, id)).collect::<Result<_, _>>()?;
        FaceSetTarget::new(body, frame, &faces)
    }

    /// [`FaceSetTarget::new`] with the topology tolerance `topo_tol_m` (the trim band of
    /// GEO.DIST.FACE.ONE).
    pub fn with_tolerance(
        body: &'b Body,
        frame: FrameRef,
        faces: &[FaceIx],
        topo_tol_m: f64,
    ) -> Result<FaceSetTarget<'b>, BodyError> {
        if !(topo_tol_m.is_finite() && topo_tol_m > 0.0) {
            return Err(BodyError::BadTolerance);
        }
        if faces.is_empty() {
            return Err(BodyError::EmptyTarget);
        }
        let mut list: Vec<FaceIx> = faces.to_vec();
        list.sort();
        for w in list.windows(2) {
            if w[0] == w[1] {
                let id = body.face(w[0]).ok_or(BodyError::UnknownFace)?.id().clone();
                return Err(BodyError::DuplicateFace { face: id });
            }
        }
        let incid = Incid::new(body).map_err(BodyError::Incid)?;
        let mut members: Vec<Member<'b>> = Vec::new();
        for &ix in &list {
            let face = body.face(ix).ok_or(BodyError::UnknownFace)?;
            let pcurves: &Pcurves = incid.pcurves();
            let projector = FaceProjector::new(body, pcurves, ix);
            let trim = TrimIndex::build(body, pcurves, ix);
            let bx = face_box(body, face, trim.as_ref().ok(), topo_tol_m)
                .unwrap_or([[-WHOLE, -WHOLE, -WHOLE], [WHOLE, WHOLE, WHOLE]]);
            let nappe = match (face.surface().as_analytic(), trim.as_ref()) {
                (Some(AnalyticSurface::Cone(c)), Ok(t)) => cone_nappe(t, c.radius(), c.half_angle()),
                _ => None,
            };
            members.push(Member { ix, id: face.id().clone(), face, projector, trim, nappe, bx });
        }
        let mut boxes = Vec::with_capacity(members.len());
        let mut whole = Vec::with_capacity(members.len());
        let mut tris: Vec<[[f64; 3]; 3]> = Vec::with_capacity(members.len());
        let mut payload: Vec<u64> = Vec::with_capacity(members.len());
        for (k, m) in members.iter().enumerate() {
            boxes.push(Aabb::new(m.bx[0], m.bx[1]).map_err(|_| BodyError::Inconsistent)?);
            whole.push(m.bx[0][0] == -WHOLE && m.bx[1][0] == WHOLE);
            tris.push([m.bx[0], m.bx[1], m.bx[1]]);
            payload.push(k as u64);
        }
        let bvh = PrimitiveBvh::build_triangles(frame, &tris, &payload, 1)?;
        Ok(FaceSetTarget { body, frame, tol: topo_tol_m, incid, members, bounds: FaceBounds { boxes, whole }, bvh })
    }

    /// The body.
    pub fn body(&self) -> &'b Body {
        self.body
    }

    /// The faces of the target, ascending face index.
    pub fn faces(&self) -> Vec<FaceIx> {
        self.members.iter().map(|m| m.ix).collect()
    }

    /// The face boxes, in the order of [`Self::faces`].
    pub fn bounds(&self) -> &FaceBounds {
        &self.bounds
    }

    /// The topology tolerance.
    pub fn tolerance(&self) -> f64 {
        self.tol
    }

    fn allowed(&self, m: usize, opts: &ProjectionOptions<'_, '_>) -> bool {
        match (opts.subset, self.members.get(m)) {
            (Some(s), Some(mm)) => s.contains(mm.ix.index() as u64),
            (None, Some(_)) => true,
            (_, None) => false,
        }
    }

    fn check_subset(&self, opts: &ProjectionOptions<'_, '_>) -> Result<(), BodyError> {
        match opts.subset {
            Some(s) if s.triangle_count() != self.body.faces().len() as u64 => Err(BodyError::SubsetMismatch),
            _ => Ok(()),
        }
    }

    /// Projects face position `m`; a hit lowers `best`.
    fn eval(&self, m: usize, p: V3, fopts: FaceQueryOptions, hits: &mut Hits, best: &mut f64) -> Result<(), BodyError> {
        let mm = self.members.get(m).ok_or(BodyError::Inconsistent)?;
        let pr = mm.projector.as_ref().map_err(|e| BodyError::Face { face: mm.id.clone(), error: e.clone() })?;
        match pr.project(p, fopts).map_err(|e| BodyError::Face { face: mm.id.clone(), error: e })? {
            FaceProjection::Hit(h) => {
                if h.distance < *best {
                    *best = h.distance;
                }
                hits.push((m, h));
            }
            FaceProjection::NoHit => {}
        }
        Ok(())
    }

    /// The REQ-003 choice (module docs).
    fn choose(&self, hits: Hits) -> Option<(usize, FaceHit)> {
        let mut dmin = f64::INFINITY;
        for (_, h) in &hits {
            if h.distance < dmin {
                dmin = h.distance;
            }
        }
        let lim = dmin + FACE_TIE_WINDOW_M;
        let mut win: Option<(usize, FaceHit)> = None;
        for (m, h) in hits {
            if h.distance > lim {
                continue;
            }
            let better = match &win {
                None => true,
                Some((w, _)) => match (self.members.get(m), self.members.get(*w)) {
                    (Some(a), Some(b)) => a.id < b.id,
                    _ => false,
                },
            };
            if better {
                win = Some((m, h));
            }
        }
        win
    }

    fn search(
        &self,
        p: V3,
        opts: &ProjectionOptions<'_, '_>,
        brute: bool,
    ) -> Result<Option<(usize, FaceHit)>, BodyError> {
        check_point(&p)?;
        check_max(opts.max_distance)?;
        self.check_subset(opts)?;
        let max = opts.max_distance;
        let fopts = FaceQueryOptions { max_distance: max, topo_tol_m: self.tol };
        let mut hits: Hits = Vec::new();
        let mut best = f64::INFINITY;
        if brute {
            for m in 0..self.members.len() {
                if self.allowed(m, opts) {
                    self.eval(m, p, fopts, &mut hits, &mut best)?;
                }
            }
        } else {
            let mut fail: Option<BodyError> = None;
            let mut order: Vec<(f64, usize)> = Vec::new();
            self.bvh.ordered_visit(p, |leaf| {
                if lb_of_sq(leaf.lower_bound_sq()) > threshold(best, max) {
                    return ControlFlow::Break(());
                }
                order.clear();
                for t in leaf.triangles() {
                    let m = t as usize;
                    if !self.allowed(m, opts) {
                        continue;
                    }
                    let Some(mm) = self.members.get(m) else {
                        fail = Some(BodyError::Inconsistent);
                        return ControlFlow::Break(());
                    };
                    order.push((box_lower_bound(&p, &mm.bx), m));
                }
                order.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
                for &(lb, m) in &order {
                    if lb > threshold(best, max) {
                        continue;
                    }
                    if let Err(e) = self.eval(m, p, fopts, &mut hits, &mut best) {
                        fail = Some(e);
                        return ControlFlow::Break(());
                    }
                }
                ControlFlow::Continue(())
            })?;
            if let Some(e) = fail {
                return Err(e);
            }
        }
        Ok(self.choose(hits))
    }

    /// The closest point of the target to `p` without its sign: the face index and the
    /// GEO.DIST.FACE.ONE hit, or `None` (nothing within `max_distance`, or no allowed face).
    pub fn closest(&self, p: V3, opts: &ProjectionOptions<'_, '_>) -> Result<Option<(FaceIx, FaceHit)>, BodyError> {
        Ok(self.search(p, opts, false)?.and_then(|(m, h)| self.members.get(m).map(|mm| (mm.ix, h))))
    }

    /// [`Self::closest`] by projecting every allowed face, without the hierarchy (the
    /// reference of AC-2).
    pub fn closest_brute(
        &self,
        p: V3,
        opts: &ProjectionOptions<'_, '_>,
    ) -> Result<Option<(FaceIx, FaceHit)>, BodyError> {
        Ok(self.search(p, opts, true)?.and_then(|(m, h)| self.members.get(m).map(|mm| (mm.ix, h))))
    }

    fn signed_hit(&self, p: V3, m: usize, hit: FaceHit) -> Result<Projection, BodyError> {
        let mm = self.members.get(m).ok_or(BodyError::Inconsistent)?;
        let s = self.incid.signed(p, &hit).map_err(BodyError::Incid)?;
        let d = hit.distance;
        let signed = match s.status {
            SignStatus::Outside => d,
            SignStatus::Inside => -d,
            _ => d,
        };
        Ok(Projection::Hit(ProjectionHit {
            foot: hit.foot,
            distance: d,
            signed_distance: signed,
            sign: HitSign::Cad(s.status),
            entity_id: mm.ix.index() as u64,
            params: Some((hit.uv[0], hit.uv[1])),
        }))
    }

    fn project_impl(&self, p: V3, opts: &ProjectionOptions<'_, '_>, brute: bool) -> Result<Projection, BodyError> {
        match self.search(p, opts, brute)? {
            None => Ok(Projection::NoHit),
            Some((m, hit)) => self.signed_hit(p, m, hit),
        }
    }

    /// [`ProjectionTarget::project`] by projecting every allowed face (the reference of AC-2).
    pub fn project_brute(&self, p: V3, opts: &ProjectionOptions<'_, '_>) -> Result<Projection, ProjectError> {
        self.project_impl(p, opts, true).map_err(ProjectError::Body)
    }

    /// [`ProjectionTarget::project_along`] testing every allowed face, without the hierarchy.
    pub fn project_along_brute(
        &self,
        p: V3,
        v: V3,
        opts: &ProjectionOptions<'_, '_>,
    ) -> Result<Projection, ProjectError> {
        self.along(p, v, opts, true).map_err(ProjectError::Body)
    }

    // ------------------------------------------------------------ along a direction

    fn along(&self, p: V3, v: V3, opts: &ProjectionOptions<'_, '_>, brute: bool) -> Result<Projection, BodyError> {
        check_point(&p)?;
        let ok =
            finite3(&v) && v.iter().all(|x| (-COORD_LIMIT..=COORD_LIMIT).contains(x)) && v.iter().any(|&x| x != 0.0);
        if !ok {
            return Err(BodyError::InvalidDirection);
        }
        check_max(opts.max_distance)?;
        self.check_subset(opts)?;
        let w = normalise_pow2(&v);
        let wlen = sqrt(dot3(w, w)).ok_or(BodyError::Inconsistent)?;
        let cands: Vec<usize> = if brute {
            (0..self.members.len()).collect()
        } else {
            self.bvh.ray(p, w, -ALONG_T, ALONG_T)?.into_iter().map(|t| t as usize).collect()
        };
        // (distance, member, s, foot, uv)
        let mut hits: Vec<(f64, usize, f64, V3, [f64; 2])> = Vec::new();
        for m in cands {
            if !self.allowed(m, opts) {
                continue;
            }
            let mm = self.members.get(m).ok_or(BodyError::Inconsistent)?;
            for (s, x, uv) in self.line_face(mm, p, w)? {
                hits.push((s.abs() * wlen, m, s, x, uv));
            }
        }
        let mut dmin = f64::INFINITY;
        for h in &hits {
            if h.0 < dmin {
                dmin = h.0;
            }
        }
        let lim = dmin + FACE_TIE_WINDOW_M;
        let mut win: Option<usize> = None;
        for (i, h) in hits.iter().enumerate() {
            if h.0 > lim {
                continue;
            }
            let better = match win.and_then(|j| hits.get(j)) {
                None => true,
                Some(b) => match (self.members.get(h.1), self.members.get(b.1)) {
                    (Some(x), Some(y)) if x.id != y.id => x.id < y.id,
                    _ => (h.2 >= 0.0 && b.2 < 0.0) || ((h.2 >= 0.0) == (b.2 >= 0.0) && h.0 < b.0),
                },
            };
            if better {
                win = Some(i);
            }
        }
        let Some(&(distance, m, s, foot, uv)) = win.and_then(|j| hits.get(j)) else {
            return Ok(Projection::NoHit);
        };
        if opts.max_distance.is_some_and(|d| distance > d) {
            return Ok(Projection::NoHit);
        }
        let mm = self.members.get(m).ok_or(BodyError::Inconsistent)?;
        Ok(Projection::Hit(ProjectionHit {
            foot,
            distance,
            signed_distance: s * wlen,
            sign: HitSign::Along,
            entity_id: mm.ix.index() as u64,
            params: Some((uv[0], uv[1])),
        }))
    }

    /// The intersections `(s, p + s w, uv)` of the line with one face (module docs).
    fn line_face(&self, mm: &Member<'_>, p: V3, w: V3) -> Result<Vec<(f64, V3, [f64; 2])>, BodyError> {
        let id = || mm.id.clone();
        let Some(a) = mm.face.surface().as_analytic() else {
            return Err(BodyError::AlongUnsupported { face: id() });
        };
        let eval = || BodyError::Eval { face: id() };
        let pl = match a {
            AnalyticSurface::Plane(s) => s.placement(),
            AnalyticSurface::Cylinder(s) => s.placement(),
            AnalyticSurface::Cone(s) => s.placement(),
            AnalyticSurface::Sphere(s) => s.placement(),
            AnalyticSurface::Torus(_) => return Err(BodyError::AlongUnsupported { face: id() }),
        };
        let l = pl.to_local(p).map_err(|_| eval())?;
        let n = [dot3(w, pl.ref_dir()), dot3(w, pl.y_dir()), dot3(w, pl.axis())];
        let roots: Vec<f64> = match a {
            AnalyticSurface::Plane(_) => {
                if n[2] == 0.0 {
                    if l[2] == 0.0 {
                        return Err(BodyError::AlongDegenerate { face: id() });
                    }
                    Vec::new()
                } else {
                    vec![-(l[2] / n[2])]
                }
            }
            AnalyticSurface::Cylinder(s) => {
                let rho = hypot(l[0], l[1]).ok_or_else(eval)?;
                let r = s.radius();
                let qa = n[0] * n[0] + n[1] * n[1];
                let qh = l[0] * n[0] + l[1] * n[1];
                let qc = (rho - r) * (rho + r);
                quadratic(qa, qh, qc).ok_or_else(|| BodyError::AlongDegenerate { face: id() })?
            }
            AnalyticSurface::Sphere(s) => {
                let dn = hypot(hypot(l[0], l[1]).ok_or_else(eval)?, l[2]).ok_or_else(eval)?;
                let r = s.radius();
                let qa = dot3(n, n);
                let qh = dot3(l, n);
                let qc = (dn - r) * (dn + r);
                quadratic(qa, qh, qc).ok_or_else(|| BodyError::AlongDegenerate { face: id() })?
            }
            AnalyticSurface::Cone(s) => {
                let k = fid_num::f::tan(s.half_angle()).map_err(|_| eval())?;
                let g = s.radius() + k * l[2];
                let rho = hypot(l[0], l[1]).ok_or_else(eval)?;
                let qa = (n[0] * n[0] + n[1] * n[1]) - (k * k) * (n[2] * n[2]);
                let qh = (l[0] * n[0] + l[1] * n[1]) - (g * k) * n[2];
                let qc = (rho - g) * (rho + g);
                quadratic(qa, qh, qc).ok_or_else(|| BodyError::AlongDegenerate { face: id() })?
            }
            AnalyticSurface::Torus(_) => return Err(BodyError::AlongUnsupported { face: id() }),
        };
        let trim = mm.trim.as_ref().map_err(|e| BodyError::Trim { face: id(), error: *e })?;
        let mut out: Vec<(f64, V3, [f64; 2])> = Vec::new();
        for s in roots {
            if !s.is_finite() || out.iter().any(|o| o.0 == s) {
                continue;
            }
            let x = [p[0] + s * w[0], p[1] + s * w[1], p[2] + s * w[2]];
            if !finite3(&x) {
                continue;
            }
            let f = a.invert(x).map_err(|_| eval())?;
            let uv = [wrap_param(a, f.u), f.v];
            if let Some((va, side)) = mm.nappe {
                if (uv[1] - va) * side < -self.tol {
                    continue;
                }
            }
            match classify_uv(trim, uv[0], uv[1], self.tol) {
                Ok(TrimClass::Inside) | Ok(TrimClass::OnBoundary) => out.push((s, x, uv)),
                Ok(TrimClass::Outside) => {}
                Err(e) => return Err(BodyError::Trim { face: id(), error: e }),
            }
        }
        Ok(out)
    }
}

/// `u` of a periodic analytic parameter in `[0, 2 pi)` (inversion already returns that;
/// kept explicit).
fn wrap_param(a: &AnalyticSurface, u: f64) -> f64 {
    match a {
        AnalyticSurface::Plane(_) => u,
        _ => wrap(u),
    }
}

/// The real roots of `a s^2 + 2 h s + c = 0` in a stable form, or `None` when every `s` is a
/// root (`a = h = c = 0`).
fn quadratic(a: f64, h: f64, c: f64) -> Option<Vec<f64>> {
    if a == 0.0 {
        if h == 0.0 {
            return if c == 0.0 { None } else { Some(Vec::new()) };
        }
        return Some(vec![-(c / (2.0 * h))]);
    }
    let disc = h * h - a * c;
    if disc.is_nan() || disc < 0.0 {
        return Some(Vec::new());
    }
    let sq = sqrt(disc)?;
    let q = if h >= 0.0 { -(h + sq) } else { -(h - sq) };
    if q == 0.0 {
        // h = 0 and disc = 0, so c = 0: the double root 0.
        return Some(vec![0.0]);
    }
    let (s1, s2) = (q / a, c / q);
    Some(if s1 == s2 { vec![s1] } else { vec![s1, s2] })
}

/// `v` scaled by exact powers of two until `1 <= max |w_k| < 2`.
fn normalise_pow2(v: &V3) -> V3 {
    let mut m = 0.0;
    for x in v {
        if x.abs() > m {
            m = x.abs();
        }
    }
    let mut w = *v;
    // Each loop is bounded by the exponent range (at most about 1100 steps in total).
    let mut steps = 0;
    while m >= 2.0 && steps < 2200 {
        w = [w[0] * 0.5, w[1] * 0.5, w[2] * 0.5];
        m *= 0.5;
        steps += 1;
    }
    while m > 0.0 && m < 1.0 && steps < 2200 {
        w = [w[0] * 2.0, w[1] * 2.0, w[2] * 2.0];
        m *= 2.0;
        steps += 1;
    }
    w
}

impl ProjectionTarget for FaceSetTarget<'_> {
    fn frame(&self) -> FrameRef {
        self.frame
    }

    fn check_options(&self, opts: &ProjectionOptions<'_, '_>) -> Result<(), ProjectError> {
        self.check_subset(opts).map_err(ProjectError::Body)
    }

    fn project(&self, p: V3, opts: &ProjectionOptions<'_, '_>) -> Result<Projection, ProjectError> {
        self.project_impl(p, opts, false).map_err(ProjectError::Body)
    }

    fn project_along(&self, p: V3, v: V3, opts: &ProjectionOptions<'_, '_>) -> Result<Projection, ProjectError> {
        self.along(p, v, opts, false).map_err(ProjectError::Body)
    }
}

// ---------------------------------------------------------------- face and body targets

macro_rules! wrapper {
    ($(#[$m:meta])* $name:ident) => {
        $(#[$m])*
        #[derive(Debug)]
        pub struct $name<'b>(FaceSetTarget<'b>);

        impl<'b> $name<'b> {
            /// The underlying face set.
            pub fn set(&self) -> &FaceSetTarget<'b> {
                &self.0
            }
        }

        impl ProjectionTarget for $name<'_> {
            fn frame(&self) -> FrameRef {
                self.0.frame
            }

            fn check_options(&self, opts: &ProjectionOptions<'_, '_>) -> Result<(), ProjectError> {
                self.0.check_options(opts)
            }

            fn project(&self, p: V3, opts: &ProjectionOptions<'_, '_>) -> Result<Projection, ProjectError> {
                self.0.project(p, opts)
            }

            fn project_along(
                &self,
                p: V3,
                v: V3,
                opts: &ProjectionOptions<'_, '_>,
            ) -> Result<Projection, ProjectError> {
                self.0.project_along(p, v, opts)
            }
        }
    };
}

wrapper!(
    /// One face of a body (Design: `FaceTarget`). The sign still uses the whole body's
    /// topology at the foot.
    FaceTarget
);

wrapper!(
    /// Every face of a body (Design: `BodyTarget`).
    BodyTarget
);

impl<'b> FaceTarget<'b> {
    /// Face `face` of `body`, in `frame`, at the default tolerance.
    pub fn new(body: &'b Body, frame: FrameRef, face: FaceIx) -> Result<FaceTarget<'b>, BodyError> {
        Ok(FaceTarget(FaceSetTarget::new(body, frame, &[face])?))
    }
}

impl<'b> BodyTarget<'b> {
    /// Every face of `body`, in `frame`, at the default tolerance.
    pub fn new(body: &'b Body, frame: FrameRef) -> Result<BodyTarget<'b>, BodyError> {
        BodyTarget::with_tolerance(body, frame, DEFAULT_TOL_M)
    }

    /// [`BodyTarget::new`] with topology tolerance `topo_tol_m`.
    pub fn with_tolerance(body: &'b Body, frame: FrameRef, topo_tol_m: f64) -> Result<BodyTarget<'b>, BodyError> {
        Ok(BodyTarget(FaceSetTarget::with_tolerance(body, frame, &all_faces(body)?, topo_tol_m)?))
    }
}

// ---------------------------------------------------------------- instances

/// A placed instance of a definition body (Design: `InstanceTarget`, REQ-005).
#[derive(Debug)]
pub struct InstanceTarget<'b> {
    inner: BodyTarget<'b>,
    to_query: Transform,
    to_def: Transform,
    frame: FrameRef,
}

impl<'b> InstanceTarget<'b> {
    /// The instance of `view`, queried in frame `query_frame` of `tree`: the definition's
    /// coordinates are in the instance frame, mapped by `tree.resolve(instance frame,
    /// query_frame)`.
    pub fn new(
        view: &InstanceView<'b>,
        tree: &FrameTree,
        query_frame: FrameId,
    ) -> Result<InstanceTarget<'b>, BodyError> {
        let to_query = tree.resolve(view.frame(), query_frame).map_err(BodyError::Frame)?;
        let inner = BodyTarget::new(view.definition(), FrameRef::from_bytes(view.frame().to_bytes()))?;
        InstanceTarget::from_transform(inner, to_query, FrameRef::from_bytes(query_frame.to_bytes()))
    }

    /// A definition target placed by `to_query` (definition coordinates to query
    /// coordinates), answering in `frame`.
    pub fn from_transform(
        inner: BodyTarget<'b>,
        to_query: Transform,
        frame: FrameRef,
    ) -> Result<InstanceTarget<'b>, BodyError> {
        let to_def = to_query.inverse()?;
        Ok(InstanceTarget { inner, to_query, to_def, frame })
    }

    /// The definition target.
    pub fn definition(&self) -> &BodyTarget<'b> {
        &self.inner
    }

    /// The definition-to-query transform.
    pub fn transform(&self) -> &Transform {
        &self.to_query
    }

    fn def_opts<'s, 'a>(&self, opts: &ProjectionOptions<'s, 'a>) -> Result<ProjectionOptions<'s, 'a>, BodyError> {
        check_max(opts.max_distance)?;
        let s = self.to_query.scale();
        let max_distance = match opts.max_distance {
            Some(d) => {
                let m = (d / s) * (1.0 + 1.0 / 1_125_899_906_842_624.0);
                Some(if m.is_finite() { m } else { f64::MAX })
            }
            None => None,
        };
        Ok(ProjectionOptions { max_distance, subset: opts.subset, direction: None })
    }

    fn back(&self, r: Projection, opts: &ProjectionOptions<'_, '_>) -> Result<Projection, BodyError> {
        let Projection::Hit(h) = r else { return Ok(Projection::NoHit) };
        let s = self.to_query.scale();
        let foot = self.to_query.apply_point(&h.foot)?;
        let distance = h.distance * s;
        if !distance.is_finite() {
            return Err(BodyError::Transform(XformError::NonFinite));
        }
        if opts.max_distance.is_some_and(|d| distance > d) {
            return Ok(Projection::NoHit);
        }
        Ok(Projection::Hit(ProjectionHit { foot, distance, signed_distance: h.signed_distance * s, ..h }))
    }

    fn closest_inst(&self, p: V3, opts: &ProjectionOptions<'_, '_>) -> Result<Projection, BodyError> {
        check_point(&p)?;
        let q = self.to_def.apply_point(&p)?;
        let o = self.def_opts(opts)?;
        let r = self.inner.project(q, &o).map_err(Self::unwrap_err)?;
        self.back(r, opts)
    }

    fn along_inst(&self, p: V3, v: V3, opts: &ProjectionOptions<'_, '_>) -> Result<Projection, BodyError> {
        check_point(&p)?;
        if !finite3(&v) {
            return Err(BodyError::InvalidDirection);
        }
        let q = self.to_def.apply_point(&p)?;
        let w = self.to_def.apply_dir(&v)?;
        let o = self.def_opts(opts)?;
        let r = self.inner.project_along(q, w, &o).map_err(Self::unwrap_err)?;
        self.back(r, opts)
    }

    fn unwrap_err(e: ProjectError) -> BodyError {
        match e {
            ProjectError::Body(b) => b,
            ProjectError::InvalidDirection => BodyError::InvalidDirection,
            ProjectError::Sign(_) => BodyError::Inconsistent,
        }
    }
}

impl ProjectionTarget for InstanceTarget<'_> {
    fn frame(&self) -> FrameRef {
        self.frame
    }

    fn check_options(&self, opts: &ProjectionOptions<'_, '_>) -> Result<(), ProjectError> {
        self.inner.check_options(opts)
    }

    fn project(&self, p: V3, opts: &ProjectionOptions<'_, '_>) -> Result<Projection, ProjectError> {
        self.closest_inst(p, opts).map_err(ProjectError::Body)
    }

    fn project_along(&self, p: V3, v: V3, opts: &ProjectionOptions<'_, '_>) -> Result<Projection, ProjectError> {
        self.along_inst(p, v, opts).map_err(ProjectError::Body)
    }
}
