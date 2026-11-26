//! B-rep defect detection without repair. Spec node `GEO.CAD.TOPO.VALID`.
//!
//! [`validate`] checks a built [`Body`] against the conditions the rest of GEO relies on and
//! returns a [`ValidationReport`] listing every [`Defect`] with its entity IDs and measured
//! value. It takes the body by shared reference and never changes it (REQ-006): there is no
//! repair, healing or rebuild here.
//!
//! # What a built body already guarantees
//!
//! [`BodyBuilder::build`](super::model::BodyBuilder::build) (spec node GEO.CAD.TOPO.MODEL)
//! refuses a description whose loops do not close through shared vertices, an edge with
//! more than two coedges, two coedges of one edge with the same sense, and an edge used once
//! in a shell declared closed. So on a [`Body`] the combinatorial kinds can only arise in the
//! forms below; [`Defect::NonManifoldEdge`] and a [`Defect::DanglingEdge`] with no use cannot
//! arise at all and are kept so the report type covers REQ-GEO.CAD.TOPO-004 in full.
//!
//! # The checks (one tolerance `tol_m` in metres for every gap and deviation)
//!
//! - **OpenLoop** (REQ-001). For each loop, each coedge `k` and the next one `k + 1`
//!   (cyclically): the end point of coedge `k` is its edge curve at `t1` (`t0` for a
//!   `Reversed` coedge), the start point of coedge `k + 1` its edge curve at `t0` (`t1`); an
//!   edge without a curve uses its vertex. `gap_m` is the largest 3-D distance
//!   `hypot(hypot(e0, e1), e2)` over the loop's junctions; the loop is reported once when
//!   `gap_m > tol_m`.
//! - **SelfIntersectingLoop** (REQ-002). The trimming polylines of a face are those of
//!   [`TrimIndex`] (GEO.CAD.TOPO.TRIM, built from the pcurves [`derive_pcurves`] gives at
//!   `tol_m`), with exact chart points `x + k P`. Segments are swept in the order
//!   (least rounded `u`, segment index); two segments whose (margin-widened) boxes overlap are
//!   tested with the exact orientation predicates of [`exact`]. Two segments that follow each
//!   other in a loop and share their joining point only meet there unless they are collinear
//!   and fold back; any other common point is an intersection. Every loop that takes part in
//!   an intersection (with itself or with another loop of the face) is reported once. A face
//!   whose boundary TRIM cannot assemble is not swept (its PCURVE failure is reported).
//! - **DanglingEdge** (REQ-003). A body is *declared solid* when at least one of its shells is
//!   declared closed. In a solid body an edge with fewer than two coedges is dangling; `uses`
//!   is its coedge count. (Such an edge lies in an open shell of the body: the builder refuses
//!   it in a closed one.)
//! - **NonManifoldEdge** (REQ-003). In a solid body an edge used by more than two distinct
//!   faces; `uses` is the face count.
//! - **InconsistentOrientation** (REQ-004). TRIM's convention is that the material of a face
//!   lies left of each loop in the `(u, v)` chart for a `Same` face and right of it for a
//!   `Reversed` face. A face's loops therefore agree with its sense when the signed area of
//!   its closed chart polylines (shoelace over the rounded segment ends, each loop taken
//!   relative to its first vertex) is positive for `Same` and negative for `Reversed`. The
//!   area is used only when every loop of the face winds zero periods, TRIM assembled the
//!   boundary and `|area| > 2^-40 sum |terms|`; otherwise the face is *undetermined* and
//!   counts as agreeing. The builder already makes the two coedges of a closed-shell edge
//!   opposite in sense, so they traverse the edge in the same direction relative to their
//!   face senses exactly when one of their two faces agrees with its sense and the other does
//!   not; such an edge of a closed shell (two coedges in two faces) is reported.
//! - **EdgeOffSurface** (REQ-005). For every edge with a curve and every distinct face of its
//!   coedges, the curve at the 33 parameters `t0 + ((t1 - t0) k) / 32` (`t_32 = t1`) is
//!   inverted onto the face surface: an analytic surface by its closed form (the distance is
//!   used whether or not the foot is unique), a NURBS surface by GEO.NURBS.INVERT (a certified
//!   answer, tied or not; an uncertified one gives its best known distance, an upper bound,
//!   so a doubtful sample is never silently passed). `deviation_m` is the largest distance; a
//!   sample whose curve point or inversion cannot be evaluated is left out.
//! - **PcurveMismatch** (REQ-005). For every coedge with an imported pcurve,
//!   [`pcurve_disagreement`] (33 parameters); reported above `tol_m`. A pcurve that cannot be
//!   evaluated has no value here (TRIM then reports the face undecidable).
//! - **PcurveFailed**, passed through from [`derive_pcurves`] at `tol_m`: the coedge, the
//!   [`FailReason`] and the best deviation measured (infinite when none was).
//! - **SeamWinding**, passed through from [`derive_pcurves`] for a loop on a surface periodic
//!   in both directions (a torus or a doubly periodic NURBS), where TRIM cannot use a winding
//!   loop. On a surface periodic in one direction a winding loop is a valid band or cap
//!   (GEO.CAD.TOPO.PCURVE REQ-004) and is not a defect.
//!
//! # Order and cap (REQ-007)
//!
//! Defects are sorted by kind in the order of [`DefectKind`], then by ascending
//! [`TopologyId`] of the primary entity (the face second for EdgeOffSurface). With a cap the
//! first `cap` defects are kept and `truncated` counts the rest.
//!
//! # Determinism (D0, KNP-1)
//!
//! Binary64 `+ - * /` one operation at a time (no FMA, no `f64::min`/`max`), `hypot` from
//! `fid_num`, trigonometry only inside GEO.NURBS, exact integer predicates for every
//! intersection decision, vectors and B-trees only, fixed iteration orders. Nothing panics.
//!
//! [`exact`]: super::trim::exact

use core::cmp::Ordering;
use core::fmt;
use std::collections::BTreeSet;

use fid_geo_nurbs::{invert_surface, InvertError};

use super::geom::CurveDef;
use super::model::{Body, CoedgeIx, EdgeIx, FaceIx, LoopIx, Sense, TopologyId};
use super::pcurve::{
    curve_point, derive_pcurves, dist3, grid, larger, pcurve_disagreement, FailReason, PcurveError, Pcurves, Surf,
};
use super::trim::exact::{cmp, orient, Shifted};
use super::trim::{TrimIndex, TrimSeg};

/// Default validation tolerance in metres [unverified: needs evidence from IO.CORP STEP
/// corpora].
pub const DEFAULT_VALID_TOL_M: f64 = 1e-7;

/// Edge parameters sampled for [`Defect::EdgeOffSurface`] (REQ-005).
pub const EDGE_SAMPLES: usize = 33;

/// `2^-40`: relative margin on rounded chart boxes and the area threshold.
const REL: f64 = 1.0 / 1_099_511_627_776.0;

/// The kind of a defect; the declaration order is the report order (REQ-007).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum DefectKind {
    OpenLoop,
    SelfIntersectingLoop,
    DanglingEdge,
    NonManifoldEdge,
    InconsistentOrientation,
    EdgeOffSurface,
    PcurveMismatch,
    PcurveFailed,
    SeamWinding,
}

impl DefectKind {
    /// Stable variant name.
    pub fn code(self) -> &'static str {
        match self {
            DefectKind::OpenLoop => "OpenLoop",
            DefectKind::SelfIntersectingLoop => "SelfIntersectingLoop",
            DefectKind::DanglingEdge => "DanglingEdge",
            DefectKind::NonManifoldEdge => "NonManifoldEdge",
            DefectKind::InconsistentOrientation => "InconsistentOrientation",
            DefectKind::EdgeOffSurface => "EdgeOffSurface",
            DefectKind::PcurveMismatch => "PcurveMismatch",
            DefectKind::PcurveFailed => "PcurveFailed",
            DefectKind::SeamWinding => "SeamWinding",
        }
    }
}

/// One defect, with the stable IDs of its entities and its measured value.
#[derive(Debug, Clone, PartialEq)]
pub enum Defect {
    /// REQ-001: the largest 3-D gap between consecutive coedges of the loop.
    OpenLoop { r#loop: TopologyId, gap_m: f64 },
    /// REQ-002: the loop's trimming polyline meets itself or another loop of its face.
    SelfIntersectingLoop { r#loop: TopologyId },
    /// REQ-003: an edge of a solid body with `uses` (< 2) coedges.
    DanglingEdge { edge: TopologyId, uses: u64 },
    /// REQ-003: an edge of a solid body used by `uses` (> 2) faces.
    NonManifoldEdge { edge: TopologyId, uses: u64 },
    /// REQ-004: the edge's two coedges traverse it in the same direction relative to their
    /// face senses.
    InconsistentOrientation { edge: TopologyId },
    /// REQ-005: the edge curve is `deviation_m` from the face surface at its worst sample.
    EdgeOffSurface { edge: TopologyId, face: TopologyId, deviation_m: f64 },
    /// REQ-005: the imported pcurve's surface image is `deviation_m` from the edge curve.
    PcurveMismatch { coedge: TopologyId, deviation_m: f64 },
    /// GEO.CAD.TOPO.PCURVE REQ-003: no trimming curve for the coedge.
    PcurveFailed { coedge: TopologyId, reason: FailReason, best_deviation_m: f64 },
    /// GEO.CAD.TOPO.PCURVE REQ-004 on a doubly periodic surface.
    SeamWinding { r#loop: TopologyId, periods: [i64; 2] },
}

impl Defect {
    /// The kind of this defect.
    pub fn kind(&self) -> DefectKind {
        match self {
            Defect::OpenLoop { .. } => DefectKind::OpenLoop,
            Defect::SelfIntersectingLoop { .. } => DefectKind::SelfIntersectingLoop,
            Defect::DanglingEdge { .. } => DefectKind::DanglingEdge,
            Defect::NonManifoldEdge { .. } => DefectKind::NonManifoldEdge,
            Defect::InconsistentOrientation { .. } => DefectKind::InconsistentOrientation,
            Defect::EdgeOffSurface { .. } => DefectKind::EdgeOffSurface,
            Defect::PcurveMismatch { .. } => DefectKind::PcurveMismatch,
            Defect::PcurveFailed { .. } => DefectKind::PcurveFailed,
            Defect::SeamWinding { .. } => DefectKind::SeamWinding,
        }
    }

    /// The primary entity: the loop, edge or coedge the defect is about.
    pub fn entity(&self) -> &TopologyId {
        match self {
            Defect::OpenLoop { r#loop, .. }
            | Defect::SelfIntersectingLoop { r#loop }
            | Defect::SeamWinding { r#loop, .. } => r#loop,
            Defect::DanglingEdge { edge, .. }
            | Defect::NonManifoldEdge { edge, .. }
            | Defect::InconsistentOrientation { edge }
            | Defect::EdgeOffSurface { edge, .. } => edge,
            Defect::PcurveMismatch { coedge, .. } | Defect::PcurveFailed { coedge, .. } => coedge,
        }
    }

    /// The second entity, if any (the face of an EdgeOffSurface).
    pub fn second(&self) -> Option<&TopologyId> {
        match self {
            Defect::EdgeOffSurface { face, .. } => Some(face),
            _ => None,
        }
    }

    /// REQ-007 order: kind, then primary ID, then second ID.
    fn order(&self, o: &Defect) -> Ordering {
        self.kind()
            .cmp(&o.kind())
            .then_with(|| self.entity().cmp(o.entity()))
            .then_with(|| self.second().cmp(&o.second()))
    }
}

/// The result of [`validate`].
#[derive(Debug, Clone, PartialEq)]
pub struct ValidationReport {
    /// The tolerance the body was checked with, metres.
    pub tolerance_m: f64,
    /// The defects in REQ-007 order, at most `cap` of them.
    pub defects: Vec<Defect>,
    /// Defects found beyond the cap.
    pub truncated: u64,
}

/// Why [`validate`] produced no report. Defects are data, never errors.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ValidError {
    /// `tol_m` is not a finite positive number.
    BadTolerance { value: f64 },
    /// PCURVE or TRIM refused the body in a way a built body cannot cause (fail closed).
    Internal { code: &'static str },
}

impl ValidError {
    /// Stable variant name.
    pub fn code(&self) -> &'static str {
        match self {
            ValidError::BadTolerance { .. } => "BadTolerance",
            ValidError::Internal { .. } => "Internal",
        }
    }
}

impl fmt::Display for ValidError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ValidError::BadTolerance { value } => write!(f, "tolerance {value} is not finite and positive"),
            ValidError::Internal { code } => write!(f, "validation could not run: {code}"),
        }
    }
}

impl core::error::Error for ValidError {}

/// Checks `body` with tolerance `tol_m` metres and returns every defect in REQ-007 order, at
/// most `cap` of them (see the module documentation). The body is only read.
pub fn validate(body: &Body, tol_m: f64, cap: Option<u64>) -> Result<ValidationReport, ValidError> {
    if !(tol_m.is_finite() && tol_m > 0.0) {
        return Err(ValidError::BadTolerance { value: tol_m });
    }
    let pcurves = derive_pcurves(body, tol_m).map_err(|e| match e {
        PcurveError::BadTolerance { value } => ValidError::BadTolerance { value },
        other => ValidError::Internal { code: other.code() },
    })?;
    let mut indexes = Vec::with_capacity(body.faces().len());
    for i in 0..body.faces().len() {
        let ix =
            TrimIndex::build(body, &pcurves, FaceIx::at(i)).map_err(|e| ValidError::Internal { code: e.code() })?;
        indexes.push(ix);
    }
    let mut defects = Vec::new();
    open_loops(body, tol_m, &mut defects);
    self_intersections(body, &indexes, &mut defects);
    edge_uses(body, &mut defects);
    orientation(body, &pcurves, &indexes, &mut defects);
    off_surface(body, tol_m, &mut defects);
    mismatches(body, tol_m, &mut defects);
    pcurve_reports(body, &pcurves, &mut defects);
    defects.sort_by(|a, b| a.order(b));
    let mut truncated = 0u64;
    if let Some(c) = cap {
        let keep = usize::try_from(c).unwrap_or(usize::MAX);
        if defects.len() > keep {
            truncated = u64::try_from(defects.len() - keep).unwrap_or(u64::MAX);
            defects.truncate(keep);
        }
    }
    Ok(ValidationReport { tolerance_m: tol_m, defects, truncated })
}

// ---------------------------------------------------------------- ids

fn loop_id(body: &Body, l: LoopIx) -> Option<TopologyId> {
    body.r#loop(l).map(|x| x.id().clone())
}

fn edge_id(body: &Body, e: EdgeIx) -> Option<TopologyId> {
    body.edge(e).map(|x| x.id().clone())
}

fn coedge_id(body: &Body, c: CoedgeIx) -> Option<TopologyId> {
    body.coedge(c).map(|x| x.id().clone())
}

fn face_id(body: &Body, f: FaceIx) -> Option<TopologyId> {
    body.face(f).map(|x| x.id().clone())
}

// ---------------------------------------------------------------- REQ-001 open loops

/// The start (`end == false`) or end point of a coedge.
fn coedge_point(body: &Body, c: CoedgeIx, end: bool) -> Option<[f64; 3]> {
    let ce = body.coedge(c)?;
    let e = body.edge(ce.edge())?;
    let [t0, t1] = e.param_range();
    let at_t1 = end == (ce.sense() == Sense::Same);
    match e.curve() {
        Some(curve) => curve_point(curve, if at_t1 { t1 } else { t0 }).ok(),
        None => body.vertex(if at_t1 { e.end() } else { e.start() }).map(|v| v.point()),
    }
}

fn open_loops(body: &Body, tol: f64, out: &mut Vec<Defect>) {
    for (li, lp) in body.loops().iter().enumerate() {
        let cs = lp.coedges();
        let n = cs.len();
        let mut worst: Option<f64> = None;
        for k in 0..n {
            let (Some(&a), Some(&b)) = (cs.get(k), cs.get((k + 1) % n)) else { continue };
            let (Some(p), Some(q)) = (coedge_point(body, a, true), coedge_point(body, b, false)) else { continue };
            if let Some(d) = dist3(p, q) {
                worst = Some(worst.map_or(d, |w| larger(w, d)));
            }
        }
        if let Some(gap_m) = worst {
            if gap_m > tol {
                if let Some(id) = loop_id(body, LoopIx::at(li)) {
                    out.push(Defect::OpenLoop { r#loop: id, gap_m });
                }
            }
        }
    }
}

// ---------------------------------------------------------------- REQ-002 self-intersection

type P2 = [Shifted; 2];

struct Seg {
    a: P2,
    b: P2,
    lo: [f64; 2],
    hi: [f64; 2],
    margin: f64,
    loop_pos: usize,
}

fn exact_point(x: [f64; 2], k: [i64; 2], periods: [Option<f64>; 2]) -> P2 {
    core::array::from_fn(|i| Shifted { x: x[i], k: k[i], p: periods[i].unwrap_or(0.0) })
}

fn same_point(p: &P2, q: &P2) -> bool {
    cmp(p[0], q[0]) == Some(Ordering::Equal) && cmp(p[1], q[1]) == Some(Ordering::Equal)
}

fn abs(x: f64) -> f64 {
    if x < 0.0 {
        0.0 - x
    } else {
        x
    }
}

fn seg_of(g: &TrimSeg, periods: [Option<f64>; 2]) -> Seg {
    let [lo, hi] = g.bbox;
    let margin = REL * ((1.0 + (abs(lo[0]) + abs(lo[1]))) + (abs(hi[0]) + abs(hi[1])));
    Seg {
        a: exact_point(g.pa.x, g.pa.k, periods),
        b: exact_point(g.pb.x, g.pb.k, periods),
        lo,
        hi,
        margin,
        loop_pos: g.loop_ix,
    }
}

/// `p` lies within the closed box of `a` and `b` (exactly); `p` is collinear with them.
fn within(a: &P2, b: &P2, p: &P2) -> bool {
    (0..2).all(|i| {
        let (ca, cb) = (cmp(p[i], a[i]), cmp(p[i], b[i]));
        !matches!(
            (ca, cb),
            (Some(Ordering::Greater), Some(Ordering::Greater)) | (Some(Ordering::Less), Some(Ordering::Less))
        ) && ca.is_some()
            && cb.is_some()
    })
}

/// Whether the closed segments `a b` and `c d` share a point (exact).
fn segments_meet(a: &P2, b: &P2, c: &P2, d: &P2) -> bool {
    let (Some(o1), Some(o2), Some(o3), Some(o4)) =
        (orient(*a, *b, *c), orient(*a, *b, *d), orient(*c, *d, *a), orient(*c, *d, *b))
    else {
        return false;
    };
    let opposite = |x: Ordering, y: Ordering| x != Ordering::Equal && y != Ordering::Equal && x != y;
    if opposite(o1, o2) && opposite(o3, o4) {
        return true;
    }
    (o1 == Ordering::Equal && within(a, b, c))
        || (o2 == Ordering::Equal && within(a, b, d))
        || (o3 == Ordering::Equal && within(c, d, a))
        || (o4 == Ordering::Equal && within(c, d, b))
}

/// `a -> b` followed by `b -> d`: they meet beyond `b` only when collinear and folding back.
fn folds_back(a: &P2, b: &P2, d: &P2) -> bool {
    if orient(*a, *b, *d) != Some(Ordering::Equal) {
        return false;
    }
    for ((&ai, &bi), &di) in a.iter().zip(b).zip(d) {
        let ab = cmp(ai, bi);
        if ab != Some(Ordering::Equal) {
            return ab.is_some() && cmp(di, bi) == ab;
        }
    }
    false
}

/// Loop positions of the face that take part in an intersection.
fn face_intersections(ix: &TrimIndex) -> BTreeSet<usize> {
    let chart = ix.chart();
    let periods = [chart.period_u, chart.period_v];
    let segs: Vec<Seg> = ix.segments().iter().map(|g| seg_of(g, periods)).collect();
    let n = segs.len();
    // Cyclic successor of each segment within its loop (segments are stored loop by loop).
    let mut next = vec![0usize; n];
    let mut start = 0usize;
    while start < n {
        let lp = segs.get(start).map_or(0, |s| s.loop_pos);
        let mut end = start;
        while end < n && segs.get(end).is_some_and(|s| s.loop_pos == lp) {
            end += 1;
        }
        for k in start..end {
            if let Some(slot) = next.get_mut(k) {
                *slot = if k + 1 < end { k + 1 } else { start };
            }
        }
        start = end;
    }
    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by(|&i, &j| {
        let (li, lj) = (segs.get(i).map_or(0.0, |s| s.lo[0]), segs.get(j).map_or(0.0, |s| s.lo[0]));
        li.total_cmp(&lj).then(i.cmp(&j))
    });
    let max_margin = segs.iter().fold(0.0, |m, s| larger(m, s.margin));
    let mut hit = BTreeSet::new();
    let mut active: Vec<usize> = Vec::new();
    for &j in &order {
        let Some(s) = segs.get(j) else { continue };
        active.retain(|&i| segs.get(i).is_some_and(|a| a.hi[0] + (a.margin + max_margin) >= s.lo[0]));
        for &i in &active {
            let Some(g) = segs.get(i) else { continue };
            let m = g.margin + s.margin;
            if g.lo[1] > s.hi[1] + m || s.lo[1] > g.hi[1] + m || g.lo[0] > s.hi[0] + m || s.lo[0] > g.hi[0] + m {
                continue;
            }
            let meet = if i == j {
                false
            } else if next.get(i) == Some(&j) && same_point(&g.b, &s.a) {
                folds_back(&g.a, &g.b, &s.b)
            } else if next.get(j) == Some(&i) && same_point(&s.b, &g.a) {
                folds_back(&s.a, &s.b, &g.b)
            } else {
                segments_meet(&g.a, &g.b, &s.a, &s.b)
            };
            if meet {
                hit.insert(g.loop_pos);
                hit.insert(s.loop_pos);
            }
        }
        active.push(j);
    }
    hit
}

fn self_intersections(body: &Body, indexes: &[TrimIndex], out: &mut Vec<Defect>) {
    for (fi, ix) in indexes.iter().enumerate() {
        if ix.undecidable().is_some() {
            continue;
        }
        let Some(face) = body.face(FaceIx::at(fi)) else { continue };
        for pos in face_intersections(ix) {
            if let Some(id) = face.loops().get(pos).and_then(|&l| loop_id(body, l)) {
                out.push(Defect::SelfIntersectingLoop { r#loop: id });
            }
        }
    }
}

// ---------------------------------------------------------------- REQ-003 edge uses

/// Coedges of every edge, canonical order.
fn edge_coedges(body: &Body) -> Vec<Vec<CoedgeIx>> {
    let mut uses = vec![Vec::new(); body.edges().len()];
    for ci in 0..body.coedges().len() {
        let c = CoedgeIx::at(ci);
        if let Some(u) = body.coedge(c).and_then(|ce| uses.get_mut(ce.edge().index())) {
            u.push(c);
        }
    }
    uses
}

fn face_of(body: &Body, c: CoedgeIx) -> Option<FaceIx> {
    body.r#loop(body.coedge(c)?.r#loop()).map(|l| l.face())
}

fn edge_uses(body: &Body, out: &mut Vec<Defect>) {
    let solid = body.shells().iter().any(|s| s.closed());
    if !solid {
        return;
    }
    for (ei, cs) in edge_coedges(body).iter().enumerate() {
        let Some(id) = edge_id(body, EdgeIx::at(ei)) else { continue };
        let faces: BTreeSet<FaceIx> = cs.iter().filter_map(|&c| face_of(body, c)).collect();
        if cs.len() < 2 {
            out.push(Defect::DanglingEdge { edge: id, uses: cs.len() as u64 });
        } else if faces.len() > 2 {
            out.push(Defect::NonManifoldEdge { edge: id, uses: faces.len() as u64 });
        }
    }
}

// ---------------------------------------------------------------- REQ-004 orientation

/// Whether the face's loops disagree with its sense; `None` when undetermined.
fn flipped(body: &Body, pcurves: &Pcurves, fi: usize, ix: &TrimIndex) -> Option<bool> {
    let face = body.face(FaceIx::at(fi))?;
    let chart = pcurves.charts.get(fi)?;
    if face.loops().is_empty() || ix.undecidable().is_some() {
        return None;
    }
    if !chart.loop_periods.iter().all(|p| *p == Some([0, 0])) {
        return None;
    }
    let mut total = 0.0;
    let mut mag = 0.0;
    let mut reference: Option<(usize, [f64; 2])> = None;
    for g in ix.segments() {
        let r = match reference {
            Some((pos, r)) if pos == g.loop_ix => r,
            _ => {
                reference = Some((g.loop_ix, g.a));
                g.a
            }
        };
        let (ax, ay, bx, by) = (g.a[0] - r[0], g.a[1] - r[1], g.b[0] - r[0], g.b[1] - r[1]);
        let left = ax * by;
        let right = ay * bx;
        total += left - right;
        mag += abs(left) + abs(right);
    }
    if !(total.is_finite() && mag.is_finite()) || abs(total) <= REL * mag {
        return None;
    }
    Some((total > 0.0) != (face.sense() == Sense::Same))
}

fn orientation(body: &Body, pcurves: &Pcurves, indexes: &[TrimIndex], out: &mut Vec<Defect>) {
    let flips: Vec<bool> =
        indexes.iter().enumerate().map(|(fi, ix)| flipped(body, pcurves, fi, ix).unwrap_or(false)).collect();
    for (ei, cs) in edge_coedges(body).iter().enumerate() {
        let [a, b] = cs.as_slice() else { continue };
        let (Some(fa), Some(fb)) = (face_of(body, *a), face_of(body, *b)) else { continue };
        if fa == fb {
            continue;
        }
        let closed = body.face(fa).and_then(|f| body.shell(f.shell())).is_some_and(|s| s.closed());
        if !closed {
            continue;
        }
        let (Some(&xa), Some(&xb)) = (flips.get(fa.index()), flips.get(fb.index())) else { continue };
        if xa != xb {
            if let Some(id) = edge_id(body, EdgeIx::at(ei)) {
                out.push(Defect::InconsistentOrientation { edge: id });
            }
        }
    }
}

// ---------------------------------------------------------------- REQ-005 deviations

/// Distance from `q` to the surface, or `None` when it cannot be measured.
fn surface_distance(s: Surf<'_>, q: [f64; 3]) -> Option<f64> {
    let d = match s {
        Surf::A(a) => a.invert(q).ok()?.distance,
        Surf::N(n) => match invert_surface(n, q) {
            Ok(i) => i.distance,
            Err(InvertError::NonConvergence { best }) => best.distance,
            Err(_) => return None,
        },
    };
    d.is_finite().then_some(d)
}

fn curve_deviation(s: Surf<'_>, c: &CurveDef, range: [f64; 2]) -> Option<f64> {
    let mut worst: Option<f64> = None;
    for k in 0..EDGE_SAMPLES {
        let t = grid(range[0], range[1], k, EDGE_SAMPLES - 1);
        let Ok(q) = curve_point(c, t) else { continue };
        if let Some(d) = surface_distance(s, q) {
            worst = Some(worst.map_or(d, |w| larger(w, d)));
        }
    }
    worst
}

fn off_surface(body: &Body, tol: f64, out: &mut Vec<Defect>) {
    for (ei, cs) in edge_coedges(body).iter().enumerate() {
        let Some(edge) = body.edge(EdgeIx::at(ei)) else { continue };
        let Some(curve) = edge.curve() else { continue };
        let faces: BTreeSet<FaceIx> = cs.iter().filter_map(|&c| face_of(body, c)).collect();
        for f in faces {
            let Some(s) = body.face(f).and_then(|x| Surf::of(x.surface())) else { continue };
            let Some(dev) = curve_deviation(s, curve, edge.param_range()) else { continue };
            if dev > tol {
                if let (Some(e), Some(fid)) = (edge_id(body, EdgeIx::at(ei)), face_id(body, f)) {
                    out.push(Defect::EdgeOffSurface { edge: e, face: fid, deviation_m: dev });
                }
            }
        }
    }
}

fn mismatches(body: &Body, tol: f64, out: &mut Vec<Defect>) {
    for (ci, ce) in body.coedges().iter().enumerate() {
        if ce.pcurve().is_none() {
            continue;
        }
        let c = CoedgeIx::at(ci);
        if let Ok(d) = pcurve_disagreement(body, c) {
            if d > tol {
                if let Some(id) = coedge_id(body, c) {
                    out.push(Defect::PcurveMismatch { coedge: id, deviation_m: d });
                }
            }
        }
    }
}

// ---------------------------------------------------------------- PCURVE pass-through

fn pcurve_reports(body: &Body, pcurves: &Pcurves, out: &mut Vec<Defect>) {
    for r in &pcurves.curves {
        if let Err(PcurveError::PcurveFailed { coedge, best_deviation_m, reason }) = r {
            if let Some(id) = coedge_id(body, *coedge) {
                out.push(Defect::PcurveFailed { coedge: id, reason: *reason, best_deviation_m: *best_deviation_m });
            }
        }
    }
    for w in &pcurves.windings {
        let PcurveError::SeamWinding { r#loop, periods } = w else { continue };
        let Some(face) = body.r#loop(*r#loop).map(|l| l.face()) else { continue };
        let doubly = pcurves.charts.get(face.index()).is_some_and(|c| c.period_u.is_some() && c.period_v.is_some());
        if doubly {
            if let Some(id) = loop_id(body, *r#loop) {
                out.push(Defect::SeamWinding { r#loop: id, periods: *periods });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    fn p(x: f64, y: f64) -> P2 {
        [Shifted::plain(x), Shifted::plain(y)]
    }

    #[test]
    fn segment_predicates() {
        // Proper crossing, touching at an end, collinear overlap, disjoint collinear.
        assert!(segments_meet(&p(0.0, 0.0), &p(1.0, 1.0), &p(1.0, 0.0), &p(0.0, 1.0)));
        assert!(segments_meet(&p(0.0, 0.0), &p(1.0, 0.0), &p(0.5, 0.0), &p(0.5, 1.0)));
        assert!(segments_meet(&p(0.0, 0.0), &p(2.0, 0.0), &p(1.0, 0.0), &p(3.0, 0.0)));
        assert!(!segments_meet(&p(0.0, 0.0), &p(1.0, 0.0), &p(2.0, 0.0), &p(3.0, 0.0)));
        assert!(!segments_meet(&p(0.0, 0.0), &p(1.0, 0.0), &p(0.0, 1.0), &p(1.0, 1.0)));
        // A shared joint: straight on, a turn, and a fold back.
        assert!(!folds_back(&p(0.0, 0.0), &p(1.0, 0.0), &p(2.0, 0.0)));
        assert!(!folds_back(&p(0.0, 0.0), &p(1.0, 0.0), &p(1.0, 1.0)));
        assert!(folds_back(&p(0.0, 0.0), &p(1.0, 0.0), &p(0.5, 0.0)));
        assert!(!folds_back(&p(0.0, 0.0), &p(0.0, 1.0), &p(0.0, 2.0)));
        assert!(folds_back(&p(0.0, 0.0), &p(0.0, 1.0), &p(0.0, -1.0)));
        assert!(folds_back(&p(0.0, 0.0), &p(0.0, 1.0), &p(0.0, 0.25)));
    }

    #[test]
    fn shifted_points_compare_by_value() {
        let tau = core::f64::consts::TAU;
        let a = [Shifted { x: tau, k: 0, p: tau }, Shifted::plain(0.0)];
        let b = [Shifted { x: 0.0, k: 1, p: tau }, Shifted::plain(0.0)];
        assert!(same_point(&a, &b));
        assert!(!same_point(&a, &p(0.0, 0.0)));
    }

    #[test]
    fn kind_order_is_the_declaration_order() {
        let kinds = [
            DefectKind::OpenLoop,
            DefectKind::SelfIntersectingLoop,
            DefectKind::DanglingEdge,
            DefectKind::NonManifoldEdge,
            DefectKind::InconsistentOrientation,
            DefectKind::EdgeOffSurface,
            DefectKind::PcurveMismatch,
            DefectKind::PcurveFailed,
            DefectKind::SeamWinding,
        ];
        assert!(kinds.windows(2).all(|w| w[0] < w[1]));
        assert_eq!(kinds.map(DefectKind::code)[8], "SeamWinding");
    }
}
