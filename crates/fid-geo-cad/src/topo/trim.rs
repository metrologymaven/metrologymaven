//! Trimmed-face point classification. Spec node `GEO.CAD.TOPO.TRIM`.
//!
//! [`TrimIndex::build`] turns the trimming curves of one face (from [`derive_pcurves`], spec
//! node GEO.CAD.TOPO.PCURVE) into closed uv polylines in the face's parameter chart;
//! [`classify_uv`] says whether a parameter `(u, v)` is [`TrimClass::Inside`],
//! [`TrimClass::Outside`] or [`TrimClass::OnBoundary`] of the face, with the boundary band
//! `tol_m` given in metres (REQ-001 to REQ-005).
//!
//! # The boundary of record
//!
//! The boundary is the set of uv polylines built here, pinned as follows; every decision
//! below is made on these polylines exactly.
//!
//! - **Derived pcurves** are their PCURVE polylines, vertex for vertex.
//! - **Imported line pcurves** `o + t d` are the chord from `pcurve(t0)` to `pcurve(t1)`, the
//!   ends of the edge range (exactly the line).
//! - **Imported NURBS pcurves** are flattened by fixed bisection: the 17 parameters
//!   `t0 + ((t1 - t0) k) / 16` (`k = 16`: `t1`), then a left-to-right stack: a chord `(L, R)` is
//!   accepted when the surface image of its uv midpoint lies within [`FLATTEN_TOL_M`] =
//!   `DEFAULT_TOL_M / 4` = 2.5e-9 m of the surface image of the pcurve at `(t_L + t_R) * 0.5`;
//!   otherwise that parameter is inserted, at most `MAX_DEPTH` = 12 levels deep
//!   ([`Undecidable::Flatten`] beyond).
//! - Polylines run in the coedge's direction. In a periodic direction with period `P` every
//!   coedge after the first of its loop is moved by `-P floor((s - e) / P + 0.5)` (`s` its
//!   first vertex, `e` the previous coedge's last vertex), so that a loop of imported
//!   pcurves that jumps a whole period between coedges stays continuous (zero for derived
//!   pcurves, which PCURVE already unwrapped). Then the loop's chart offset
//!   ([`FaceChart::loop_offsets`]) is added, `p + shift` then `+ offset`, each only when non-zero.
//! - When a coedge's last vertex is not bit-identical to the next coedge's first vertex
//!   (cyclically), a connecting segment joins them; it belongs to the next coedge. Zero-length
//!   segments are dropped.
//! - **Seams.** A coedge whose edge is used by two coedges of the same face is a seam: its
//!   segments take part in the crossing count, but they are not boundary (no exact
//!   on-boundary test, no band).
//!
//! # Classification, in this order
//!
//! 1. `tol_m` finite and positive, else [`TrimError::BadTolerance`] (REQ-003).
//! 2. `u`, `v` finite; a non-periodic coordinate inside the surface's closed domain (sphere
//!    `v` in `[-pi/2, pi/2]`, a NURBS non-periodic direction in its knot domain); a periodic
//!    one within `2^40` periods of the chart. Else [`TrimError::OutsideDomain`].
//! 3. A face whose boundary is not known gives [`TrimError::Undecidable`], never a guess:
//!    a coedge whose pcurve failed (`PcurveFailed`), a loop that winds around a periodic
//!    direction (`SeamWinding`; the even-odd rule in one chart is not defined for it), a
//!    flattening that did not converge, a face wider than two periods. The first reason in
//!    face loop order, coedge order within a loop, then the loop's winding, is reported.
//! 4. A face with no loops is the whole surface: [`TrimClass::Inside`].
//! 5. **Exact pass.** For every chart copy `(ku, kv)` (the face's periodic translates whose
//!    bounding box can contain the query, `k` from `floor((q - hi) / P)` to
//!    `floor((q - lo) / P) + 1`) the query is `q - k P` as an exact [`exact::Shifted`] value.
//!    For each non-seam segment: `q` equal to an end point, or on a horizontal segment, or
//!    collinear with a straddling segment, is [`TrimClass::OnBoundary`] (exact, distance zero).
//!    Crossing number with the ray `+u` from `q`, half-open rule: a segment `a -> b` counts
//!    when exactly one end has `v >= q.v` and the crossing lies right of `q`
//!    (`orient(a, b, q) > 0` going up, `< 0` going down, `= 0` (only a seam) counts). The query
//!    is inside a copy when its count is odd, inside the face when inside any copy.
//! 6. **Band** (REQ-002). `Q = S(u, v)`. For each non-seam segment `a -> b` the distance from
//!    `Q` to the surface image of the segment is bounded by branch and bound over dyadic
//!    pieces `[s0, s1]` of `s` in `[0, 1]`, points `a + (b - a) s` (left piece first). With `m`
//!    the piece midpoint, `d = |Q - S(m)|` (the GEO.NURBS.ANALYTIC distance), `h` half the
//!    Lipschitz bound of `S` along the piece (`(Lu |du| + Lv |dv|) / 2`, times `1 + 2^-20`) and
//!    the guard `E = 2^-40 (|Q|inf + |S(m)|inf + (Lu + Lv)(|a|inf + |b|inf))`:
//!    `d + E <= tol` is a witness (OnBoundary); `d - h - E > tol` prunes the piece; a piece with
//!    `h <= rho / 2`, `rho = tol * 2^-12`, counts as OnBoundary; otherwise it is halved (at most
//!    64 levels and [`MAX_BAND_PIECES`] pieces below the roots per query, else
//!    [`TrimError::Budget`]; each segment's root piece `[0, 1]` is evaluated once, at build). So every
//!    point within `tol_m` of the boundary image is OnBoundary, and an OnBoundary point is
//!    within `tol_m (1 + 2^-13) + 2E` of it: the band is closed outward by that resolution.
//!    `S` is evaluated with non-periodic coordinates clamped into the domain (a 1-Lipschitz
//!    map, so the bounds still hold).
//! 7. Otherwise Inside or Outside from step 5.
//!
//! **Lipschitz bounds** `Lu >= |Su|`, `Lv >= |Sv|`: plane 1, 1; cylinder `r`, 1; cone
//! `max |r0 + v tan a|` over the piece's end `v`, `hypot(1, tan a)`; sphere `r`, `r`; torus
//! `R + r`, `r`; NURBS `2 p (w_max / w_min) diam / dmin` per direction (`p` the degree, `diam`
//! the diagonal of the control points' bounding box, `dmin` the least positive
//! `knot[i + p] - knot[i]`): `S_u` is `p sum_i N_{i,p-1} sum_j M_j (w_ij (P_ij - S) -
//! w_{i-1,j} (P_{i-1,j} - S)) / (dknot_i W)` and `S` lies in the control points' hull.
//!
//! # Determinism (D0, KNP-1)
//!
//! Binary64 `+ - * /` one operation at a time (no FMA, no `f64::min`/`max`), `floor`,
//! `hypot` from `fid_num`, trigonometry only inside GEO.NURBS; exact integer arithmetic in
//! [`exact`]; vectors and a `BTreeMap` only; every loop bounded. Nothing panics.
//!
//! [`derive_pcurves`]: super::pcurve::derive_pcurves

use core::cmp::Ordering;
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

/// Flattening tolerance of imported NURBS pcurves, metres (`DEFAULT_TOL_M / 4`).
pub const FLATTEN_TOL_M: f64 = DEFAULT_TOL_M / 4.0;
/// Initial chords of a flattened NURBS pcurve.
pub const FLATTEN_INITIAL: usize = 16;
/// Band resolution relative to `tol_m` (`2^-12`).
pub const RESOLUTION_REL: f64 = 1.0 / 4096.0;
/// Evaluation guard relative to the coordinates (`2^-40`).
pub const GUARD_REL: f64 = 1.0 / 1_099_511_627_776.0;
/// Inflation of every Lipschitz bound (`1 + 2^-20`).
pub const LIPSCHITZ_INFLATE: f64 = 1.0 + 1.0 / 1_048_576.0;
/// Deepest band bisection.
pub const MAX_BAND_DEPTH: u32 = 64;
/// Most band pieces examined per classification.
pub const MAX_BAND_PIECES: usize = 1 << 16;
/// Widest face in a periodic direction, in periods.
pub const MAX_EXTENT_PERIODS: f64 = 2.0;

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
    /// The face surface is neither analytic nor NURBS.
    NoSurface,
    /// PCURVE could not give this coedge a trimming curve (REQ-GEO.CAD.TOPO.PCURVE-003).
    PcurveFailed { coedge: CoedgeIx, reason: FailReason },
    /// The pcurve result of this coedge is missing or not a curve.
    NoPcurve { coedge: CoedgeIx },
    /// The imported pcurve could not be evaluated at its edge range.
    ImportedEval { coedge: CoedgeIx },
    /// The imported NURBS pcurve did not flatten within [`FLATTEN_TOL_M`].
    Flatten { coedge: CoedgeIx },
    /// A charted vertex is not finite.
    NonFinite { coedge: CoedgeIx },
    /// PCURVE gave the loop no periods (an incomplete loop).
    IncompleteLoop { r#loop: LoopIx },
    /// The loop winds around a periodic direction (`SeamWinding`).
    Winding { r#loop: LoopIx, periods: [i64; 2] },
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

/// One segment of a trimming polyline in the face chart.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TrimSeg {
    pub a: [f64; DIM],
    pub b: [f64; DIM],
    /// Position of the loop in the face's loop list.
    pub loop_ix: usize,
    /// The coedge the segment belongs to.
    pub coedge_ix: CoedgeIx,
    /// A seam segment counts crossings but is not boundary.
    pub seam: bool,
    /// `[lo, hi]` of the two ends.
    pub bbox: [[f64; DIM]; 2],
}

/// The trimming polylines of one face in its chart, ready for [`classify_uv`].
#[derive(Debug, Clone, PartialEq)]
pub struct TrimIndex {
    face: FaceIx,
    surface: SurfaceDef,
    chart: FaceChart,
    segments: Vec<TrimSeg>,
    has_loops: bool,
    periods: [Option<f64>; DIM],
    domain: [(f64, f64); DIM],
    lo: [f64; DIM],
    hi: [f64; DIM],
    lip: [f64; DIM],
    /// `(r0, tan a)` of a cone, whose `Lu` depends on `v`.
    cone: Option<(f64, f64)>,
    undecidable: Option<Undecidable>,
    /// Per segment, the band's root [`piece`] (`None` when it could not be evaluated at build;
    /// it is then evaluated, and fails, at query time).
    roots: Vec<Option<Piece>>,
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
    for i in 0..DIM {
        if periods[i].is_none() {
            let (lo, hi) = domain[i];
            if c[i] < lo {
                c[i] = lo;
            } else if c[i] > hi {
                c[i] = hi;
            }
        }
    }
    s.point(c).map_err(|_| TrimError::SurfaceEval)
}

fn nurbs_lip(n: &NurbsSurface) -> Option<[f64; DIM]> {
    let ctrl = n.ctrl();
    let first = ctrl.first()?;
    let (mut lo, mut hi) = (*first, *first);
    for p in ctrl {
        for i in 0..3 {
            if p[i] < lo[i] {
                lo[i] = p[i];
            }
            if p[i] > hi[i] {
                hi[i] = p[i];
            }
        }
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
    Some([dir(n.knots_u().as_slice(), n.degree_u())?, dir(n.knots_v().as_slice(), n.degree_v())?])
}

/// Lipschitz bounds `[Lu, Lv]` and the cone parameters.
fn lipschitz(s: Surf<'_>) -> Option<([f64; DIM], Option<(f64, f64)>)> {
    match s {
        Surf::A(a) => match a {
            AnalyticSurface::Plane(_) => Some(([1.0, 1.0], None)),
            AnalyticSurface::Cylinder(c) => Some(([c.radius(), 1.0], None)),
            AnalyticSurface::Cone(c) => {
                let ta = fid_num::f::tan(c.half_angle()).ok()?;
                Some(([0.0, hypot(1.0, ta)?], Some((c.radius(), ta))))
            }
            AnalyticSurface::Sphere(sp) => Some(([sp.radius(), sp.radius()], None)),
            AnalyticSurface::Torus(t) => {
                let (big, small) = t.radii();
                Some(([big + small, small], None))
            }
        },
        Surf::N(n) => nurbs_lip(n).map(|l| (l, None)),
    }
}

// ---------------------------------------------------------------- building the index

/// Flattens one imported pcurve in its coedge's direction.
fn flatten(
    s: Surf<'_>,
    periods: &[Option<f64>; DIM],
    domain: &[(f64, f64); DIM],
    p: &Curve2,
    range: [f64; 2],
    reversed: bool,
    coedge: CoedgeIx,
) -> Result<Vec<[f64; DIM]>, Undecidable> {
    let at = |t: f64| pcurve_point(p, t).map_err(|_| Undecidable::ImportedEval { coedge });
    if p.as_line().is_some() {
        let (a, b) = (at(range[0])?, at(range[1])?);
        return Ok(if reversed { vec![b, a] } else { vec![a, b] });
    }
    let flat = Undecidable::Flatten { coedge };
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
        let on = eval_clamped(s, periods, domain, at(tm)?).map_err(|_| flat)?;
        let chord = eval_clamped(s, periods, domain, mid).map_err(|_| flat)?;
        let d = dist3(on, chord).ok_or(flat)?;
        if d <= FLATTEN_TOL_M {
            out.push(uvr);
            left = (tr, uvr);
            continue;
        }
        if depth >= MAX_DEPTH || tm == left.0 || tm == tr {
            return Err(flat);
        }
        stack.push((tr, uvr, depth + 1));
        stack.push((tm, at(tm)?, depth + 1));
    }
    Ok(out)
}

/// One coedge's charted polyline.
fn coedge_polyline(
    body: &Body,
    pcurves: &Pcurves,
    s: Surf<'_>,
    periods: &[Option<f64>; DIM],
    domain: &[(f64, f64); DIM],
    cx: CoedgeIx,
) -> Result<Vec<[f64; DIM]>, Undecidable> {
    let missing = Undecidable::NoPcurve { coedge: cx };
    match pcurves.curves.get(cx.index()).ok_or(missing)? {
        Ok(TrimCurve::Derived { uv, .. }) => Ok(uv.clone()),
        Ok(TrimCurve::Imported(p)) => {
            let ce = body.coedge(cx).ok_or(missing)?;
            let edge = body.edge(ce.edge()).ok_or(missing)?;
            flatten(s, periods, domain, p, edge.param_range(), ce.sense() == Sense::Reversed, cx)
        }
        Err(PcurveError::PcurveFailed { reason, .. }) => Err(Undecidable::PcurveFailed { coedge: cx, reason: *reason }),
        Err(_) => Err(missing),
    }
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
            segments: Vec::new(),
            has_loops: nloops > 0,
            periods: [None, None],
            domain: [(0.0, 0.0); DIM],
            lo: [0.0; DIM],
            hi: [0.0; DIM],
            lip: [0.0; DIM],
            cone: None,
            undecidable: None,
            roots: Vec::new(),
        };
        let Some(s) = Surf::of(f.surface()) else {
            idx.undecidable = Some(Undecidable::NoSurface);
            return Ok(idx);
        };
        idx.periods = s.periods();
        idx.domain = domain_of(s);
        match lipschitz(s) {
            Some((lip, cone)) => {
                idx.lip = lip;
                idx.cone = cone;
            }
            None => {
                idx.undecidable = Some(Undecidable::NoSurface);
                return Ok(idx);
            }
        }
        // Edge use counts within the face: an edge used twice is a seam.
        let mut uses: BTreeMap<usize, usize> = BTreeMap::new();
        for &lx in f.loops() {
            let lp = body.r#loop(lx).ok_or(TrimError::Mismatch)?;
            for &cx in lp.coedges() {
                let e = body.coedge(cx).ok_or(TrimError::Mismatch)?.edge().index();
                *uses.entry(e).or_insert(0) += 1;
            }
        }
        match idx.assemble(body, pcurves, s, &uses) {
            Ok(()) => {
                let roots = idx.segments.iter().map(|g| piece(&idx, s, g, 0.0, 1.0).ok()).collect();
                idx.roots = roots;
            }
            Err(u) => {
                idx.segments.clear();
                idx.undecidable = Some(u);
            }
        }
        Ok(idx)
    }

    fn assemble(
        &mut self,
        body: &Body,
        pcurves: &Pcurves,
        s: Surf<'_>,
        uses: &BTreeMap<usize, usize>,
    ) -> Result<(), Undecidable> {
        let f = body.face(self.face).ok_or(Undecidable::NoSurface)?;
        let periods = self.periods;
        let domain = self.domain;
        let mut first_vertex = true;
        for (j, &lx) in f.loops().iter().enumerate() {
            let lp = body.r#loop(lx).ok_or(Undecidable::IncompleteLoop { r#loop: lx })?;
            let offset = self.chart.loop_offsets.get(j).copied().unwrap_or([0.0; DIM]);
            // (polyline, coedge, seam) per coedge, charted.
            let mut parts: Vec<(Vec<[f64; DIM]>, CoedgeIx, bool)> = Vec::with_capacity(lp.coedges().len());
            let mut prev_end: Option<[f64; DIM]> = None;
            for &cx in lp.coedges() {
                let mut pts = coedge_polyline(body, pcurves, s, &periods, &domain, cx)?;
                let seam = body.coedge(cx).and_then(|ce| uses.get(&ce.edge().index())).is_some_and(|&n| n >= 2);
                let mut shift = [0.0; DIM];
                if let (Some(e), Some(st)) = (prev_end, pts.first()) {
                    for i in 0..DIM {
                        if let Some(p) = periods[i] {
                            shift[i] = 0.0 - floor((st[i] - e[i]) / p + 0.5) * p;
                        }
                    }
                }
                for q in pts.iter_mut() {
                    for i in 0..DIM {
                        if shift[i] != 0.0 {
                            q[i] += shift[i];
                        }
                    }
                }
                prev_end = pts.last().copied();
                for q in pts.iter_mut() {
                    for i in 0..DIM {
                        if offset[i] != 0.0 {
                            q[i] += offset[i];
                        }
                    }
                    if !(q[0].is_finite() && q[1].is_finite()) {
                        return Err(Undecidable::NonFinite { coedge: cx });
                    }
                }
                parts.push((pts, cx, seam));
            }
            match self.chart.loop_periods.get(j).copied().flatten() {
                None => return Err(Undecidable::IncompleteLoop { r#loop: lx }),
                Some(p) if p != [0, 0] => return Err(Undecidable::Winding { r#loop: lx, periods: p }),
                Some(_) => {}
            }
            let n = parts.len();
            for k in 0..n {
                let Some((pts, cx, seam)) = parts.get(k) else { continue };
                for w in pts.windows(2) {
                    if let [a, b] = w {
                        self.push(*a, *b, j, *cx, *seam, &mut first_vertex);
                    }
                }
                // Connector to the next coedge's first vertex.
                if let (Some(end), Some((next, ncx, _))) = (pts.last(), parts.get((k + 1) % n)) {
                    if let Some(st) = next.first() {
                        self.push(*end, *st, j, *ncx, false, &mut first_vertex);
                    }
                }
            }
        }
        for i in 0..DIM {
            if let Some(p) = periods[i] {
                if !self.segments.is_empty() && self.hi[i] - self.lo[i] > MAX_EXTENT_PERIODS * p {
                    return Err(Undecidable::ChartExtent);
                }
            }
        }
        Ok(())
    }

    fn push(
        &mut self,
        a: [f64; DIM],
        b: [f64; DIM],
        loop_ix: usize,
        coedge_ix: CoedgeIx,
        seam: bool,
        first: &mut bool,
    ) {
        if a[0].to_bits() == b[0].to_bits() && a[1].to_bits() == b[1].to_bits() {
            return;
        }
        let mut bbox = [a, a];
        for i in 0..DIM {
            if b[i] < bbox[0][i] {
                bbox[0][i] = b[i];
            }
            if b[i] > bbox[1][i] {
                bbox[1][i] = b[i];
            }
        }
        for i in 0..DIM {
            if *first || bbox[0][i] < self.lo[i] {
                self.lo[i] = bbox[0][i];
            }
            if *first || bbox[1][i] > self.hi[i] {
                self.hi[i] = bbox[1][i];
            }
        }
        *first = false;
        self.segments.push(TrimSeg { a, b, loop_ix, coedge_ix, seam, bbox });
    }

    /// The face this index belongs to.
    pub fn face(&self) -> FaceIx {
        self.face
    }

    /// The trimming segments in the chart, loop by loop.
    pub fn segments(&self) -> &[TrimSeg] {
        &self.segments
    }

    /// The face chart from PCURVE.
    pub fn chart(&self) -> &FaceChart {
        &self.chart
    }

    /// Why the face cannot be classified, if it cannot.
    pub fn undecidable(&self) -> Option<Undecidable> {
        self.undecidable
    }
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
                let k = (q[i] - index.lo[i]) / p;
                if !(k.is_finite() && abs(k) <= exact::MAX_K as f64 / 2.0) {
                    return Err(outside);
                }
            }
        }
    }
    if let Some(r) = index.undecidable {
        return Err(TrimError::Undecidable(r));
    }
    if !index.has_loops {
        return Ok(TrimClass::Inside);
    }
    let Some(s) = Surf::of(&index.surface) else {
        return Err(TrimError::Undecidable(Undecidable::NoSurface));
    };

    // Step 5: exact pass over the chart copies.
    let mut ranges = [(0i64, 0i64); DIM];
    for i in 0..DIM {
        if let Some(p) = index.periods[i] {
            let lo = floor((q[i] - index.hi[i]) / p) as i64;
            let hi = floor((q[i] - index.lo[i]) / p) as i64 + 1;
            ranges[i] = (lo, hi);
        }
    }
    let mut inside = false;
    for ku in ranges[0].0..=ranges[0].1 {
        for kv in ranges[1].0..=ranges[1].1 {
            let qs = [
                Shifted { x: u, k: 0 - ku, p: index.periods[0].unwrap_or(0.0) },
                Shifted { x: v, k: 0 - kv, p: index.periods[1].unwrap_or(0.0) },
            ];
            match copy_pass(index, qs)? {
                Pass::OnBoundary => return Ok(TrimClass::OnBoundary),
                Pass::Count(odd) => inside |= odd,
            }
        }
    }

    // Step 6: the band.
    let big_q = eval_clamped(s, &index.periods, &index.domain, q)?;
    let mut pieces = 0usize;
    for (seg, root) in index.segments.iter().zip(index.roots.iter()) {
        if seg.seam {
            continue;
        }
        if band(index, s, big_q, seg, *root, tol_m, &mut pieces)? {
            return Ok(TrimClass::OnBoundary);
        }
    }
    Ok(if inside { TrimClass::Inside } else { TrimClass::Outside })
}

enum Pass {
    OnBoundary,
    Count(bool),
}

fn sh(p: [f64; DIM]) -> [Shifted; DIM] {
    [Shifted::plain(p[0]), Shifted::plain(p[1])]
}

fn ord(a: Shifted, b: Shifted) -> Result<Ordering, TrimError> {
    exact::cmp(a, b).ok_or(TrimError::SurfaceEval)
}

/// Exact on-boundary test and crossing parity for one chart copy.
fn copy_pass(index: &TrimIndex, q: [Shifted; DIM]) -> Result<Pass, TrimError> {
    let mut odd = false;
    for seg in &index.segments {
        let (a, b) = (sh(seg.a), sh(seg.b));
        // Quick rejection: the query is below or above the segment in v, or right of it.
        let below = ord(q[1], Shifted::plain(seg.bbox[0][1]))? == Ordering::Less;
        let above = ord(q[1], Shifted::plain(seg.bbox[1][1]))? == Ordering::Greater;
        if below || above {
            continue;
        }
        let right = ord(q[0], Shifted::plain(seg.bbox[1][0]))? == Ordering::Greater;
        if right {
            continue;
        }
        let ga = ord(a[1], q[1])? != Ordering::Less;
        let gb = ord(b[1], q[1])? != Ordering::Less;
        if !seg.seam {
            let is_q = |p: [Shifted; DIM]| -> Result<bool, TrimError> {
                Ok(ord(p[0], q[0])? == Ordering::Equal && ord(p[1], q[1])? == Ordering::Equal)
            };
            if is_q(a)? || is_q(b)? {
                return Ok(Pass::OnBoundary);
            }
            if seg.a[1] == seg.b[1]
                && ord(a[1], q[1])? == Ordering::Equal
                && ord(Shifted::plain(seg.bbox[0][0]), q[0])? != Ordering::Greater
            {
                return Ok(Pass::OnBoundary);
            }
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
        let up = gb;
        if (up && o == Ordering::Greater) || (!up && o == Ordering::Less) {
            odd = !odd;
        }
    }
    Ok(Pass::Count(odd))
}

fn at(seg: &TrimSeg, s: f64) -> [f64; DIM] {
    [seg.a[0] + (seg.b[0] - seg.a[0]) * s, seg.a[1] + (seg.b[1] - seg.a[1]) * s]
}

/// Step 6 for one segment: is some point of its surface image within `tol` of `big_q`?
/// The surface image of a piece's midpoint, its `Lu` and half its Lipschitz bound.
type Piece = ([f64; 3], f64, f64);

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

/// Step 6 for one segment: is some point of its surface image within `tol` of `big_q`?
/// `root` is the cached [`piece`] of `[0, 1]`.
fn band(
    index: &TrimIndex,
    s: Surf<'_>,
    big_q: [f64; 3],
    seg: &TrimSeg,
    root: Option<Piece>,
    tol: f64,
    pieces: &mut usize,
) -> Result<bool, TrimError> {
    let rho = tol * RESOLUTION_REL;
    let lv = index.lip[1];
    let coord = ninf2(seg.a) + ninf2(seg.b);
    let mut stack: Vec<(f64, f64, u32)> = vec![(0.0, 1.0, 0)];
    while let Some((s0, s1, depth)) = stack.pop() {
        let (sm, lu, half) = match (depth, root) {
            (0, Some(r)) => r,
            _ => piece(index, s, seg, s0, s1)?,
        };
        let d = dist3(big_q, sm).ok_or(TrimError::SurfaceEval)?;
        let guard = GUARD_REL * ((ninf3(big_q) + ninf3(sm)) + (lu + lv) * coord);
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
        assert_eq!(FLATTEN_TOL_M, 2.5e-9);
        assert_eq!(RESOLUTION_REL, 2f64.powi(-12));
        assert_eq!(GUARD_REL, 2f64.powi(-40));
        assert_eq!(LIPSCHITZ_INFLATE, 1.0 + 2f64.powi(-20));
    }

    #[test]
    fn letters() {
        assert_eq!(TrimClass::Inside.letter(), 'I');
        assert_eq!(TrimClass::Outside.letter(), 'O');
        assert_eq!(TrimClass::OnBoundary.letter(), 'B');
    }
}
