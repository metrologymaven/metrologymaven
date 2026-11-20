//! Parameter-space curves for coedges. Spec node `GEO.CAD.TOPO.PCURVE`.
//!
//! [`derive_pcurves`] gives every coedge of a checked [`Body`] a [`TrimCurve`]: the imported
//! pcurve when the coedge carries one (REQ-001, returned unchanged and never re-derived),
//! otherwise a piecewise-linear uv polyline derived by inverting the 3-D edge curve onto the
//! face surface (REQ-002). Each face gets a [`FaceChart`]: the periods of its surface and one
//! offset per loop, so that the loops of a face on a periodic surface lie in one continuous
//! parameter chart (REQ-004). [`pcurve_disagreement`] measures how far an imported pcurve's
//! surface image is from its 3-D edge curve (REQ-005).
//!
//! # Pinned rules (value-affecting; `fixtures/geo/cad/topo/make_pcurve_fixtures.py` models
//! them bit for bit and checks them against an independent closed-form oracle)
//!
//! **Samples.** For an edge range `[t0, t1]` the [`INITIAL_SAMPLES`] = 17 parameters are
//! `t_k = t0 + ((t1 - t0) * k) / 16` for `k < 16` and `t_16 = t1`. A coedge of sense
//! `Reversed` walks them from `t_16` down to `t_0`, so a derived polyline always runs in the
//! coedge's direction (its `t` decreases for a reversed coedge).
//!
//! **Inversion.** Each sample point `C(t)` is inverted onto the face surface: analytic
//! surfaces by their closed form ([`AnalyticSurface::invert`]), NURBS surfaces by the global
//! [`invert_surface`] of GEO.NURBS.INVERT (it takes no seed: the previous sample's uv is used
//! for unwrapping, not for seeding; see the PR). An inversion that is not a certified unique
//! answer never yields a pcurve: `Unresolved` (and `InvertError::NonConvergence`), a certified
//! `tie`, an analytic `NonUnique` foot and any inversion error all fail the coedge with
//! [`FailReason::Inversion`].
//!
//! **Tolerance.** Every vertex of a derived polyline and every chord midpoint checked lies
//! within `tol_m` ([`DEFAULT_TOL_M`] = 1e-8 m) of the edge curve, measured in 3-D as
//! `hypot(hypot(e0, e1), e2)` with `e = S(uv) - C(t)` (the GEO.NURBS.ANALYTIC distance).
//! `max_dev_m` is the largest of all these measured distances. If any of the 17 initial
//! samples is farther than `tol_m` from the surface the coedge fails at once
//! ([`FailReason::Tolerance`], `best_deviation_m` the largest sample distance): no
//! polyline through those parameters can do better.
//!
//! **Singular samples (poles and apexes).** A sample is singular in `u` when
//! `|Su|^2 <= SING2 |Sv|^2` (and in `v` when `|Sv|^2 <= SING2 |Su|^2`) at its inverted
//! parameters, [`SING2`] = 2^-80 (`|Su| <= 2^-40 |Sv|`): a sphere pole, a cone apex, a
//! collapsed NURBS boundary. Its singular coordinate is not determined by the point, so it is
//! inherited from the left neighbour in the sweep (the first sample takes it from the first
//! initial sample that is not singular in that coordinate). When the sweep leaves a singular
//! vertex `L` towards `R`, the chord starts at `A = L` with the singular coordinate replaced
//! by `R`'s; if `A` differs from `L` a second vertex `A` with the same `t` is emitted (a
//! zero-length 3-D step along the degenerate line), so the polyline stays continuous in uv.
//!
//! **Unwrapping.** On a periodic direction with period `P` a sample's coordinate `c` is moved
//! to `c + P * floor((r - c) / P + 0.5)` with the reference `r` the left neighbour's coordinate,
//! so consecutive vertices differ by at most half a period (exact ties go up). When the left
//! neighbour is singular in that coordinate (a pole crossing, where every jump is valid) the
//! reference is the coedge's first vertex instead. The first vertex of a coedge is unwrapped
//! against the end of the previous coedge of its loop (see loops below), if any.
//!
//! **Refinement.** Segments are processed left to right from a stack. A segment `(L, R)` is
//! accepted when the surface image of its uv midpoint `(A + R) * 0.5` lies within `tol_m` of
//! `C((t_L + t_R) * 0.5)`; otherwise the midpoint parameter is sampled, inverted and the two
//! halves are processed, left first. Bisection stops at depth [`MAX_DEPTH`] = 12 below the
//! 16 initial segments, so a polyline has at most 16 x 2^12 = [`MAX_SEGMENTS`] = 2^16
//! segments; a segment that still fails there fails the coedge ([`FailReason::Tolerance`],
//! REQ-003) with `best_deviation_m` the largest distance measured so far.
//!
//! **Loops and charts.** Coedges of a loop are processed from the first coedge with an
//! imported pcurve (else the first coedge) cyclically, so derived coedges continue the chart
//! of the imported ones; imported pcurves are evaluated at the edge parameter (same-parameter
//! convention) and never shifted. For a loop whose coedges all have a curve, the displacement
//! `sum_k (end_k - start_k) + reduce(start_{k+1} - end_k)` around the loop (`reduce(j) =
//! j - P floor(j / P + 0.5)`, loop order, cyclic) gives `periods = floor(total / P + 0.5)` per
//! periodic direction; a loop with non-zero periods is reported as
//! [`PcurveError::SeamWinding`] (valid for a band, e.g. a full cylinder band winds once). The
//! loop's range is the bounding box of its derived vertices and of the 33 sample points of
//! each imported pcurve; its centre is `(lo + hi) * 0.5`. The anchor loop (the outer loop,
//! else the first loop of the face) gets the offset `-P floor((centre - lo_P) / P)` that puts
//! its centre in `[lo_P, lo_P + P)` (`lo_P` the domain start, 0 for analytic surfaces); every
//! other loop gets `-P floor((centre - c0) / P + 0.5)` with `c0` the anchored centre. A loop
//! with a failed coedge, or an imported pcurve that cannot be evaluated at its edge range, has
//! no periods (`None`) and offset zero; if it is the anchor every offset is zero.
//!
//! **Disagreement.** [`pcurve_disagreement`] evaluates an imported pcurve at the
//! [`DISAGREEMENT_SAMPLES`] = 33 parameters `t0 + ((t1 - t0) * k) / 32` (`t_32 = t1`) of its
//! edge range and returns the largest 3-D distance between the surface image and the edge
//! curve.
//!
//! # Determinism (D0, KNP-1)
//!
//! Binary64 `+ - * /` written out one operation at a time (no `mul_add`, no `f64::min` or
//! `f64::max`: comparisons instead), `floor` (exact), `hypot` from `fid_num::f`, trigonometry
//! only inside GEO.NURBS. Fixed sample order, a left-to-right stack with a fixed depth limit,
//! vectors only, every loop bounded. Nothing panics.

use core::fmt;

use fid_geo_nurbs::{
    invert_surface, AnalyticSurface, Curve, EvalError, Inversion, InvertError, NurbsSurface, Reason, Status, Surface,
    Uniqueness,
};

use super::geom::{Curve2, CurveDef, SurfaceDef};
use super::model::{Body, CoedgeIx, LoopIx, Sense};

/// REQ-002: default derivation tolerance, metres [unverified].
pub const DEFAULT_TOL_M: f64 = 1e-8;
/// Uniform samples of the edge range before refinement.
pub const INITIAL_SAMPLES: usize = 17;
/// Bisection depth below the 16 initial segments.
pub const MAX_DEPTH: u32 = 12;
/// REQ-003: most segments of a derived polyline (16 x 2^MAX_DEPTH).
pub const MAX_SEGMENTS: usize = 65_536;
/// REQ-005: parameters at which an imported pcurve is compared with its edge curve.
pub const DISAGREEMENT_SAMPLES: usize = 33;
/// Singular-direction threshold on squared derivative lengths (2^-80).
pub const SING2: f64 = 1.0 / 1_208_925_819_614_629_174_706_176.0;

const DIM: usize = 2;

// ---------------------------------------------------------------- public types

/// The trimming curve of one coedge.
#[derive(Debug, Clone, PartialEq)]
pub enum TrimCurve {
    /// The coedge's imported pcurve, unchanged (REQ-001).
    Imported(Curve2),
    /// A derived uv polyline in the coedge's direction: vertex `i` is `uv[i]` at edge
    /// parameter `t[i]`; `max_dev_m` is the largest 3-D distance measured (REQ-002).
    Derived { uv: Vec<[f64; 2]>, t: Vec<f64>, max_dev_m: f64 },
}

/// Why an inversion was not accepted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InversionFault {
    /// GEO.NURBS.INVERT could not certify the answer.
    Unresolved(Reason),
    /// GEO.NURBS.INVERT found two distinct closest points.
    Tie,
    /// The analytic closed form flagged the foot as not unique.
    NonUnique,
    /// The inversion refused the query (its error code).
    Error(&'static str),
}

/// Why a pcurve could not be derived.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailReason {
    /// The edge has no 3-D curve and the coedge no pcurve.
    NoEdgeCurve,
    /// The edge curve could not be evaluated at a sample parameter.
    CurveEval,
    /// The surface could not be evaluated (or a distance overflowed).
    SurfaceEval,
    /// An inversion was not a certified unique answer.
    Inversion(InversionFault),
    /// The tolerance was not reached within [`MAX_SEGMENTS`] segments, or a sample is off the
    /// surface.
    Tolerance,
}

/// Errors and reports of this leaf.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PcurveError {
    /// The tolerance is not a finite positive number.
    BadTolerance { value: f64 },
    /// REQ-003: no pcurve for this coedge; the other coedges are unaffected.
    /// `best_deviation_m` is the largest distance measured (infinite when none was).
    PcurveFailed { coedge: CoedgeIx, best_deviation_m: f64, reason: FailReason },
    /// REQ-004: the loop does not close in the unwrapped chart; `periods` per direction.
    SeamWinding { r#loop: LoopIx, periods: [i64; 2] },
    /// Coedge `index` lies in no loop (unreachable for a body from `BodyBuilder::build`).
    NotInLoop { index: usize },
}

impl PcurveError {
    /// Stable variant name.
    pub fn code(&self) -> &'static str {
        match self {
            PcurveError::BadTolerance { .. } => "BadTolerance",
            PcurveError::PcurveFailed { .. } => "PcurveFailed",
            PcurveError::SeamWinding { .. } => "SeamWinding",
            PcurveError::NotInLoop { .. } => "NotInLoop",
        }
    }
}

impl fmt::Display for PcurveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PcurveError::BadTolerance { value } => write!(f, "tolerance {value} is not finite and positive"),
            PcurveError::PcurveFailed { coedge, best_deviation_m, reason } => {
                write!(f, "no pcurve for coedge {}: {reason:?}, best deviation {best_deviation_m:e} m", coedge.index())
            }
            PcurveError::SeamWinding { r#loop, periods } => {
                write!(
                    f,
                    "loop {} winds {} x {} periods in the unwrapped chart",
                    r#loop.index(),
                    periods[0],
                    periods[1]
                )
            }
            PcurveError::NotInLoop { index } => write!(f, "coedge {index} lies in no loop"),
        }
    }
}

impl core::error::Error for PcurveError {}

/// Why [`pcurve_disagreement`] has no value.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DisagreementError {
    /// The coedge index is not in the body.
    UnknownCoedge,
    /// The coedge carries no imported pcurve.
    NotImported,
    /// The edge has no 3-D curve.
    NoEdgeCurve,
    /// The pcurve could not be evaluated at an edge parameter.
    PcurveEval(EvalError),
    /// The edge curve could not be evaluated.
    CurveEval(EvalError),
    /// The surface could not be evaluated at the pcurve's uv (or a distance overflowed).
    SurfaceEval(EvalError),
}

/// The parameter chart of one face (REQ-004).
#[derive(Debug, Clone, PartialEq)]
pub struct FaceChart {
    /// Period of `u`, `None` when `u` is not periodic.
    pub period_u: Option<f64>,
    /// Period of `v`, `None` when `v` is not periodic.
    pub period_v: Option<f64>,
    /// Per loop of the face (face order): add to every uv of the loop's trim curves.
    pub loop_offsets: Vec<[f64; 2]>,
    /// Per loop: periods wound in `u` and `v`, `None` when the loop is incomplete.
    pub loop_periods: Vec<Option<[i64; 2]>>,
}

/// Result of [`derive_pcurves`].
#[derive(Debug, Clone, PartialEq)]
pub struct Pcurves {
    /// Per coedge, in canonical coedge order.
    pub curves: Vec<Result<TrimCurve, PcurveError>>,
    /// Per face, in canonical face order.
    pub charts: Vec<FaceChart>,
    /// [`PcurveError::SeamWinding`] reports in ascending loop order.
    pub windings: Vec<PcurveError>,
}

impl Pcurves {
    /// Every failure, coedge order, then every winding report.
    pub fn errors(&self) -> Vec<PcurveError> {
        let mut out: Vec<PcurveError> = self.curves.iter().filter_map(|c| c.as_ref().err().copied()).collect();
        out.extend(self.windings.iter().copied());
        out
    }
}

// ---------------------------------------------------------------- geometry helpers

pub(crate) fn hypot(x: f64, y: f64) -> Option<f64> {
    fid_num::f::hypot(x, y).ok()
}

/// `hypot(hypot(e0, e1), e2)` with `e = a - b`.
pub(crate) fn dist3(a: [f64; 3], b: [f64; 3]) -> Option<f64> {
    let e = [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
    let d = hypot(hypot(e[0], e[1])?, e[2])?;
    if d.is_finite() {
        Some(d)
    } else {
        None
    }
}

/// `(a0 b0 + a1 b1) + a2 b2`.
fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    (a[0] * b[0] + a[1] * b[1]) + a[2] * b[2]
}

/// `floor(x)`; `x` is finite.
pub(crate) fn floor(x: f64) -> f64 {
    x.floor()
}

/// `c + P floor((r - c) / P + 0.5)`: the representative of `c` nearest `r`.
fn nearest(c: f64, r: f64, p: f64) -> f64 {
    let k = floor((r - c) / p + 0.5);
    if k == 0.0 {
        c
    } else {
        c + k * p
    }
}

fn reduce(j: f64, p: f64) -> f64 {
    j - floor(j / p + 0.5) * p
}

pub(crate) fn larger(a: f64, b: f64) -> f64 {
    if b > a {
        b
    } else {
        a
    }
}

#[derive(Clone, Copy)]
pub(crate) enum Surf<'a> {
    A(&'a AnalyticSurface),
    N(&'a NurbsSurface),
}

impl<'a> Surf<'a> {
    pub(crate) fn of(def: &'a SurfaceDef) -> Option<Surf<'a>> {
        if let Some(a) = def.as_analytic() {
            return Some(Surf::A(a));
        }
        def.as_nurbs().map(Surf::N)
    }

    pub(crate) fn periods(&self) -> [Option<f64>; DIM] {
        use core::f64::consts::TAU;
        match self {
            Surf::A(a) => match a {
                AnalyticSurface::Plane(_) => [None, None],
                AnalyticSurface::Cylinder(_) | AnalyticSurface::Cone(_) | AnalyticSurface::Sphere(_) => {
                    [Some(TAU), None]
                }
                AnalyticSurface::Torus(_) => [Some(TAU), Some(TAU)],
            },
            Surf::N(n) => {
                let ((u0, u1), (v0, v1)) = n.domain();
                [n.periodic_u().then_some(u1 - u0), n.periodic_v().then_some(v1 - v0)]
            }
        }
    }

    /// Start of each parameter domain (0 for the analytic periodic angles).
    fn lower(&self) -> [f64; DIM] {
        match self {
            Surf::A(_) => [0.0, 0.0],
            Surf::N(n) => {
                let ((u0, _), (v0, _)) = n.domain();
                [u0, v0]
            }
        }
    }

    /// Analytic surfaces take any unwrapped value; a NURBS surface's periodic coordinates are
    /// reduced into its domain first.
    fn param(&self, uv: [f64; DIM]) -> [f64; DIM] {
        match self {
            Surf::A(_) => uv,
            Surf::N(n) => {
                let ((u0, u1), (v0, v1)) = n.domain();
                let w = |c: f64, lo: f64, hi: f64, periodic: bool| {
                    if !periodic {
                        return c;
                    }
                    let p = hi - lo;
                    let r = c - floor((c - lo) / p) * p;
                    if r < lo {
                        lo
                    } else if r > hi {
                        hi
                    } else {
                        r
                    }
                };
                [w(uv[0], u0, u1, n.periodic_u()), w(uv[1], v0, v1, n.periodic_v())]
            }
        }
    }

    pub(crate) fn point(&self, uv: [f64; DIM]) -> Result<[f64; 3], EvalError> {
        let [u, v] = self.param(uv);
        Ok(match self {
            Surf::A(a) => a.eval(u, v, 0)?.p,
            Surf::N(n) => n.eval(u, v, 0)?.p,
        })
    }

    /// Singular directions at `uv` (see the module docs).
    fn singular(&self, uv: [f64; DIM]) -> Result<[bool; DIM], EvalError> {
        let [u, v] = self.param(uv);
        let d = match self {
            Surf::A(a) => a.eval(u, v, 1)?,
            Surf::N(n) => n.eval(u, v, 1)?,
        };
        let (su, sv) = match (d.su, d.sv) {
            (Some(su), Some(sv)) => (su, sv),
            _ => return Err(EvalError::Inconsistent),
        };
        let (a2, b2) = (dot(su, su), dot(sv, sv));
        Ok([a2 <= SING2 * b2, b2 <= SING2 * a2])
    }

    /// Accepted inversion `(uv, distance)`, or the fault and the best known distance.
    fn invert(&self, q: [f64; 3]) -> Result<([f64; DIM], f64), (InversionFault, Option<f64>)> {
        match self {
            Surf::A(a) => match a.invert(q) {
                Ok(f) => match f.uniqueness {
                    Uniqueness::Unique => Ok(([f.u, f.v], f.distance)),
                    Uniqueness::NonUnique => Err((InversionFault::NonUnique, Some(f.distance))),
                },
                Err(e) => Err((InversionFault::Error(e.code()), None)),
            },
            Surf::N(n) => accept_nurbs(invert_surface(n, q)),
        }
    }
}

/// Only a certified, untied GEO.NURBS.INVERT answer is accepted.
fn accept_nurbs(
    r: Result<Inversion, InvertError<Inversion>>,
) -> Result<([f64; DIM], f64), (InversionFault, Option<f64>)> {
    match r {
        Ok(i) => match i.status {
            Status::Certified { tie: false } => Ok(([i.u, i.v], i.distance)),
            Status::Certified { tie: true } => Err((InversionFault::Tie, Some(i.distance))),
            Status::Unresolved { reason } => Err((InversionFault::Unresolved(reason), Some(i.distance))),
        },
        Err(InvertError::NonConvergence { best }) => {
            let reason = match best.status {
                Status::Unresolved { reason } => reason,
                Status::Certified { .. } => Reason::Convergence,
            };
            Err((InversionFault::Unresolved(reason), Some(best.distance)))
        }
        Err(e) => Err((InversionFault::Error(e.code()), None)),
    }
}

fn curve_point(c: &CurveDef, t: f64) -> Result<[f64; 3], EvalError> {
    if let Some(a) = c.as_analytic() {
        return Ok(a.eval(t, 0)?.p);
    }
    match c.as_nurbs() {
        Some(n) => Ok(n.eval(t, 0)?.p),
        None => Err(EvalError::Inconsistent),
    }
}

pub(crate) fn pcurve_point(p: &Curve2, t: f64) -> Result<[f64; DIM], EvalError> {
    if let Some((o, d)) = p.as_line() {
        let uv = [o[0] + t * d[0], o[1] + t * d[1]];
        if !(uv[0].is_finite() && uv[1].is_finite()) {
            return Err(EvalError::NonFinite);
        }
        return Ok(uv);
    }
    match p.as_nurbs() {
        Some(n) => {
            let r = n.eval(t, 0)?.p;
            Ok([r[0], r[1]])
        }
        None => Err(EvalError::Inconsistent),
    }
}

/// `t0 + ((t1 - t0) * k) / n` for `k < n`, `t1` for `k = n`.
pub(crate) fn grid(t0: f64, t1: f64, k: usize, n: usize) -> f64 {
    if k == n {
        t1
    } else {
        t0 + ((t1 - t0) * k as f64) / n as f64
    }
}

// ---------------------------------------------------------------- derivation of one coedge

/// A failed derivation: the reason and the largest distance measured.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Fail {
    reason: FailReason,
    best: f64,
}

struct Raw {
    t: f64,
    q: [f64; 3],
    uv: [f64; DIM],
    dist: f64,
    sing: [bool; DIM],
    depth: u32,
}

/// Samples `t`: the curve point, the accepted inversion and its singular directions.
fn sample(s: Surf<'_>, c: &CurveDef, t: f64, depth: u32) -> Result<Raw, (FailReason, Option<f64>)> {
    let q = curve_point(c, t).map_err(|_| (FailReason::CurveEval, None))?;
    let (uv, dist) = s.invert(q).map_err(|(f, d)| (FailReason::Inversion(f), d))?;
    let sing = s.singular(uv).map_err(|_| (FailReason::SurfaceEval, Some(dist)))?;
    Ok(Raw { t, q, uv, dist, sing, depth })
}

struct Out {
    uv: Vec<[f64; DIM]>,
    t: Vec<f64>,
    max_dev: f64,
}

impl Out {
    /// Emits a vertex after measuring it against the edge point `q`.
    fn emit(&mut self, s: Surf<'_>, uv: [f64; DIM], t: f64, q: [f64; 3], tol: f64) -> Result<(), Fail> {
        let d = deviation(s, uv, q).map_err(|reason| Fail { reason, best: self.max_dev })?;
        self.max_dev = larger(self.max_dev, d);
        if d > tol {
            return Err(Fail { reason: FailReason::Tolerance, best: self.max_dev });
        }
        self.uv.push(uv);
        self.t.push(t);
        Ok(())
    }
}

fn deviation(s: Surf<'_>, uv: [f64; DIM], q: [f64; 3]) -> Result<f64, FailReason> {
    let p = s.point(uv).map_err(|_| FailReason::SurfaceEval)?;
    dist3(p, q).ok_or(FailReason::SurfaceEval)
}

/// Derives the polyline of one coedge over `[t0, t1]` (REQ-002, REQ-003). `reference` is the
/// uv the first vertex is unwrapped against (the end of the previous coedge of the loop).
fn derive(
    s: Surf<'_>,
    c: &CurveDef,
    range: [f64; 2],
    reversed: bool,
    tol: f64,
    reference: Option<[f64; DIM]>,
) -> Result<TrimCurve, Fail> {
    let periods = s.periods();
    // 1. The 17 initial samples, all of them, so a failure reports the worst distance.
    let mut init: Vec<Raw> = Vec::with_capacity(INITIAL_SAMPLES);
    let mut worst = 0.0;
    let mut unknown = false;
    let mut fault: Option<FailReason> = None;
    for k in 0..INITIAL_SAMPLES {
        let j = if reversed { INITIAL_SAMPLES - 1 - k } else { k };
        match sample(s, c, grid(range[0], range[1], j, INITIAL_SAMPLES - 1), 0) {
            Ok(r) => {
                worst = larger(worst, r.dist);
                init.push(r);
            }
            Err((reason, d)) => {
                match d {
                    Some(d) => worst = larger(worst, d),
                    None => unknown = true,
                }
                if fault.is_none() {
                    fault = Some(reason);
                }
            }
        }
    }
    if let Some(reason) = fault {
        return Err(Fail { reason, best: if unknown { f64::INFINITY } else { worst } });
    }
    if worst > tol {
        return Err(Fail { reason: FailReason::Tolerance, best: worst });
    }

    // 2. The first vertex: singular coordinates from the first non-singular initial sample,
    //    periodic coordinates nearest the reference.
    let mut stack: Vec<Raw> = Vec::with_capacity(INITIAL_SAMPLES + 2 * MAX_DEPTH as usize);
    let mut first_uv = [0.0; DIM];
    let first = match init.first() {
        Some(f) => (f.t, f.q, f.sing, f.uv),
        None => return Err(Fail { reason: FailReason::CurveEval, best: worst }),
    };
    for (i, fu) in first_uv.iter_mut().enumerate() {
        let mut v = first.3[i];
        if first.2[i] {
            if let Some(r) = init.iter().find(|r| !r.sing[i]) {
                v = r.uv[i];
            }
        }
        if let (Some(p), Some(r)) = (periods[i], reference) {
            v = nearest(v, r[i], p);
        }
        *fu = v;
    }
    let anchor = first_uv;
    let mut out = Out { uv: Vec::new(), t: Vec::new(), max_dev: 0.0 };
    out.emit(s, first_uv, first.0, first.1, tol)?;
    // The left vertex of the current segment: t, edge point, uv, singular flags.
    let mut l_t = first.0;
    let mut l_q = first.1;
    let mut l_uv = first_uv;
    let mut l_sing = first.2;
    while init.len() > 1 {
        if let Some(r) = init.pop() {
            stack.push(r);
        }
    }

    // 3. Left-to-right refinement.
    while let Some(r) = stack.pop() {
        let mut r_uv = r.uv;
        for i in 0..DIM {
            if r.sing[i] {
                r_uv[i] = l_uv[i];
            } else if let Some(p) = periods[i] {
                let reference = if l_sing[i] { anchor[i] } else { l_uv[i] };
                r_uv[i] = nearest(r.uv[i], reference, p);
            }
        }
        let mut a = l_uv;
        for i in 0..DIM {
            if l_sing[i] {
                a[i] = r_uv[i];
            }
        }
        let mid = [(a[0] + r_uv[0]) * 0.5, (a[1] + r_uv[1]) * 0.5];
        let tm = (l_t + r.t) * 0.5;
        let qm = curve_point(c, tm).map_err(|_| Fail { reason: FailReason::CurveEval, best: out.max_dev })?;
        let dm = deviation(s, mid, qm).map_err(|reason| Fail { reason, best: out.max_dev })?;
        if dm <= tol {
            out.max_dev = larger(out.max_dev, dm);
            if a[0].to_bits() != l_uv[0].to_bits() || a[1].to_bits() != l_uv[1].to_bits() {
                out.emit(s, a, l_t, l_q, tol)?;
            }
            out.emit(s, r_uv, r.t, r.q, tol)?;
            l_t = r.t;
            l_q = r.q;
            l_uv = r_uv;
            l_sing = r.sing;
            continue;
        }
        let best = larger(out.max_dev, dm);
        if r.depth >= MAX_DEPTH || tm == l_t || tm == r.t {
            return Err(Fail { reason: FailReason::Tolerance, best });
        }
        let m = sample(s, c, tm, r.depth + 1).map_err(|(reason, d)| Fail {
            reason,
            best: match d {
                Some(d) => larger(best, d),
                None => f64::INFINITY,
            },
        })?;
        if m.dist > tol {
            return Err(Fail { reason: FailReason::Tolerance, best: larger(best, m.dist) });
        }
        stack.push(Raw { depth: r.depth + 1, ..r });
        stack.push(m);
    }
    Ok(TrimCurve::Derived { uv: out.uv, t: out.t, max_dev_m: out.max_dev })
}

/// Derives the pcurve of one coedge in its own chart (no loop context), ignoring any imported
/// pcurve. The building block of [`derive_pcurves`], exposed for validators and tests.
pub fn derive_coedge(body: &Body, coedge: CoedgeIx, tol_m: f64) -> Result<TrimCurve, PcurveError> {
    check_tol(tol_m)?;
    let failed = |reason, best| PcurveError::PcurveFailed { coedge, best_deviation_m: best, reason };
    let surf = coedge_surface(body, coedge).ok_or(failed(FailReason::SurfaceEval, f64::INFINITY))?;
    derive_one(body, coedge, surf, tol_m, None).map_err(|f| failed(f.reason, f.best))
}

fn check_tol(tol_m: f64) -> Result<(), PcurveError> {
    if tol_m.is_finite() && tol_m > 0.0 {
        Ok(())
    } else {
        Err(PcurveError::BadTolerance { value: tol_m })
    }
}

fn coedge_surface(body: &Body, coedge: CoedgeIx) -> Option<Surf<'_>> {
    let lp = body.r#loop(body.coedge(coedge)?.r#loop())?;
    Surf::of(body.face(lp.face())?.surface())
}

fn derive_one(
    body: &Body,
    coedge: CoedgeIx,
    s: Surf<'_>,
    tol: f64,
    reference: Option<[f64; DIM]>,
) -> Result<TrimCurve, Fail> {
    let no_curve = Fail { reason: FailReason::NoEdgeCurve, best: f64::INFINITY };
    let ce = body.coedge(coedge).ok_or(no_curve)?;
    let edge = body.edge(ce.edge()).ok_or(no_curve)?;
    let c = edge.curve().ok_or(no_curve)?;
    derive(s, c, edge.param_range(), ce.sense() == Sense::Reversed, tol, reference)
}

// ---------------------------------------------------------------- loops and faces

/// Range accumulator over the periodic coordinates of a loop.
struct Range {
    lo: [f64; DIM],
    hi: [f64; DIM],
    empty: bool,
}

impl Range {
    fn add(&mut self, p: [f64; DIM]) {
        for i in 0..DIM {
            if self.empty || p[i] < self.lo[i] {
                self.lo[i] = p[i];
            }
            if self.empty || p[i] > self.hi[i] {
                self.hi[i] = p[i];
            }
        }
        self.empty = false;
    }

    fn centre(&self) -> [f64; DIM] {
        [(self.lo[0] + self.hi[0]) * 0.5, (self.lo[1] + self.hi[1]) * 0.5]
    }
}

/// Start and end uv of an imported pcurve in its coedge's direction, and its 33 sample points.
fn imported_ends(body: &Body, coedge: CoedgeIx, p: &Curve2, range: &mut Range) -> Option<([f64; DIM], [f64; DIM])> {
    let ce = body.coedge(coedge)?;
    let [t0, t1] = body.edge(ce.edge())?.param_range();
    for k in 0..DISAGREEMENT_SAMPLES {
        range.add(pcurve_point(p, grid(t0, t1, k, DISAGREEMENT_SAMPLES - 1)).ok()?);
    }
    let (a, b) = (pcurve_point(p, t0).ok()?, pcurve_point(p, t1).ok()?);
    Some(if ce.sense() == Sense::Reversed { (b, a) } else { (a, b) })
}

struct LoopResult {
    periods: Option<[i64; 2]>,
    centre: [f64; DIM],
}

fn process_loop(
    body: &Body,
    s: Surf<'_>,
    coedges: &[CoedgeIx],
    tol: f64,
    curves: &mut [Option<Result<TrimCurve, PcurveError>>],
) -> LoopResult {
    let periods = s.periods();
    let n = coedges.len();
    let start = coedges.iter().position(|&c| body.coedge(c).and_then(|ce| ce.pcurve()).is_some()).unwrap_or(0);
    let mut ends: Vec<Option<([f64; DIM], [f64; DIM])>> = vec![None; n];
    let mut range = Range { lo: [0.0; DIM], hi: [0.0; DIM], empty: true };
    let mut prev_end: Option<[f64; DIM]> = None;
    for i in 0..n {
        let k = (start + i) % n;
        let cx = coedges[k];
        let imported = body.coedge(cx).and_then(|ce| ce.pcurve());
        let (result, end) = match imported {
            Some(p) => {
                let e = imported_ends(body, cx, p, &mut range);
                ends[k] = e;
                (Ok(TrimCurve::Imported(p.clone())), e.map(|x| x.1))
            }
            None => match derive_one(body, cx, s, tol, prev_end) {
                Ok(tc) => {
                    let mut e = None;
                    if let TrimCurve::Derived { uv, .. } = &tc {
                        for p in uv {
                            range.add(*p);
                        }
                        if let (Some(a), Some(b)) = (uv.first(), uv.last()) {
                            e = Some((*a, *b));
                        }
                    }
                    ends[k] = e;
                    (Ok(tc), e.map(|x| x.1))
                }
                Err(f) => {
                    (Err(PcurveError::PcurveFailed { coedge: cx, best_deviation_m: f.best, reason: f.reason }), None)
                }
            },
        };
        if let Some(slot) = curves.get_mut(cx.index()) {
            *slot = Some(result);
        }
        prev_end = end;
    }
    let complete = ends.iter().all(Option::is_some);
    let mut out = LoopResult { periods: None, centre: range.centre() };
    if complete && !range.empty {
        let mut wound = [0i64; 2];
        for (i, w) in wound.iter_mut().enumerate() {
            let Some(p) = periods[i] else { continue };
            let mut total = 0.0;
            for k in 0..n {
                if let (Some((a, b)), Some((next, _))) = (ends[k], ends[(k + 1) % n]) {
                    total += b[i] - a[i];
                    total += reduce(next[i] - b[i], p);
                }
            }
            *w = floor(total / p + 0.5) as i64;
        }
        out.periods = Some(wound);
    }
    out
}

/// Derives a [`TrimCurve`] for every coedge and a [`FaceChart`] for every face of `body`
/// (REQ-001 to REQ-004), with derivation tolerance `tol_m` metres ([`DEFAULT_TOL_M`]).
/// A coedge that cannot be derived gets [`PcurveError::PcurveFailed`] and the others carry on.
pub fn derive_pcurves(body: &Body, tol_m: f64) -> Result<Pcurves, PcurveError> {
    check_tol(tol_m)?;
    let mut curves: Vec<Option<Result<TrimCurve, PcurveError>>> = vec![None; body.coedges().len()];
    let mut charts = Vec::with_capacity(body.faces().len());
    let mut wound: Vec<(LoopIx, [i64; 2])> = Vec::new();
    for face in body.faces() {
        let Some(s) = Surf::of(face.surface()) else {
            charts.push(FaceChart {
                period_u: None,
                period_v: None,
                loop_offsets: Vec::new(),
                loop_periods: Vec::new(),
            });
            continue;
        };
        let periods = s.periods();
        let lower = s.lower();
        let mut results = Vec::with_capacity(face.loops().len());
        for &lx in face.loops() {
            let coedges = body.r#loop(lx).map(|l| l.coedges()).unwrap_or(&[]);
            let r = process_loop(body, s, coedges, tol_m, &mut curves);
            if let Some(p) = r.periods {
                if p != [0, 0] {
                    wound.push((lx, p));
                }
            }
            results.push(r);
        }
        // Anchor: the outer loop, else the first.
        let anchor = face.loops().iter().position(|&lx| body.r#loop(lx).is_some_and(|l| l.outer())).unwrap_or(0);
        let mut offsets = vec![[0.0; DIM]; results.len()];
        if let Some(a) = results.get(anchor).filter(|r| r.periods.is_some()) {
            let mut c0 = [0.0; DIM];
            let mut off_a = [0.0; DIM];
            for i in 0..DIM {
                if let Some(p) = periods[i] {
                    off_a[i] = 0.0 - floor((a.centre[i] - lower[i]) / p) * p;
                    c0[i] = a.centre[i] + off_a[i];
                }
            }
            for (j, r) in results.iter().enumerate() {
                if j == anchor {
                    offsets[j] = off_a;
                    continue;
                }
                if r.periods.is_none() {
                    continue;
                }
                for i in 0..DIM {
                    if let Some(p) = periods[i] {
                        offsets[j][i] = 0.0 - floor((r.centre[i] - c0[i]) / p + 0.5) * p;
                    }
                }
            }
        }
        charts.push(FaceChart {
            period_u: periods[0],
            period_v: periods[1],
            loop_offsets: offsets,
            loop_periods: results.iter().map(|r| r.periods).collect(),
        });
    }
    wound.sort_by_key(|w| w.0);
    let curves =
        curves.into_iter().enumerate().map(|(index, c)| c.unwrap_or(Err(PcurveError::NotInLoop { index }))).collect();
    Ok(Pcurves {
        curves,
        charts,
        windings: wound.into_iter().map(|(l, periods)| PcurveError::SeamWinding { r#loop: l, periods }).collect(),
    })
}

/// REQ-005: the largest 3-D distance between the surface image of the coedge's imported
/// pcurve and its edge curve over the 33 parameters of the edge range.
pub fn pcurve_disagreement(body: &Body, coedge: CoedgeIx) -> Result<f64, DisagreementError> {
    let ce = body.coedge(coedge).ok_or(DisagreementError::UnknownCoedge)?;
    let p = ce.pcurve().ok_or(DisagreementError::NotImported)?;
    let edge = body.edge(ce.edge()).ok_or(DisagreementError::UnknownCoedge)?;
    let c = edge.curve().ok_or(DisagreementError::NoEdgeCurve)?;
    let s = coedge_surface(body, coedge).ok_or(DisagreementError::UnknownCoedge)?;
    let [t0, t1] = edge.param_range();
    let mut worst = 0.0;
    for k in 0..DISAGREEMENT_SAMPLES {
        let t = grid(t0, t1, k, DISAGREEMENT_SAMPLES - 1);
        let uv = pcurve_point(p, t).map_err(DisagreementError::PcurveEval)?;
        let q = curve_point(c, t).map_err(DisagreementError::CurveEval)?;
        let sp = s.point(uv).map_err(DisagreementError::SurfaceEval)?;
        let d = dist3(sp, q).ok_or(DisagreementError::SurfaceEval(EvalError::NonFinite))?;
        worst = larger(worst, d);
    }
    Ok(worst)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    fn inv(status: Status) -> Inversion {
        Inversion { u: 0.25, v: 0.5, foot: [0.0; 3], distance: 1e-3, on_boundary: false, status }
    }

    #[test]
    fn only_certified_untied_nurbs_inversions_are_accepted() {
        assert_eq!(accept_nurbs(Ok(inv(Status::Certified { tie: false }))), Ok(([0.25, 0.5], 1e-3)));
        assert_eq!(accept_nurbs(Ok(inv(Status::Certified { tie: true }))), Err((InversionFault::Tie, Some(1e-3))));
        for reason in [Reason::Budget, Reason::Iteration, Reason::Convergence] {
            let u = Status::Unresolved { reason };
            assert_eq!(accept_nurbs(Ok(inv(u))), Err((InversionFault::Unresolved(reason), Some(1e-3))));
            assert_eq!(
                accept_nurbs(Err(InvertError::NonConvergence { best: inv(u) })),
                Err((InversionFault::Unresolved(reason), Some(1e-3)))
            );
        }
        assert_eq!(
            accept_nurbs(Err(InvertError::NonFiniteQuery)),
            Err((InversionFault::Error("NonFiniteQuery"), None))
        );
    }

    #[test]
    fn unwrapping_rules() {
        let p = core::f64::consts::TAU;
        assert_eq!(nearest(0.1, 6.2, p), 0.1 + p);
        assert_eq!(nearest(6.2, 0.1, p), 6.2 - p);
        assert_eq!(nearest(1.0, 1.5, p), 1.0);
        // An exact half-period tie goes up.
        assert_eq!(nearest(core::f64::consts::PI, 0.0, p), core::f64::consts::PI);
        assert_eq!(reduce(-p, p), 0.0);
        assert_eq!(grid(0.0, 1.0, 16, 16), 1.0);
        assert_eq!(grid(0.0, 1.0, 8, 16), 0.5);
        assert_eq!(MAX_SEGMENTS, 16 << MAX_DEPTH);
        assert_eq!(SING2, 2f64.powi(-80));
    }
}
