//! Trimmed-face point classification. Spec node `GEO.CAD.TOPO.TRIM`.
//!
//! [`TrimIndex::build`] turns the trimming curves of one face (from [`derive_pcurves`], spec
//! node GEO.CAD.TOPO.PCURVE) into closed uv polylines in the face's parameter chart;
//! [`classify_uv`] says whether a parameter `(u, v)` is [`TrimClass::Inside`],
//! [`TrimClass::Outside`] or [`TrimClass::OnBoundary`] of the face, with the boundary band
//! `tol_m` given in metres (REQ-001 to REQ-005). The rules below are pinned in
//! `spec/geo/cad/topo/trim/NODE.md` (Design, amended by the review of PR 295).
//!
//! # The boundary of record
//!
//! A chart vertex is a [`ChartPoint`]: binary64 coordinates `x` plus whole periods `k`, the
//! exact real `x + k P` in each periodic direction (`P` the period). Periods are never added
//! in floating point, so lifted and copied curves meet exactly.
//!
//! - **Derived pcurves** are their PCURVE polylines, vertex for vertex.
//! - **Imported line pcurves** `o + t d` are the chord from `pcurve(t0)` to `pcurve(t1)`.
//! - **Imported NURBS pcurves** are flattened by fixed bisection at `tol_m / 4` (REQ-002 and
//!   the spec's Design): the 17 parameters `t0 + ((t1 - t0) k) / 16` (`k = 16`: `t1`), then a
//!   left-to-right stack; a chord `(L, R)` is accepted when the surface image of its uv midpoint
//!   lies within `tol_m / 4` of the surface image of the pcurve at `(t_L + t_R) * 0.5`;
//!   otherwise that parameter is inserted, at most `MAX_DEPTH` = 12 levels deep
//!   ([`Undecidable::Flatten`] beyond). [`TrimIndex::build`] flattens at
//!   [`DEFAULT_FLATTEN_TOL_M`]; [`classify_uv`] re-flattens a face that has NURBS pcurves when
//!   `tol_m / 4` differs. A point classified Inside or Outside is farther than `tol_m` from the
//!   polyline's image, so farther than `3 tol_m / 4` from the pcurve's (as far as the midpoint
//!   test measures it), and cannot change side.
//! - Polylines run in the coedge's direction. In a periodic direction every coedge after the
//!   first of its loop gets `k = -floor((s - e) / P + 0.5)` (`s` its first vertex, `e` the
//!   previous coedge's last vertex, both rounded), so a loop of imported pcurves that jumps a
//!   period stays continuous; then the loop's chart offset ([`FaceChart::loop_offsets`]),
//!   `floor(offset / P + 0.5)` periods, is added.
//! - **Sphere poles.** On a sphere a vertex with `|v - (+-pi/2)| <= 2^-40` gets `v = +-pi/2`
//!   exactly (the f64 constants), so nothing of the chart lies beyond a pole.
//! - **Seams.** On a face with a periodic direction, an edge used by exactly two coedges of
//!   the face whose uses lie `k != 0` periods apart (first use's start against second use's
//!   end, `k = floor(d / P + 0.5)`) is a seam: crossed, but not boundary. On a face without a
//!   periodic direction both uses are ordinary boundary (a bridge or slit). If the two uses
//!   coincide (`k = 0`, PCURVE unwraps both to the same place, e.g. a sphere's meridian seam),
//!   and the edge lies on a period line (every vertex within `2^-40 P` of the domain start
//!   plus whole periods; otherwise the two uses are a slit, ordinary boundary on both),
//!   the second use is lifted one period to the side where the material lies: with material
//!   on the left of the first use (on the right for a `Reversed` face), going `+v` puts it at
//!   `-P` in `u`. A coincident use that does not move along the axis is
//!   [`Undecidable::SeamCoincident`].
//! - **Connectors.** Each coedge's last vertex is joined to the next coedge's first vertex
//!   (cyclically) unless they are identical; the closing connector of a loop that winds `w`
//!   periods goes to the first vertex plus `w` periods. A connector next to a seam coedge
//!   (a pole connector) is not boundary either. Zero-length segments are dropped.
//!
//! # Inside and outside
//!
//! - **No periodic direction (or two, e.g. a torus):** even-odd crossing number of the ray
//!   `+u` from `q`, half-open rule (a segment counts when exactly one end has `v >= q.v` and
//!   the crossing lies right of `q`); on a doubly periodic face over every chart copy, inside
//!   if inside any. A doubly periodic face with a winding loop or a seam is
//!   [`Undecidable::Winding`] / [`Undecidable::DoublyPeriodicSeam`].
//! - **One periodic direction `p` (cylinder, cone, sphere, NURBS periodic in one direction):**
//!   the ray goes along the other direction `n` (`+v` for analytic surfaces). The crossings of
//!   the lifted loop curves (all chart copies `k` in `p`) above `q` are counted with the
//!   half-open rule in `p`. Material lies on the left of every loop (right for a `Reversed`
//!   face), so the far end of the ray (band top or pole) is material when the topmost curve
//!   has material above it: a segment at the greatest `n` that runs along `p` decides it
//!   directly (`+u` at the top: material above), otherwise the turn at the topmost vertex
//!   (left turn: material below). `q` is inside when `far_end XOR (crossings odd)`. A query
//!   exactly on a seam or pole connector is inside (both sides are the face).
//! - Every decision uses the exact predicates of [`exact`] on `x + k P`; on the boundary
//!   polylines (a vertex, a segment along the ray's cross direction, a collinear straddling
//!   segment) is [`TrimClass::OnBoundary`] exactly.
//!
//! # The band (REQ-002)
//!
//! `Q = S(u, v)` at the query as given. For each boundary segment `a -> b` (approximate chart
//! coordinates `x + k P` rounded) the distance from `Q` to the surface image of the segment is
//! bounded by branch and bound over dyadic pieces of `s` in `[0, 1]`, points `a + (b - a) s`
//! (left piece first). With `m` the piece midpoint, `d = |Q - S(m)|`, `h` half the Lipschitz
//! bound of `S` along the piece (`(Lu |du| + Lv |dv|) / 2`, times `1 + 2^-20`) and the guard
//! `G` below: `d + G <= tol` is a witness (OnBoundary); `d - h - G > tol` prunes the piece; a
//! piece with `h <= rho / 2`, `rho = tol * 2^-12`, counts as OnBoundary; otherwise it is halved
//! (at most 64 levels and [`MAX_BAND_PIECES`] pieces below the roots per query, else
//! [`TrimError::Budget`]; each segment's root piece is evaluated once per assembly). Every
//! point within `tol_m` of the boundary image is therefore OnBoundary (never Inside or
//! Outside), and every point the parity decides is farther than `tol_m + D` from it.
//!
//! **The guard `G`** bounds every rounding between the real distance and the computed one:
//!
//! - evaluation of `S` at `Q` and at `m`: `2^-48 M(uv)` each for an analytic surface, with
//!   `M = |O|inf + sum |l_k|` the magnitudes of the local coordinates summed in
//!   `O + l0 X + l1 Y + l2 Z` (plane `|u| + |v|`; cylinder `2r + |v|`; cone
//!   `2|r0 + v tan a| + |v|`; sphere `3r`; torus `2(R + r) + r`). Each world coordinate is
//!   three products and three sums (`gamma_6 < 2^-50` relative to `M`), the local coordinates
//!   carry at most two products and the `sin`/`cos` error (taken as 1 ulp, at most `2^-52`
//!   absolute for a value in `[-1, 1]`: REQ-KRN.NUM.FN-006 and its AC-2 self-test hold every
//!   vendored function within 1 ulp of a 200-bit oracle on the self-test inputs; that the bound
//!   holds for every input is [unverified]), together below `2^-49 M`; `2^-48` doubles it.
//!   For a NURBS surface `2^-40 (p_u + p_v + 2) (w_max / w_min) max|P|inf`: a conservative
//!   enlargement of the stability bound of convex-combination evaluation, not proven for
//!   GEO.NURBS.EVAL's algorithm [unverified].
//! - the distance: `2^-48 (|Q|inf + |S(m)|inf)` (three subtractions and two `hypot`s, each
//!   within 1 ulp by the same KRN.NUM.FN self-test bound [unverified beyond its inputs]).
//! - the uv rounding of `a + (b - a) s` and of `x + k P`: `2^-50 (Lu + Lv)(|a|inf + |b|inf)`.
//! - the period drift `D`: the chart copy `k` of the boundary is `S(x + k P)` while the band
//!   measures the base image; for an analytic surface `P` is the binary64 `TAU`, within `2^-51`
//!   of `2 pi`, and for a NURBS surface the reduction into the domain rounds by at most
//!   `2^-51 |c|`: `D = Lp 2^-51 ((kmax + 1) P + |lo_p| + |hi_p|)`, `kmax` the largest copy
//!   index of the query, `Lp` the Lipschitz bound in `p`. `|k|` is capped at `2^20`
//!   (beyond: [`TrimError::OutsideDomain`]), so `D` stays below `2^-30 Lp P`.
//!
//! **Lipschitz bounds** `Lu >= |Su|`, `Lv >= |Sv|`: plane 1, 1; cylinder `r`, 1; cone
//! `max |r0 + v tan a|` over the piece's end `v`, `hypot(1, tan a)`; sphere `r`, `r`; torus
//! `R + r`, `r`; NURBS `2 p (w_max / w_min) diam / dmin` per direction (`p` the degree, `diam`
//! the diagonal of the control points' bounding box, `dmin` the least positive
//! `knot[i + p] - knot[i]`): `S_u = p sum_i N_{i,p-1} sum_j M_j (w_ij (P_ij - S) -
//! w_{i-1,j} (P_{i-1,j} - S)) / (dknot_i W)` and `S` lies in the control points' hull.
//! `S` is evaluated with non-periodic coordinates clamped into the domain (1-Lipschitz).
//!
//! # Order of checks
//!
//! `tol_m` finite and positive ([`TrimError::BadTolerance`]); `u`, `v` finite, a non-periodic
//! coordinate inside the closed domain, a periodic one within `2^20` periods of the chart
//! ([`TrimError::OutsideDomain`]); a face whose boundary is not known
//! ([`TrimError::Undecidable`]: a failed pcurve, an incomplete loop, a flattening that does not
//! converge, a doubly periodic winding or seam, more than two periods wide); a face with no
//! loops is Inside; then the exact pass, the band, the parity.
//!
//! # Determinism (D0, KNP-1)
//!
//! Binary64 `+ - * /` one operation at a time (no FMA, no `f64::min`/`max`), `floor`,
//! `hypot` from `fid_num`, trigonometry only inside GEO.NURBS; exact integer arithmetic in
//! [`exact`]; vectors and a `BTreeMap` only; every loop bounded. Nothing panics.
//!
//! [`derive_pcurves`]: super::pcurve::derive_pcurves

use core::cmp::Ordering;
use core::f64::consts::FRAC_PI_2;
use core::fmt;
use std::collections::BTreeMap;

use fid_geo_nurbs::{AnalyticSurface, NurbsSurface, Surface};

use super::geom::{Curve2, SurfaceDef};
use super::model::{Body, CoedgeIx, FaceIx, LoopIx, Sense};
use super::pcurve::{
    dist3, floor, grid, hypot, larger, pcurve_point, FaceChart, FailReason, PcurveError, Pcurves, Surf, TrimCurve,
    DEFAULT_TOL_M, MAX_DEPTH,
};

pub mod exact;

use exact::Shifted;

/// Flattening tolerance of imported NURBS pcurves in [`TrimIndex::build`], metres.
pub const DEFAULT_FLATTEN_TOL_M: f64 = DEFAULT_TOL_M / 4.0;
/// Initial chords of a flattened NURBS pcurve.
pub const FLATTEN_INITIAL: usize = 16;
/// Band resolution relative to `tol_m` (`2^-12`).
pub const RESOLUTION_REL: f64 = 1.0 / 4096.0;
/// Inflation of every Lipschitz bound (`1 + 2^-20`).
pub const LIPSCHITZ_INFLATE: f64 = 1.0 + 1.0 / 1_048_576.0;
/// Deepest band bisection.
pub const MAX_BAND_DEPTH: u32 = 64;
/// Most band pieces below the roots examined per classification.
pub const MAX_BAND_PIECES: usize = 1 << 16;
/// Widest face in a periodic direction, in periods.
pub const MAX_EXTENT_PERIODS: f64 = 2.0;
/// Largest chart copy index of a query.
pub const MAX_COPY: i64 = 1 << 20;
/// Distance from `+-pi/2` within which a sphere vertex is put on the pole (`2^-40`).
pub const POLE_SNAP: f64 = 1.0 / 1_099_511_627_776.0;

/// `2^-48`: analytic evaluation and distance guards.
const EVAL_REL: f64 = 1.0 / 281_474_976_710_656.0;
/// `2^-40`: NURBS evaluation guard.
const NURBS_EVAL_REL: f64 = 1.0 / 1_099_511_627_776.0;
/// `2^-50`: uv rounding guard.
const UV_REL: f64 = 1.0 / 1_125_899_906_842_624.0;
/// `2^-51`: period drift.
const DRIFT_REL: f64 = 1.0 / 2_251_799_813_685_248.0;

const DIM: usize = 2;

// ---------------------------------------------------------------- public types

/// Where a parameter lies relative to a trimmed face (REQ-001).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrimClass {
    Inside,
    Outside,
    OnBoundary,
}

impl TrimClass {
    /// One letter: `I`, `O`, `B`.
    pub fn letter(self) -> char {
        match self {
            TrimClass::Inside => 'I',
            TrimClass::Outside => 'O',
            TrimClass::OnBoundary => 'B',
        }
    }
}

/// Why a face's boundary is not known, so no point of it can be classified.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Undecidable {
    /// The face surface is neither analytic nor NURBS, or has no Lipschitz bound.
    NoSurface,
    /// PCURVE could not give this coedge a trimming curve (REQ-GEO.CAD.TOPO.PCURVE-003).
    PcurveFailed { coedge: CoedgeIx, reason: FailReason },
    /// The pcurve result of this coedge is missing or not a curve.
    NoPcurve { coedge: CoedgeIx },
    /// The imported pcurve could not be evaluated at its edge range.
    ImportedEval { coedge: CoedgeIx },
    /// The imported NURBS pcurve did not flatten within `tol_m / 4`.
    Flatten { coedge: CoedgeIx },
    /// A chart vertex is not finite.
    NonFinite { coedge: CoedgeIx },
    /// PCURVE gave the loop no periods (an incomplete loop).
    IncompleteLoop { r#loop: LoopIx },
    /// A loop winds around a doubly periodic surface (e.g. a torus).
    Winding { r#loop: LoopIx, periods: [i64; 2] },
    /// A seam on a doubly periodic surface.
    DoublyPeriodicSeam { coedge: CoedgeIx },
    /// The two uses of a seam coincide and the first does not move along the ray.
    SeamCoincident { coedge: CoedgeIx },
    /// The topmost boundary vertex is a spike, so the far end cannot be decided.
    FarEnd,
    /// The face spans more than [`MAX_EXTENT_PERIODS`] periods.
    ChartExtent,
}

impl Undecidable {
    /// Stable variant name.
    pub fn code(&self) -> &'static str {
        match self {
            Undecidable::NoSurface => "NoSurface",
            Undecidable::PcurveFailed { .. } => "PcurveFailed",
            Undecidable::NoPcurve { .. } => "NoPcurve",
            Undecidable::ImportedEval { .. } => "ImportedEval",
            Undecidable::Flatten { .. } => "Flatten",
            Undecidable::NonFinite { .. } => "NonFinite",
            Undecidable::IncompleteLoop { .. } => "IncompleteLoop",
            Undecidable::Winding { .. } => "Winding",
            Undecidable::DoublyPeriodicSeam { .. } => "DoublyPeriodicSeam",
            Undecidable::SeamCoincident { .. } => "SeamCoincident",
            Undecidable::FarEnd => "FarEnd",
            Undecidable::ChartExtent => "ChartExtent",
        }
    }
}

/// Errors of this leaf.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TrimError {
    /// REQ-003: `tol_m` is not a finite positive number.
    BadTolerance { value: f64 },
    /// REQ-003: the parameter is not finite or lies outside the surface domain.
    OutsideDomain { u: f64, v: f64 },
    /// The face index is not in the body.
    UnknownFace,
    /// The pcurves were not derived from this body.
    Mismatch,
    /// The face's boundary is not known; no answer is guessed.
    Undecidable(Undecidable),
    /// The surface could not be evaluated (or a distance overflowed).
    SurfaceEval,
    /// The band search exceeded [`MAX_BAND_DEPTH`] or [`MAX_BAND_PIECES`].
    Budget,
}

impl TrimError {
    /// Stable variant name.
    pub fn code(&self) -> &'static str {
        match self {
            TrimError::BadTolerance { .. } => "BadTolerance",
            TrimError::OutsideDomain { .. } => "OutsideDomain",
            TrimError::UnknownFace => "UnknownFace",
            TrimError::Mismatch => "Mismatch",
            TrimError::Undecidable(_) => "Undecidable",
            TrimError::SurfaceEval => "SurfaceEval",
            TrimError::Budget => "Budget",
        }
    }
}

impl fmt::Display for TrimError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TrimError::BadTolerance { value } => write!(f, "tolerance {value} is not finite and positive"),
            TrimError::OutsideDomain { u, v } => write!(f, "parameter ({u}, {v}) is outside the surface domain"),
            TrimError::UnknownFace => f.write_str("face is not in the body"),
            TrimError::Mismatch => f.write_str("pcurves were not derived from this body"),
            TrimError::Undecidable(r) => write!(f, "face boundary is not known: {r:?}"),
            TrimError::SurfaceEval => f.write_str("surface evaluation failed"),
            TrimError::Budget => f.write_str("boundary band search exceeded its budget"),
        }
    }
}

impl core::error::Error for TrimError {}

/// A chart vertex: the exact real `x[i] + k[i] P_i` per direction (`k` is zero in a
/// non-periodic direction).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ChartPoint {
    pub x: [f64; DIM],
    pub k: [i64; DIM],
}

impl ChartPoint {
    fn same(&self, o: &ChartPoint) -> bool {
        self.k == o.k && self.x[0].to_bits() == o.x[0].to_bits() && self.x[1].to_bits() == o.x[1].to_bits()
    }

    fn exact(&self, periods: &[Option<f64>; DIM]) -> [Shifted; DIM] {
        core::array::from_fn(|i| Shifted { x: self.x[i], k: self.k[i], p: periods[i].unwrap_or(0.0) })
    }

    fn approx(&self, periods: &[Option<f64>; DIM]) -> [f64; DIM] {
        core::array::from_fn(|i| match periods[i] {
            Some(p) if self.k[i] != 0 => self.x[i] + self.k[i] as f64 * p,
            _ => self.x[i],
        })
    }
}

/// One segment of a trimming polyline in the face chart.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TrimSeg {
    /// Exact ends.
    pub pa: ChartPoint,
    pub pb: ChartPoint,
    /// Rounded ends `x + k P` (band evaluation and bounding boxes).
    pub a: [f64; DIM],
    pub b: [f64; DIM],
    /// Position of the loop in the face's loop list.
    pub loop_ix: usize,
    /// The coedge the segment belongs to.
    pub coedge_ix: CoedgeIx,
    /// A seam or pole connector: crossed, not boundary.
    pub seam: bool,
    /// `[lo, hi]` of the rounded ends.
    pub bbox: [[f64; DIM]; 2],
}

/// How inside and outside are decided.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Rule {
    /// Even-odd with the ray `+u` (no or two periodic directions).
    EvenOdd,
    /// The ray along `n`, periodic direction `p`, far end material or not.
    Column { p: usize, n: usize, far_end: bool },
}

#[derive(Debug, Clone, PartialEq)]
enum Src {
    Poly(Vec<[f64; DIM]>),
    Nurbs { curve: Curve2, range: [f64; 2], reversed: bool },
}

#[derive(Debug, Clone, PartialEq)]
struct CoSrc {
    cx: CoedgeIx,
    edge: usize,
    src: Src,
}

#[derive(Debug, Clone, PartialEq)]
struct LoopSrc {
    offset_k: [i64; DIM],
    wind: [i64; DIM],
    coedges: Vec<CoSrc>,
}

/// The surface image of a piece's midpoint, its `Lu` and half its Lipschitz bound.
type Piece = ([f64; 3], f64, f64);

/// The assembled boundary at one flattening tolerance.
#[derive(Debug, Clone, PartialEq)]
struct Asm {
    segments: Vec<TrimSeg>,
    roots: Vec<Option<Piece>>,
    lo: [f64; DIM],
    hi: [f64; DIM],
    rule: Rule,
}

/// Evaluation-error model of the surface (see the module docs).
#[derive(Debug, Clone, Copy, PartialEq)]
enum ErrModel {
    Plane { o: f64 },
    Cylinder { o: f64, r: f64 },
    Cone { o: f64, r0: f64, ta: f64 },
    Sphere { o: f64, r: f64 },
    Torus { o: f64, big: f64, small: f64 },
    Nurbs { e: f64 },
}

/// The trimming polylines of one face in its chart, ready for [`classify_uv`].
#[derive(Debug, Clone, PartialEq)]
pub struct TrimIndex {
    face: FaceIx,
    surface: SurfaceDef,
    chart: FaceChart,
    reversed: bool,
    has_loops: bool,
    has_nurbs: bool,
    sphere: bool,
    periods: [Option<f64>; DIM],
    domain: [(f64, f64); DIM],
    lip: [f64; DIM],
    /// `(r0, tan a)` of a cone, whose `Lu` depends on `v`.
    cone: Option<(f64, f64)>,
    err: ErrModel,
    loops: Vec<LoopSrc>,
    flat_tol: f64,
    asm: Result<Asm, Undecidable>,
}

// ---------------------------------------------------------------- helpers

fn abs(x: f64) -> f64 {
    x.abs()
}

fn ninf3(p: [f64; 3]) -> f64 {
    larger(larger(abs(p[0]), abs(p[1])), abs(p[2]))
}

fn ninf2(p: [f64; DIM]) -> f64 {
    larger(abs(p[0]), abs(p[1]))
}

fn domain_of(s: Surf<'_>) -> [(f64, f64); DIM] {
    let (du, dv) = match s {
        Surf::A(a) => a.domain(),
        Surf::N(n) => n.domain(),
    };
    [du, dv]
}

/// `S(uv)` with non-periodic coordinates clamped into the domain.
fn eval_clamped(
    s: Surf<'_>,
    periods: &[Option<f64>; DIM],
    domain: &[(f64, f64); DIM],
    uv: [f64; DIM],
) -> Result<[f64; 3], TrimError> {
    let mut c = uv;
    for (i, ci) in c.iter_mut().enumerate() {
        if periods[i].is_none() {
            let (lo, hi) = domain[i];
            if *ci < lo {
                *ci = lo;
            } else if *ci > hi {
                *ci = hi;
            }
        }
    }
    s.point(c).map_err(|_| TrimError::SurfaceEval)
}

fn nurbs_lip(n: &NurbsSurface) -> Option<([f64; DIM], f64)> {
    let ctrl = n.ctrl();
    let first = ctrl.first()?;
    let (mut lo, mut hi) = (*first, *first);
    let mut big = 0.0;
    for p in ctrl {
        for i in 0..3 {
            if p[i] < lo[i] {
                lo[i] = p[i];
            }
            if p[i] > hi[i] {
                hi[i] = p[i];
            }
        }
        big = larger(big, ninf3(*p));
    }
    let diam = dist3(hi, lo)?;
    let ratio = match n.weights() {
        None => 1.0,
        Some(w) => {
            let w0 = *w.first()?;
            let (mut wmin, mut wmax) = (w0, w0);
            for &x in w {
                if x.is_nan() || x <= 0.0 {
                    return None;
                }
                if x < wmin {
                    wmin = x;
                }
                if x > wmax {
                    wmax = x;
                }
            }
            wmax / wmin
        }
    };
    let dir = |knots: &[f64], p: usize| -> Option<f64> {
        if p == 0 {
            return Some(0.0);
        }
        let mut dmin: Option<f64> = None;
        for (i, &k) in knots.iter().enumerate() {
            let Some(&kp) = knots.get(i + p) else { break };
            let d = kp - k;
            if d > 0.0 && dmin.is_none_or(|m| d < m) {
                dmin = Some(d);
            }
        }
        let l = ((2.0 * p as f64) * ratio) * diam / dmin?;
        l.is_finite().then_some(l)
    };
    let lip = [dir(n.knots_u().as_slice(), n.degree_u())?, dir(n.knots_v().as_slice(), n.degree_v())?];
    let e = NURBS_EVAL_REL * ((n.degree_u() + n.degree_v() + 2) as f64 * ratio) * big;
    e.is_finite().then_some((lip, e))
}

/// Lipschitz bounds `[Lu, Lv]`, the cone parameters and the error model.
type SurfModel = ([f64; DIM], Option<(f64, f64)>, ErrModel);

fn surface_model(s: Surf<'_>) -> Option<SurfModel> {
    let o = |pl: &fid_geo_nurbs::Placement| ninf3(pl.origin());
    match s {
        Surf::A(a) => match a {
            AnalyticSurface::Plane(p) => Some(([1.0, 1.0], None, ErrModel::Plane { o: o(p.placement()) })),
            AnalyticSurface::Cylinder(c) => {
                Some(([c.radius(), 1.0], None, ErrModel::Cylinder { o: o(c.placement()), r: c.radius() }))
            }
            AnalyticSurface::Cone(c) => {
                let ta = fid_num::f::tan(c.half_angle()).ok()?;
                let err = ErrModel::Cone { o: o(c.placement()), r0: c.radius(), ta };
                Some(([0.0, hypot(1.0, ta)?], Some((c.radius(), ta)), err))
            }
            AnalyticSurface::Sphere(sp) => {
                Some(([sp.radius(), sp.radius()], None, ErrModel::Sphere { o: o(sp.placement()), r: sp.radius() }))
            }
            AnalyticSurface::Torus(t) => {
                let (big, small) = t.radii();
                Some(([big + small, small], None, ErrModel::Torus { o: o(t.placement()), big, small }))
            }
        },
        Surf::N(n) => nurbs_lip(n).map(|(l, e)| (l, None, ErrModel::Nurbs { e })),
    }
}

impl ErrModel {
    /// Bound on the evaluation error of `S(uv)`, metres.
    fn at(&self, uv: [f64; DIM]) -> f64 {
        let m = match *self {
            ErrModel::Plane { o } => o + (abs(uv[0]) + abs(uv[1])),
            ErrModel::Cylinder { o, r } => o + (2.0 * r + abs(uv[1])),
            ErrModel::Cone { o, r0, ta } => o + (2.0 * abs(r0 + uv[1] * ta) + abs(uv[1])),
            ErrModel::Sphere { o, r } => o + 3.0 * r,
            ErrModel::Torus { o, big, small } => o + (2.0 * (big + small) + small),
            ErrModel::Nurbs { e } => return e,
        };
        EVAL_REL * m
    }
}

// ---------------------------------------------------------------- sources and assembly

/// Flattens one imported NURBS pcurve in its coedge's direction at `flat` metres.
#[allow(clippy::too_many_arguments)]
fn flatten(
    s: Surf<'_>,
    periods: &[Option<f64>; DIM],
    domain: &[(f64, f64); DIM],
    p: &Curve2,
    range: [f64; 2],
    reversed: bool,
    coedge: CoedgeIx,
    flat: f64,
) -> Result<Vec<[f64; DIM]>, Undecidable> {
    let at = |t: f64| pcurve_point(p, t).map_err(|_| Undecidable::ImportedEval { coedge });
    let fail = Undecidable::Flatten { coedge };
    let n = FLATTEN_INITIAL;
    let mut stack: Vec<(f64, [f64; DIM], u32)> = Vec::with_capacity(n + 2 * MAX_DEPTH as usize + 1);
    // Push right to left so the left-most chord is processed first.
    for k in 0..n {
        let j = if reversed { k } else { n - k };
        let t = grid(range[0], range[1], j, n);
        stack.push((t, at(t)?, 0));
    }
    let j0 = if reversed { n } else { 0 };
    let t0 = grid(range[0], range[1], j0, n);
    let mut left = (t0, at(t0)?);
    let mut out = vec![left.1];
    while let Some((tr, uvr, depth)) = stack.pop() {
        let tm = (left.0 + tr) * 0.5;
        let mid = [(left.1[0] + uvr[0]) * 0.5, (left.1[1] + uvr[1]) * 0.5];
        let on = eval_clamped(s, periods, domain, at(tm)?).map_err(|_| fail)?;
        let chord = eval_clamped(s, periods, domain, mid).map_err(|_| fail)?;
        let d = dist3(on, chord).ok_or(fail)?;
        if d <= flat {
            out.push(uvr);
            left = (tr, uvr);
            continue;
        }
        if depth >= MAX_DEPTH || tm == left.0 || tm == tr {
            return Err(fail);
        }
        stack.push((tr, uvr, depth + 1));
        stack.push((tm, at(tm)?, depth + 1));
    }
    Ok(out)
}

/// One coedge's trimming source.
fn coedge_source(body: &Body, pcurves: &Pcurves, cx: CoedgeIx) -> Result<CoSrc, Undecidable> {
    let missing = Undecidable::NoPcurve { coedge: cx };
    let ce = body.coedge(cx).ok_or(missing)?;
    let edge = body.edge(ce.edge()).ok_or(missing)?;
    let reversed = ce.sense() == Sense::Reversed;
    let src = match pcurves.curves.get(cx.index()).ok_or(missing)? {
        Ok(TrimCurve::Derived { uv, .. }) => Src::Poly(uv.clone()),
        Ok(TrimCurve::Imported(p)) => {
            let range = edge.param_range();
            if p.as_line().is_some() {
                let at = |t: f64| pcurve_point(p, t).map_err(|_| Undecidable::ImportedEval { coedge: cx });
                let (a, b) = (at(range[0])?, at(range[1])?);
                Src::Poly(if reversed { vec![b, a] } else { vec![a, b] })
            } else {
                Src::Nurbs { curve: p.clone(), range, reversed }
            }
        }
        Err(PcurveError::PcurveFailed { reason, .. }) => {
            return Err(Undecidable::PcurveFailed { coedge: cx, reason: *reason })
        }
        Err(_) => return Err(missing),
    };
    Ok(CoSrc { cx, edge: ce.edge().index(), src })
}

/// A coedge's chart vertices with its coedge, edge and seam flag.
struct Part {
    pts: Vec<ChartPoint>,
    cx: CoedgeIx,
    edge: usize,
    seam: bool,
}

/// Whether every vertex lies on a period line `lo + j P` (within `2^-40 P`) in each periodic
/// direction, `lo` the domain start.
fn on_period_line(
    pts: &[ChartPoint],
    per: &[Option<f64>; DIM],
    domain: &[(f64, f64); DIM],
    periodic: &[usize],
) -> bool {
    pts.iter().all(|q| {
        let a = q.approx(per);
        periodic.iter().all(|&i| match per[i] {
            Some(p) => {
                let r = (a[i] - domain[i].0) / p;
                abs(r - floor(r + 0.5)) <= POLE_SNAP
            }
            None => true,
        })
    })
}

/// Collects segments and their bounding box.
struct Emit {
    segs: Vec<TrimSeg>,
    lo: [f64; DIM],
    hi: [f64; DIM],
    per: [Option<f64>; DIM],
}

impl Emit {
    fn push(&mut self, pa: ChartPoint, pb: ChartPoint, j: usize, cx: CoedgeIx, seam: bool) {
        if pa.same(&pb) {
            return;
        }
        let (a, b) = (pa.approx(&self.per), pb.approx(&self.per));
        let mut bbox = [a, a];
        let first = self.segs.is_empty();
        for i in 0..DIM {
            if b[i] < bbox[0][i] {
                bbox[0][i] = b[i];
            }
            if b[i] > bbox[1][i] {
                bbox[1][i] = b[i];
            }
            if first || bbox[0][i] < self.lo[i] {
                self.lo[i] = bbox[0][i];
            }
            if first || bbox[1][i] > self.hi[i] {
                self.hi[i] = bbox[1][i];
            }
        }
        self.segs.push(TrimSeg { pa, pb, a, b, loop_ix: j, coedge_ix: cx, seam, bbox });
    }
}

/// `floor(x + 0.5)` as an integer, `None` when out of range.
fn round_k(x: f64) -> Option<i64> {
    let k = floor(x + 0.5);
    (k.is_finite() && abs(k) <= 4.0 * MAX_COPY as f64).then_some(k as i64)
}

impl TrimIndex {
    /// Builds the index of `face` from the pcurves [`derive_pcurves`] gave for `body`.
    /// A face whose boundary is not known still builds; [`classify_uv`] then returns
    /// [`TrimError::Undecidable`].
    ///
    /// [`derive_pcurves`]: super::pcurve::derive_pcurves
    pub fn build(body: &Body, pcurves: &Pcurves, face: FaceIx) -> Result<TrimIndex, TrimError> {
        let f = body.face(face).ok_or(TrimError::UnknownFace)?;
        if pcurves.curves.len() != body.coedges().len() || pcurves.charts.len() != body.faces().len() {
            return Err(TrimError::Mismatch);
        }
        let chart = pcurves.charts.get(face.index()).ok_or(TrimError::Mismatch)?.clone();
        let nloops = f.loops().len();
        if chart.loop_offsets.len() != nloops || chart.loop_periods.len() != nloops {
            return Err(TrimError::Mismatch);
        }
        let mut idx = TrimIndex {
            face,
            surface: f.surface().clone(),
            chart,
            reversed: f.sense() == Sense::Reversed,
            has_loops: nloops > 0,
            has_nurbs: false,
            sphere: matches!(f.surface().as_analytic(), Some(AnalyticSurface::Sphere(_))),
            periods: [None, None],
            domain: [(0.0, 0.0); DIM],
            lip: [0.0; DIM],
            cone: None,
            err: ErrModel::Nurbs { e: 0.0 },
            loops: Vec::new(),
            flat_tol: DEFAULT_FLATTEN_TOL_M,
            asm: Err(Undecidable::NoSurface),
        };
        let Some(s) = Surf::of(f.surface()) else {
            return Ok(idx);
        };
        let Some((lip, cone, err)) = surface_model(s) else {
            return Ok(idx);
        };
        idx.periods = s.periods();
        idx.domain = domain_of(s);
        idx.lip = lip;
        idx.cone = cone;
        idx.err = err;
        match idx.sources(body, pcurves) {
            Ok(loops) => {
                idx.has_nurbs = loops.iter().any(|l| l.coedges.iter().any(|c| matches!(c.src, Src::Nurbs { .. })));
                idx.loops = loops;
                idx.asm = idx.assemble(s, DEFAULT_FLATTEN_TOL_M);
            }
            Err(u) => idx.asm = Err(u),
        }
        Ok(idx)
    }

    fn sources(&self, body: &Body, pcurves: &Pcurves) -> Result<Vec<LoopSrc>, Undecidable> {
        let f = body.face(self.face).ok_or(Undecidable::NoSurface)?;
        let mut out = Vec::with_capacity(f.loops().len());
        for (j, &lx) in f.loops().iter().enumerate() {
            let lp = body.r#loop(lx).ok_or(Undecidable::IncompleteLoop { r#loop: lx })?;
            let mut coedges = Vec::with_capacity(lp.coedges().len());
            for &cx in lp.coedges() {
                coedges.push(coedge_source(body, pcurves, cx)?);
            }
            let wind =
                self.chart.loop_periods.get(j).copied().flatten().ok_or(Undecidable::IncompleteLoop { r#loop: lx })?;
            let off = self.chart.loop_offsets.get(j).copied().unwrap_or([0.0; DIM]);
            let mut offset_k = [0i64; DIM];
            for i in 0..DIM {
                if let Some(p) = self.periods[i] {
                    offset_k[i] = round_k(off[i] / p).ok_or(Undecidable::ChartExtent)?;
                }
            }
            let periodic = self.periods.iter().filter(|p| p.is_some()).count();
            if periodic == 2 && wind != [0, 0] {
                return Err(Undecidable::Winding { r#loop: lx, periods: wind });
            }
            out.push(LoopSrc { offset_k, wind, coedges });
        }
        Ok(out)
    }

    /// Assembles the boundary of record with NURBS pcurves flattened at `flat` metres.
    fn assemble(&self, s: Surf<'_>, flat: f64) -> Result<Asm, Undecidable> {
        let per = self.periods;
        let periodic: Vec<usize> = (0..DIM).filter(|&i| per[i].is_some()).collect();
        // 1. Chart vertices per coedge.
        let mut parts: Vec<Vec<Part>> = Vec::with_capacity(self.loops.len());
        for lp in &self.loops {
            let mut row: Vec<Part> = Vec::with_capacity(lp.coedges.len());
            let mut prev_end: Option<[f64; DIM]> = None;
            for c in &lp.coedges {
                let mut base = match &c.src {
                    Src::Poly(v) => v.clone(),
                    Src::Nurbs { curve, range, reversed } => {
                        flatten(s, &per, &self.domain, curve, *range, *reversed, c.cx, flat)?
                    }
                };
                if self.sphere {
                    for q in base.iter_mut() {
                        for pole in [FRAC_PI_2, -FRAC_PI_2] {
                            if abs(q[1] - pole) <= POLE_SNAP {
                                q[1] = pole;
                            }
                        }
                    }
                }
                let mut kc = [0i64; DIM];
                if let (Some(e), Some(st)) = (prev_end, base.first()) {
                    for &i in &periodic {
                        if let Some(p) = per[i] {
                            kc[i] = 0 - round_k((st[i] - e[i]) / p).ok_or(Undecidable::ChartExtent)?;
                        }
                    }
                }
                let mut pts = Vec::with_capacity(base.len());
                for q in &base {
                    if !(q[0].is_finite() && q[1].is_finite()) {
                        return Err(Undecidable::NonFinite { coedge: c.cx });
                    }
                    pts.push(ChartPoint { x: *q, k: [kc[0] + lp.offset_k[0], kc[1] + lp.offset_k[1]] });
                }
                // The continuity reference excludes the loop offset, like the shift itself.
                prev_end = pts.last().map(|p| ChartPoint { x: p.x, k: kc }.approx(&per));
                row.push(Part { pts, cx: c.cx, edge: c.edge, seam: false });
            }
            parts.push(row);
        }
        // 2. Seams.
        if !periodic.is_empty() {
            let mut uses: BTreeMap<usize, Vec<(usize, usize)>> = BTreeMap::new();
            for (j, row) in parts.iter().enumerate() {
                for (k, part) in row.iter().enumerate() {
                    uses.entry(part.edge).or_default().push((j, k));
                }
            }
            for list in uses.values() {
                let [(ja, ka), (jb, kb)] = list.as_slice() else { continue };
                let (Some(pa), Some(pb)) =
                    (parts.get(*ja).and_then(|r| r.get(*ka)), parts.get(*jb).and_then(|r| r.get(*kb)))
                else {
                    continue;
                };
                let (Some(a0), Some(a1), Some(b1)) = (pa.pts.first(), pa.pts.last(), pb.pts.last()) else { continue };
                let (a0, a1, b1) = (a0.approx(&per), a1.approx(&per), b1.approx(&per));
                let mut apart = false;
                for &i in &periodic {
                    if let Some(p) = per[i] {
                        apart |= round_k((a0[i] - b1[i]) / p).ok_or(Undecidable::ChartExtent)? != 0;
                    }
                }
                // Coincident uses (k = 0) are a seam only when the edge lies on a period line
                // (every vertex of the first use within 2^-40 P of the domain start plus whole
                // periods): otherwise they are a slit, ordinary boundary on both uses.
                if !apart && !on_period_line(&pa.pts, &per, &self.domain, &periodic) {
                    continue;
                }
                let lift = if apart {
                    0
                } else {
                    let [pi] = periodic.as_slice() else {
                        return Err(Undecidable::DoublyPeriodicSeam { coedge: pb.cx });
                    };
                    let n = 1 - *pi;
                    let dn = a1[n] - a0[n];
                    if dn == 0.0 {
                        return Err(Undecidable::SeamCoincident { coedge: pb.cx });
                    }
                    // Material on the left of the first use: towards -u when it runs +v (p = u),
                    // towards +v when it runs +u (p = v); mirrored on a reversed face.
                    let towards_plus = if *pi == 0 { dn < 0.0 } else { dn > 0.0 };
                    if towards_plus != self.reversed {
                        1
                    } else {
                        -1
                    }
                };
                if periodic.len() == 2 {
                    return Err(Undecidable::DoublyPeriodicSeam { coedge: pb.cx });
                }
                if let Some(part) = parts.get_mut(*jb).and_then(|r| r.get_mut(*kb)) {
                    part.seam = true;
                    if lift != 0 {
                        if let Some(&pi) = periodic.first() {
                            for q in part.pts.iter_mut() {
                                q.k[pi] += lift;
                            }
                        }
                    }
                }
                if let Some(part) = parts.get_mut(*ja).and_then(|r| r.get_mut(*ka)) {
                    part.seam = true;
                }
            }
        }
        // 3. Segments.
        let mut em = Emit { segs: Vec::new(), lo: [0.0; DIM], hi: [0.0; DIM], per };
        let mut ranges: Vec<(usize, usize)> = Vec::with_capacity(parts.len());
        for (j, row) in parts.iter().enumerate() {
            let start = em.segs.len();
            let n = row.len();
            let wind = self.loops.get(j).map(|l| l.wind).unwrap_or([0, 0]);
            for (k, part) in row.iter().enumerate() {
                for w in part.pts.windows(2) {
                    if let [a, b] = w {
                        em.push(*a, *b, j, part.cx, part.seam);
                    }
                }
                let next = row.get((k + 1) % n);
                if let (Some(end), Some(nx)) = (part.pts.last(), next) {
                    if let Some(st) = nx.pts.first() {
                        let mut target = *st;
                        if k + 1 == n {
                            for i in 0..DIM {
                                target.k[i] += wind[i];
                            }
                        }
                        em.push(*end, target, j, nx.cx, part.seam || nx.seam);
                    }
                }
            }
            ranges.push((start, em.segs.len()));
        }
        let Emit { segs, lo, hi, .. } = em;
        for &i in &periodic {
            if let Some(p) = per[i] {
                if !segs.is_empty() && hi[i] - lo[i] > MAX_EXTENT_PERIODS * p {
                    return Err(Undecidable::ChartExtent);
                }
            }
        }
        // 4. The rule.
        let rule = match periodic.as_slice() {
            [p] => {
                let (p, n) = (*p, 1 - *p);
                Rule::Column { p, n, far_end: far_end(&segs, &ranges, &per, p, n, self.reversed)? }
            }
            _ => Rule::EvenOdd,
        };
        let roots = segs.iter().map(|g| piece(self, s, g, 0.0, 1.0).ok()).collect();
        Ok(Asm { segments: segs, roots, lo, hi, rule })
    }

    /// The face this index belongs to.
    pub fn face(&self) -> FaceIx {
        self.face
    }

    /// The trimming segments in the chart, loop by loop (as built, at
    /// [`DEFAULT_FLATTEN_TOL_M`]); empty when the face is undecidable.
    pub fn segments(&self) -> &[TrimSeg] {
        match &self.asm {
            Ok(a) => &a.segments,
            Err(_) => &[],
        }
    }

    /// The inside/outside rule of the face, when it is decidable.
    pub fn rule(&self) -> Option<Rule> {
        self.asm.as_ref().ok().map(|a| a.rule)
    }

    /// The face chart from PCURVE.
    pub fn chart(&self) -> &FaceChart {
        &self.chart
    }

    /// Why the face cannot be classified, if it cannot.
    pub fn undecidable(&self) -> Option<Undecidable> {
        self.asm.as_ref().err().copied()
    }
}

/// Whether the far end of the ray (greatest `n`) is material (see the module docs).
fn far_end(
    segs: &[TrimSeg],
    ranges: &[(usize, usize)],
    per: &[Option<f64>; DIM],
    p: usize,
    n: usize,
    reversed: bool,
) -> Result<bool, Undecidable> {
    let mut top: Option<f64> = None;
    for g in segs {
        for c in [g.pa.x[n], g.pb.x[n]] {
            if top.is_none_or(|t| c > t) {
                top = Some(c);
            }
        }
    }
    let Some(top) = top else { return Err(Undecidable::FarEnd) };
    // Left of a segment running +p is +n when p = u (n = v), -n when p = v (n = u).
    let left_up = |dir_plus: bool| -> bool { dir_plus == (p == 0) };
    for g in segs {
        if g.pa.x[n] == top && g.pb.x[n] == top {
            let o = exact::cmp(g.pb.exact(per)[p], g.pa.exact(per)[p]).ok_or(Undecidable::FarEnd)?;
            return match o {
                Ordering::Equal => Err(Undecidable::FarEnd),
                o => Ok(left_up(o == Ordering::Greater) != reversed),
            };
        }
    }
    for &(start, end) in ranges {
        for k in start..end {
            let Some(g) = segs.get(k) else { continue };
            if g.pb.x[n] != top {
                continue;
            }
            let next = if k + 1 < end { k + 1 } else { start };
            let Some(h) = segs.get(next) else { continue };
            let o = exact::orient(g.pa.exact(per), g.pb.exact(per), h.pb.exact(per)).ok_or(Undecidable::FarEnd)?;
            // In (u, v) order a left turn at a top vertex has the material below it.
            return match o {
                Ordering::Equal => Err(Undecidable::FarEnd),
                o => Ok((o == Ordering::Less) != reversed),
            };
        }
    }
    Err(Undecidable::FarEnd)
}

// ---------------------------------------------------------------- classification

/// Classifies `(u, v)` against the trimmed face of `index` with boundary band `tol_m` metres
/// (REQ-001 to REQ-005). See the module documentation for the pinned rules.
pub fn classify_uv(index: &TrimIndex, u: f64, v: f64, tol_m: f64) -> Result<TrimClass, TrimError> {
    if !(tol_m.is_finite() && tol_m > 0.0) {
        return Err(TrimError::BadTolerance { value: tol_m });
    }
    let q = [u, v];
    let outside = TrimError::OutsideDomain { u, v };
    let base = index.asm.as_ref().ok();
    for i in 0..DIM {
        if !q[i].is_finite() {
            return Err(outside);
        }
        match index.periods[i] {
            None => {
                let (lo, hi) = index.domain[i];
                if !(lo..=hi).contains(&q[i]) {
                    return Err(outside);
                }
            }
            Some(p) => {
                let lo = base.map(|a| a.lo[i]).unwrap_or(0.0);
                let k = (q[i] - lo) / p;
                if !(k.is_finite() && abs(k) <= MAX_COPY as f64) {
                    return Err(outside);
                }
            }
        }
    }
    if let Err(r) = &index.asm {
        return Err(TrimError::Undecidable(*r));
    }
    if !index.has_loops {
        return Ok(TrimClass::Inside);
    }
    let Some(s) = Surf::of(&index.surface) else {
        return Err(TrimError::Undecidable(Undecidable::NoSurface));
    };
    let flat = tol_m * 0.25;
    let tmp;
    let asm = match &index.asm {
        Ok(a) if !index.has_nurbs || flat == index.flat_tol => a,
        _ => {
            tmp = index.assemble(s, flat).map_err(TrimError::Undecidable)?;
            &tmp
        }
    };

    // The exact pass over the chart copies.
    let mut ranges = [(0i64, 0i64); DIM];
    let mut kmax = 0i64;
    for i in 0..DIM {
        if let Some(p) = index.periods[i] {
            let lo = floor((q[i] - asm.hi[i]) / p) as i64 - 1;
            let hi = floor((q[i] - asm.lo[i]) / p) as i64 + 1;
            ranges[i] = (lo, hi);
            kmax = kmax.max(lo.abs()).max(hi.abs());
        }
    }
    let mut parity = false;
    let mut crossings = 0u64;
    let mut on_seam = false;
    for ku in ranges[0].0..=ranges[0].1 {
        for kv in ranges[1].0..=ranges[1].1 {
            let qs = [
                Shifted { x: u, k: 0 - ku, p: index.periods[0].unwrap_or(0.0) },
                Shifted { x: v, k: 0 - kv, p: index.periods[1].unwrap_or(0.0) },
            ];
            match asm.rule {
                Rule::EvenOdd => match even_odd(index, asm, qs)? {
                    Pass::OnBoundary => return Ok(TrimClass::OnBoundary),
                    Pass::Count(odd, _) => parity |= odd,
                },
                Rule::Column { p, n, .. } => match column(index, asm, qs, p, n)? {
                    Pass::OnBoundary => return Ok(TrimClass::OnBoundary),
                    Pass::Count(_, c) => {
                        crossings += c.0;
                        on_seam |= c.1;
                    }
                },
            }
        }
    }

    // The band.
    let big_q = eval_clamped(s, &index.periods, &index.domain, q)?;
    let drift = drift(index, asm, q, kmax);
    let mut pieces = 0usize;
    for (seg, root) in asm.segments.iter().zip(asm.roots.iter()) {
        if seg.seam {
            continue;
        }
        if band(index, s, big_q, q, seg, *root, tol_m, drift, &mut pieces)? {
            return Ok(TrimClass::OnBoundary);
        }
    }
    let inside = match asm.rule {
        Rule::EvenOdd => parity,
        Rule::Column { far_end, .. } => on_seam || (far_end != (crossings % 2 == 1)),
    };
    Ok(if inside { TrimClass::Inside } else { TrimClass::Outside })
}

/// The period drift `D` (module docs).
fn drift(index: &TrimIndex, asm: &Asm, q: [f64; DIM], kmax: i64) -> f64 {
    let mut d = 0.0;
    for i in 0..DIM {
        if let Some(p) = index.periods[i] {
            let lp = match (i, index.cone) {
                (0, Some((r0, ta))) => {
                    let n = 1;
                    let a = larger(abs(r0 + asm.lo[n] * ta), abs(r0 + asm.hi[n] * ta));
                    larger(a, abs(r0 + q[n] * ta))
                }
                _ => index.lip[i],
            };
            d += (lp * LIPSCHITZ_INFLATE) * DRIFT_REL * (((kmax + 1) as f64 * p + abs(asm.lo[i])) + abs(asm.hi[i]));
        }
    }
    d
}

enum Pass {
    OnBoundary,
    /// Even-odd parity, or (crossings above, on a seam) for the column rule.
    Count(bool, (u64, bool)),
}

fn ord(a: Shifted, b: Shifted) -> Result<Ordering, TrimError> {
    exact::cmp(a, b).ok_or(TrimError::SurfaceEval)
}

fn is_at(p: [Shifted; DIM], q: [Shifted; DIM]) -> Result<bool, TrimError> {
    Ok(ord(p[0], q[0])? == Ordering::Equal && ord(p[1], q[1])? == Ordering::Equal)
}

/// On a segment that is constant in `c` (`a[c] == b[c] == q[c]`) between its ends in `1 - c`?
fn on_aligned(a: [Shifted; DIM], b: [Shifted; DIM], q: [Shifted; DIM], c: usize) -> Result<bool, TrimError> {
    let o = 1 - c;
    if ord(a[c], b[c])? != Ordering::Equal || ord(a[c], q[c])? != Ordering::Equal {
        return Ok(false);
    }
    let lo_ok = ord(a[o], q[o])? != Ordering::Greater || ord(b[o], q[o])? != Ordering::Greater;
    let hi_ok = ord(a[o], q[o])? != Ordering::Less || ord(b[o], q[o])? != Ordering::Less;
    Ok(lo_ok && hi_ok)
}

/// Even-odd with the ray `+u` for one chart copy.
fn even_odd(index: &TrimIndex, asm: &Asm, q: [Shifted; DIM]) -> Result<Pass, TrimError> {
    let per = &index.periods;
    let mut odd = false;
    let plain_q = q[0].k == 0 && q[1].k == 0;
    for seg in &asm.segments {
        // Quick rejection: with no period shifts the box is exact.
        if plain_q && seg.pa.k == [0, 0] && seg.pb.k == [0, 0] {
            let (vq, uq) = (q[1].x, q[0].x);
            if vq < seg.bbox[0][1] || vq > seg.bbox[1][1] || uq > seg.bbox[1][0] {
                continue;
            }
        }
        let (a, b) = (seg.pa.exact(per), seg.pb.exact(per));
        let ga = ord(a[1], q[1])? != Ordering::Less;
        let gb = ord(b[1], q[1])? != Ordering::Less;
        if !seg.seam && (is_at(a, q)? || is_at(b, q)? || on_aligned(a, b, q, 1)?) {
            return Ok(Pass::OnBoundary);
        }
        if ga == gb {
            continue;
        }
        let o = exact::orient(a, b, q).ok_or(TrimError::SurfaceEval)?;
        if o == Ordering::Equal {
            if !seg.seam {
                return Ok(Pass::OnBoundary);
            }
            odd = !odd;
            continue;
        }
        if (gb && o == Ordering::Greater) || (!gb && o == Ordering::Less) {
            odd = !odd;
        }
    }
    Ok(Pass::Count(odd, (0, false)))
}

/// The column rule for one chart copy: crossings above `q` along `n`.
fn column(index: &TrimIndex, asm: &Asm, q: [Shifted; DIM], p: usize, n: usize) -> Result<Pass, TrimError> {
    let per = &index.periods;
    let mut count = 0u64;
    let mut on_seam = false;
    let plain_q = q[0].k == 0 && q[1].k == 0;
    for seg in &asm.segments {
        // Quick rejection: with no period shifts the box is exact.
        if plain_q && seg.pa.k == [0, 0] && seg.pb.k == [0, 0] {
            let (pq, nq) = (q[p].x, q[n].x);
            if pq < seg.bbox[0][p] || pq > seg.bbox[1][p] || nq > seg.bbox[1][n] {
                continue;
            }
        }
        let (a, b) = (seg.pa.exact(per), seg.pb.exact(per));
        let ga = ord(a[p], q[p])? != Ordering::Less;
        let gb = ord(b[p], q[p])? != Ordering::Less;
        let touch = is_at(a, q)? || is_at(b, q)? || on_aligned(a, b, q, p)?;
        if touch {
            if seg.seam {
                on_seam = true;
            } else {
                return Ok(Pass::OnBoundary);
            }
        }
        if ga == gb {
            continue;
        }
        let o = exact::orient(a, b, q).ok_or(TrimError::SurfaceEval)?;
        if o == Ordering::Equal {
            if seg.seam {
                on_seam = true;
                continue;
            }
            return Ok(Pass::OnBoundary);
        }
        // gb: the segment runs +p. With p = u, n = v, the crossing is above q when q lies right
        // of a +u segment (o < 0) or left of a -u one; with p = v, n = u, the mirror.
        let plus = gb;
        let above = if p == 0 { (o == Ordering::Less) == plus } else { (o == Ordering::Greater) == plus };
        if above {
            count += 1;
        }
    }
    Ok(Pass::Count(false, (count, on_seam)))
}

fn at(seg: &TrimSeg, s: f64) -> [f64; DIM] {
    [seg.a[0] + (seg.b[0] - seg.a[0]) * s, seg.a[1] + (seg.b[1] - seg.a[1]) * s]
}

fn piece(index: &TrimIndex, s: Surf<'_>, seg: &TrimSeg, s0: f64, s1: f64) -> Result<Piece, TrimError> {
    let du = abs(seg.b[0] - seg.a[0]);
    let dv = abs(seg.b[1] - seg.a[1]);
    let w = s1 - s0;
    let lu = match index.cone {
        Some((r0, ta)) => {
            let (v0, v1) = (at(seg, s0)[1], at(seg, s1)[1]);
            larger(abs(r0 + v0 * ta), abs(r0 + v1 * ta))
        }
        None => index.lip[0],
    };
    let lv = index.lip[1];
    let half = (((lu * du) * w + (lv * dv) * w) * 0.5) * LIPSCHITZ_INFLATE;
    let sm = eval_clamped(s, &index.periods, &index.domain, at(seg, (s0 + s1) * 0.5))?;
    Ok((sm, lu, half))
}

/// Is some point of the segment's surface image within `tol` of `big_q`? `root` is the
/// cached [`piece`] of `[0, 1]`.
#[allow(clippy::too_many_arguments)]
fn band(
    index: &TrimIndex,
    s: Surf<'_>,
    big_q: [f64; 3],
    q: [f64; DIM],
    seg: &TrimSeg,
    root: Option<Piece>,
    tol: f64,
    drift: f64,
    pieces: &mut usize,
) -> Result<bool, TrimError> {
    let rho = tol * RESOLUTION_REL;
    let lv = index.lip[1];
    let coord = ninf2(seg.a) + ninf2(seg.b);
    let eq = index.err.at(q);
    let mut stack: Vec<(f64, f64, u32)> = vec![(0.0, 1.0, 0)];
    while let Some((s0, s1, depth)) = stack.pop() {
        let (sm, lu, half) = match (depth, root) {
            (0, Some(r)) => r,
            _ => piece(index, s, seg, s0, s1)?,
        };
        let d = dist3(big_q, sm).ok_or(TrimError::SurfaceEval)?;
        let m = at(seg, (s0 + s1) * 0.5);
        let guard =
            (((eq + index.err.at(m)) + EVAL_REL * (ninf3(big_q) + ninf3(sm))) + UV_REL * ((lu + lv) * coord)) + drift;
        if !(half.is_finite() && guard.is_finite()) {
            return Err(TrimError::SurfaceEval);
        }
        if d + guard <= tol {
            return Ok(true);
        }
        if (d - half) - guard > tol {
            continue;
        }
        if half <= rho * 0.5 {
            return Ok(true);
        }
        if depth >= MAX_BAND_DEPTH {
            return Err(TrimError::Budget);
        }
        *pieces += 2;
        if *pieces > MAX_BAND_PIECES {
            return Err(TrimError::Budget);
        }
        let mid = (s0 + s1) * 0.5;
        stack.push((mid, s1, depth + 1));
        stack.push((s0, mid, depth + 1));
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    #[test]
    fn constants() {
        assert_eq!(DEFAULT_FLATTEN_TOL_M, 2.5e-9);
        assert_eq!(RESOLUTION_REL, 2f64.powi(-12));
        assert_eq!(LIPSCHITZ_INFLATE, 1.0 + 2f64.powi(-20));
        assert_eq!(POLE_SNAP, 2f64.powi(-40));
        assert_eq!(EVAL_REL, 2f64.powi(-48));
        assert_eq!(NURBS_EVAL_REL, 2f64.powi(-40));
        assert_eq!(UV_REL, 2f64.powi(-50));
        assert_eq!(DRIFT_REL, 2f64.powi(-51));
        // binary64 TAU is within 2^-51 of 2 pi (TAU - 2 pi = -2.449e-16).
        assert!(core::hint::black_box(2.449_293_598_294_706_4e-16) < DRIFT_REL);
    }

    #[test]
    fn letters() {
        assert_eq!(TrimClass::Inside.letter(), 'I');
        assert_eq!(TrimClass::Outside.letter(), 'O');
        assert_eq!(TrimClass::OnBoundary.letter(), 'B');
    }
}
