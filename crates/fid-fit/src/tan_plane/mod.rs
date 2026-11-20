//! Tangential plane in three variants. Spec node `KRN.ENV.TAN.PLANE`.
//!
//! The plane that touches a point set from its material-free side, as a surface plate touches
//! a datum face. Three objectives, selected by a required [`TanVariant`] (no default):
//! external minimax (`fit.plane.tan_minimax@1.0.0`: the smallest largest depth), constrained
//! L1 (`fit.plane.tan_l1@1.0.0`: the smallest sum of depths) and constrained L2
//! (`fit.plane.tan_l2@1.0.0`: the smallest sum of squared depths), each with every point on the
//! material side. The *depth* of a point is its perpendicular distance below the tangent plane,
//! measured into the material; it is never negative for a feasible plane.
//!
//! # Entry points
//!
//! [`fit_tan_plane`] returns the full [`TanPlaneResult`] (contact set, maximum violation,
//! certificate, rocking report, optimality). [`TanPlane`] implements the contract's
//! [`Fit`] trait and returns the contract's [`FitResult`] summary of the same fit.
//!
//! # Pinned procedure
//!
//! 1. **Checks**, in this order: the variant is present (`InvalidParams{field: "variant"}`,
//!    REQ-001; there is no default); [`FitParams::validate`]; the method is
//!    `Method::Tangential` (`method`), the weights uniform (`weights`), the only constraint a
//!    fixed axis direction, the fixed normal (`constraints`); `side_tol` finite and not
//!    negative, `rock_tol` finite in `[0, 1]`, `starts` in `1..=fid_solv::mstart::MAX_STARTS`;
//!    the material side against the data ([`MaterialSide::validate`]).
//! 2. **Fixed normal** (REQ-004): at least one point (`TooFewPoints{need: 1}`); `n` is the
//!    constraint made unit length ([`unit_direction`]) and oriented to the material-free side
//!    (rule 4; a zero orientation product is `InvalidParams{field:
//!    "constraints.fixed_axis_direction"}`). On the centred, scaled data `q_i`,
//!    `t_i = red::dot(q_i, n)`, `t_max` their total-order maximum, and the depths are
//!    `(t_max - t_i) * scale`, so the extreme point's depth is exactly 0 and no depth is
//!    negative. The plane passes through `centroid + t_max * scale * n`. The objective is the
//!    largest depth (minimax), `red::sum` of the depths (L1) or `red::sum_sq` of the depths
//!    (L2, m^2). This is the exact global optimum of every variant for that normal (the
//!    objective grows with the offset, which the constraints bound below by `t_max`): no
//!    iteration and no certificate, [`Optimality::ClosedForm`], and a rocking report of the one
//!    candidate (`rocking = false`).
//! 3. **Free normal**: constrained L2 is refused with [`FitError::NotCertifiable`] (its optimum
//!    generally touches fewer points than it has parameters, so it is not a vertex and the
//!    certificate pipeline cannot prove it; see the leaf's Open questions). Otherwise a seed is
//!    required when random starts are used (`InvalidParams{field: "seed"}`), then
//!    `ls_lin::PlaneLs` gives the least-squares normal `n0` (its `TooFewPoints`, `Collinear` and
//!    `IllConditioned` errors pass through).
//! 4. **Orientation**: the outward direction is the material side's direction
//!    (`MaterialSide::Outward`) or the `red::sum` of `red::dot(normal_i, n0)` over the points
//!    (`MaterialSide::PointNormals`); `n0` is negated when its `red::dot` with the outward
//!    direction (or that sum) is negative; zero is `InvalidParams{field: "material_side"}`.
//! 5. **Frame**: `k` the axis with the smallest `|n0_k|` (lowest index on ties),
//!    `e1 = unit_direction(a_k - n0_k n0)` with `a_k` the unit vector of axis `k`,
//!    `e2 = unit_direction(n0 x e1)`. Local data `u_i = red::dot(q_i, e1)`,
//!    `v_i = red::dot(q_i, e2)`, `w_i = red::dot(q_i, n0) / k` on the centred, scaled points
//!    `q_i` of `contract::Centred`, with the *form scale* `k` the largest power of two not above
//!    `max w - min w` (1 when that range is zero or subnormal), so the form and the slopes are of
//!    order 1 however flat the plate is (dividing by `k` is exact). These floats **are** the
//!    problem data: the certificate is exact for them.
//! 6. **Formulation** (`problem`): tangent plane `w = c + a u + b v` (form-scaled units).
//!    Minimax: parameters `(a, b, c, e)`, rows `w_i - c - a u_i - b v_i <= 0` and
//!    `e + a u_i + b v_i - w_i <= 0`, value `(c - e) / s`; L1: parameters `(a, b, c)`, rows
//!    `w_i - c - a u_i - b v_i <= 0`, value `(N c + a U + b V - W) / s` (`U, V, W` the
//!    `red::sum` of the columns), with `s = sqrt(1 + k^2 (a^2 + b^2))`; the objective in
//!    centred units is `k` times the value. Every row is linear; the value is not. The optimum of both is
//!    a vertex: minimax rests on a facet-vertex or edge-edge pair of the hull (4 rows), L1 on the
//!    hull facet nearest the centroid (3 rows). Background: the constrained L1 objective equals
//!    `N` times the distance from the centroid to the tangent plane; published work on datum
//!    plane fitting by constrained least squares and L1 (Shakarji and Srinivasan, around 2012
//!    to 2015 [unverified citation]).
//! 7. **Starts** (slopes `(a, b)`, each lifted onto the data: `c = max_i (w_i - a u_i - b v_i)`,
//!    and for minimax `e` the minimum): start 0 is `(0, 0)` (the least-squares plane shifted to
//!    the extreme point); starts 1 to 4 are the ring `(t, 0)`, `(0, t)`, `(-t, 0)`, `(0, -t)`;
//!    the rest come from `fid_solv::mstart::generate_starts` with primary `(0, 0)`, half widths
//!    `(t, t)`, the caller's seed and stream [`STREAM`] (its primary dropped). The tilt is
//!    `t = (max w - min w) / max(max |u|, max |v|)` (form-scaled `w`), the slope the form error
//!    subtends. Each
//!    start runs `fid_solv::seq::solve` (`CertKind::Tangential`, default constants) through
//!    `mstart::run_listed` with agreement tolerances [`AGREE_TOL`].
//! 8. **Certification**: only a `seq` result that `fid_solv::cert::certify` certified is a
//!    solution; a start that fails is recorded and dropped, never replaced by its iterate. No
//!    certified start is [`FitError::SeqFailed`] with the first start's `SeqCause::code()`.
//!    The certificate proves a KKT vertex with positive multipliers. The constraints are
//!    linear but the objective is not, so that is a **strict local minimum**
//!    ([`Optimality::StrictLocal`]), not a proven global one.
//! 9. **Candidates**: from each certified start's parameters: depths
//!    `k * ((((c + a u_i) + b v_i) - w_i) / s)` times the scale (metres), contacts (depth
//!    `<= side_tol`, ascending), maximum violation `max(0, -min depth)`, objective `k` times the
//!    certified value times the scale, feature the plane through the foot of the perpendicular
//!    from the centroid, `centroid + ((k c) / s) * scale * n`, with the outward unit normal
//!    `n = unit_direction(n0 - (k a) e1 - (k b) e2)`. A certified candidate whose violation exceeds
//!    `side_tol` is [`FitError::SideViolation`] (fail closed). All candidates go to
//!    `tan_rock::rock_report` with the caller's `rock_tol` (REQ-003); the selected candidate is
//!    the result, with the certificate of the first start (in start order) that produced it. When
//!    the certified starts disagree, or a start met its stop rule but failed certification, its
//!    certificate is flagged `local_optimum_only` (KRN.SOLV.SEQ).
//!
//! # What enters the digest
//!
//! [`encode`] writes only values derived from certified parameters (and the caller's inputs):
//! never an iterate, an iteration count or a per-start history. Floats are the `F64Bits` form
//! `0x` + 16 lowercase hex digits; the certificate enters by its `sha256:` digest
//! (`fid.cert@1`, which itself excludes every iterate). [`output_digest`] is the SHA-256 of
//! those bytes.
//!
//! # Determinism
//!
//! D0 for a given seed: single thread, no FMA, no `f64::min`/`max` (total-order helpers of
//! `fid_num::f`), every sum and dot product through `fid_num::red`, the KNP-1 conic solver of
//! `fid-solv`, no hash containers. Results and their bytes are bit-identical on x86-64, ARM64
//! and wasm32.

// Indexed loops over the parallel `u`, `v`, `w` columns mirror the pinned formulas term by term.
#![allow(clippy::needless_range_loop)]

mod encode;
mod problem;

pub use encode::{encode, output_digest};
pub use problem::{OFFSET_SCALE, SLOPE_SCALE};

use fid_num::f::{total_max, total_min};
use fid_num::{red, NumError};
use fid_solv::cert::{CertKind, Certificate, Certified};
use fid_solv::mstart::{self, MultiStartError, StartError, StartSpec, StartSummary, MAX_STARTS};
use fid_solv::seq::{self, SeqConfig};
use fid_types::Determinism;

use crate::contract::{
    self, check_points, unit_direction, AlgorithmId, Centred, CommandRow, Constraint, Field, Fit, FitDiagnostics,
    FitError, FitParams, FitResult, FittedFeature, Method, Normalisation, PointsView, Primitive, ResidualStats,
    StopReason, ValueType, Weights, V3,
};
use crate::ls_lin::PlaneLs;
use crate::tan_rock::{check_defined, rock_report, CandidateSet, MaterialSide, RockReport, TanVariant};

use problem::{Form, PlaneProblem};

/// Version of the three algorithms.
pub const VERSION: (u32, u32, u32) = (1, 0, 0);

/// Default number of starts (the performance budget's 8).
pub const DEFAULT_STARTS: u32 = 8;

/// Number of deterministic ring starts after the primary (rule 7).
pub const RING_STARTS: u32 = 4;

/// The `mstart` stream of the random starts.
pub const STREAM: u64 = 0;

/// Per-parameter agreement tolerance of the starts (centred, scaled units).
pub const AGREE_TOL: f64 = 1e-9;

const OP: &str = "fit.tan_plane";

/// The tangential parameters of a plane fit (the rest are [`FitParams`]).
#[derive(Debug, Clone, PartialEq)]
pub struct TanPlaneParams {
    /// Required (`None` is `InvalidParams{field: "variant"}`); there is no default.
    pub variant: Option<TanVariant>,
    pub material_side: MaterialSide,
    /// Largest accepted violation and contact band (metres), finite, not negative.
    pub side_tol: f64,
    /// Relative rocking tolerance of `tan_rock::rock_report`, in `[0, 1]`.
    pub rock_tol: f64,
    /// Number of starts, `1..=MAX_STARTS` (default [`DEFAULT_STARTS`]).
    pub starts: u32,
}

impl TanPlaneParams {
    /// Parameters with `DEFAULT_STARTS` starts.
    pub fn new(variant: Option<TanVariant>, material_side: MaterialSide, side_tol: f64, rock_tol: f64) -> Self {
        TanPlaneParams { variant, material_side, side_tol, rock_tol, starts: DEFAULT_STARTS }
    }
}

/// What the result's optimality is proven to be.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Optimality {
    /// Fixed normal: the exact optimum of the variant for that normal, by construction
    /// (no iteration, no certificate).
    ClosedForm,
    /// Free normal: a certified KKT vertex with positive multipliers of a problem with linear
    /// constraints and a nonlinear objective, so a strict local minimum (not proven global).
    StrictLocal,
}

impl Optimality {
    /// The wire name: `closed_form`, `strict_local`.
    pub const fn as_str(self) -> &'static str {
        match self {
            Optimality::ClosedForm => "closed_form",
            Optimality::StrictLocal => "strict_local",
        }
    }
}

/// The full result of a tangential plane fit.
#[derive(Debug, Clone, PartialEq)]
pub struct TanPlaneResult {
    /// `fit.plane.<tan_minimax|tan_l2|tan_l1>@1.0.0`.
    pub algorithm: AlgorithmId,
    pub variant: TanVariant,
    /// `Plane { p, n }` with `n` the **outward** unit normal (towards the material-free side;
    /// not sign-normalised, because its sign is the side).
    pub feature: FittedFeature,
    /// The variant's objective: metres (minimax, L1) or square metres (L2).
    pub objective: f64,
    /// Depth of every point below the plane (metres, into the material), in point order.
    pub residuals: Vec<f64>,
    pub stats: ResidualStats,
    /// Points with depth `<= side_tol`, ascending.
    pub contacts: Vec<u32>,
    /// `max(0, -min depth)` (metres), at most `side_tol`.
    pub max_violation: f64,
    pub side_tol: f64,
    pub optimality: Optimality,
    /// The `fid.cert@1` certificate (free normal only).
    pub certificate: Option<Certificate>,
    /// Every candidate within `rock_tol` (REQ-003); the selected one is this result.
    pub rock: RockReport,
    /// The seed (recorded for the journal; `None` when no random start was used).
    pub seed: Option<u64>,
    /// Starts run (diagnostic; not in the digest).
    pub starts: u32,
    /// Starts without a certified result (diagnostic; not in the digest).
    pub failed_starts: u32,
}

/// The algorithm ID of a variant.
pub fn algorithm_id(variant: TanVariant) -> AlgorithmId {
    AlgorithmId {
        module: "fit".to_string(),
        feature: Primitive::Plane.slug().to_string(),
        method: variant.method_segment().to_string(),
        version: VERSION,
    }
}

/// A tangential plane fit with a validated variant, for the [`Fit`] trait.
#[derive(Debug, Clone, PartialEq)]
pub struct TanPlane {
    variant: TanVariant,
    params: TanPlaneParams,
}

impl TanPlane {
    /// Refuses parameters without a variant (`InvalidParams{field: "variant"}`, REQ-001).
    pub fn new(params: TanPlaneParams) -> Result<TanPlane, FitError> {
        let variant = TanVariant::require(params.variant)?;
        Ok(TanPlane { variant, params })
    }

    /// The variant.
    pub fn variant(&self) -> TanVariant {
        self.variant
    }

    /// The tangential parameters.
    pub fn params(&self) -> &TanPlaneParams {
        &self.params
    }
}

impl Fit for TanPlane {
    fn algorithm_id(&self) -> AlgorithmId {
        algorithm_id(self.variant)
    }

    /// The contract summary of [`fit_tan_plane`]: the feature (canonical, so its normal is
    /// sign-normalised), the depths as residuals, diagnostics `rank` = parameter count (1 for a
    /// fixed normal), `cond` = the certificate's condition estimate (0 for a fixed normal),
    /// `grad_norm` 0 and `iterations` 0 (iterate values are not values of record), stop
    /// `exchange` (`closed_form` for a fixed normal), and a contract certificate with the
    /// contacts as support and the certificate's proven value bound (metres) as gap (0 for a
    /// fixed normal).
    fn fit(&self, pts: &PointsView<'_>, params: &FitParams, seed: Option<u64>) -> Result<FitResult, FitError> {
        let r = fit_tan_plane(pts, params, &self.params, seed)?;
        let scale = Centred::new(pts)?.map().scale();
        let (rank, cond, stop, gap) = match &r.certificate {
            Some(c) => (c.params().len(), c.cond(), StopReason::Exchange, c.value_bound() * scale),
            None => (1, 0.0, StopReason::ClosedForm, 0.0),
        };
        let diagnostics = FitDiagnostics { grad_norm: 0.0, rank, cond, iterations: 0, stop };
        let mut support = contract_alloc(r.contacts.len())?;
        support.extend(r.contacts.iter().map(|&i| i as usize));
        let cert = contract::Certificate { support, gap };
        FitResult::assemble(&r.feature, r.residuals, diagnostics, Some(cert), r.algorithm)
    }
}

fn contract_alloc<T>(n: usize) -> Result<Vec<T>, FitError> {
    let mut v = Vec::new();
    v.try_reserve_exact(n).map_err(|_| FitError::Num(NumError::Domain { op: OP, detail: "allocation_failed" }))?;
    Ok(v)
}

fn check_params(params: &FitParams, tp: &TanPlaneParams, pts: &PointsView<'_>) -> Result<TanVariant, FitError> {
    let variant = TanVariant::require(tp.variant)?;
    check_defined(Primitive::Plane, variant)?;
    params.validate(pts)?;
    if params.method != Method::Tangential {
        return Err(FitError::InvalidParams { field: "method" });
    }
    if params.weights != Weights::Uniform {
        return Err(FitError::InvalidParams { field: "weights" });
    }
    if params.constraints.iter().any(|c| !matches!(c, Constraint::FixedAxisDirection(_))) {
        return Err(FitError::InvalidParams { field: "constraints" });
    }
    if !(tp.side_tol.is_finite() && tp.side_tol >= 0.0) {
        return Err(FitError::InvalidParams { field: "side_tol" });
    }
    if !(tp.rock_tol.is_finite() && (0.0..=1.0).contains(&tp.rock_tol)) {
        return Err(FitError::InvalidParams { field: "rock_tol" });
    }
    if tp.starts == 0 || tp.starts > MAX_STARTS {
        return Err(FitError::InvalidParams { field: "starts" });
    }
    tp.material_side.validate(pts)?;
    Ok(variant)
}

/// Fits the tangential plane (module documentation, rules 1 to 9).
pub fn fit_tan_plane(
    pts: &PointsView<'_>,
    params: &FitParams,
    tp: &TanPlaneParams,
    seed: Option<u64>,
) -> Result<TanPlaneResult, FitError> {
    let variant = check_params(params, tp, pts)?;
    if let Some(d) = params.fixed_axis_direction() {
        return fixed_normal(pts, variant, tp, d);
    }
    let form = match variant {
        TanVariant::ExternalMinimax => Form::Minimax,
        TanVariant::ConstrainedL1 => Form::L1,
        TanVariant::ConstrainedL2 => return Err(FitError::NotCertifiable { variant }),
    };
    let random = tp.starts.saturating_sub(1 + RING_STARTS);
    let seed = if random > 0 { Some(seed.ok_or(FitError::InvalidParams { field: "seed" })?) } else { None };
    free_normal(pts, variant, form, params, tp, seed)
}

/// The outward orientation product of `n` (rule 4).
fn orientation(side: &MaterialSide, pts: &PointsView<'_>, n: V3) -> Result<f64, FitError> {
    match side {
        MaterialSide::Outward(u) => Ok(red::dot(&n, u)?),
        MaterialSide::PointNormals => {
            let mut dots = contract_alloc(pts.len())?;
            for i in 0..pts.len() {
                let nm = pts.normal(i).ok_or(FitError::InvalidParams { field: "material_side" })?;
                dots.push(red::dot(&nm, &n)?);
            }
            Ok(red::sum(&dots)?)
        }
    }
}

fn orient(side: &MaterialSide, pts: &PointsView<'_>, n: V3, field: &'static str) -> Result<V3, FitError> {
    let o = orientation(side, pts, n)?;
    if o == 0.0 {
        return Err(FitError::InvalidParams { field });
    }
    Ok(if o < 0.0 { [-n[0] + 0.0, -n[1] + 0.0, -n[2] + 0.0] } else { n })
}

fn contacts_and_violation(depths: &[f64], side_tol: f64) -> Result<(Vec<u32>, f64, usize), FitError> {
    let mut contacts = Vec::new();
    let mut min = f64::INFINITY;
    let mut argmin = 0usize;
    for (i, &d) in depths.iter().enumerate() {
        if d <= side_tol {
            contacts
                .try_reserve(1)
                .map_err(|_| FitError::Num(NumError::Domain { op: OP, detail: "allocation_failed" }))?;
            // Point counts are below MAX_POINTS < 2^32.
            contacts.push(i as u32);
        }
        if d < min {
            min = d;
            argmin = i;
        }
    }
    let violation = total_max(0.0, -min)?;
    Ok((contacts, violation, argmin))
}

fn fixed_normal(
    pts: &PointsView<'_>,
    variant: TanVariant,
    tp: &TanPlaneParams,
    d: V3,
) -> Result<TanPlaneResult, FitError> {
    if pts.is_empty() {
        return Err(FitError::TooFewPoints { n: 0, need: 1 });
    }
    let n = unit_direction(d)?;
    let n = orient(&tp.material_side, pts, n, "constraints.fixed_axis_direction")?;
    let c = Centred::new(pts)?;
    let map = c.map();
    let q = c.xyz();
    let mut t = contract_alloc(pts.len())?;
    let mut tmax = f64::NEG_INFINITY;
    for qi in q.chunks_exact(3) {
        let ti = red::dot(qi, &n)?;
        tmax = total_max(tmax, ti)?;
        t.push(ti);
    }
    let mut depths = contract_alloc(pts.len())?;
    for &ti in &t {
        depths.push(map.inverse_length(tmax - ti)?);
    }
    let objective = match variant {
        TanVariant::ExternalMinimax => {
            let mut m = 0.0_f64;
            for &x in &depths {
                m = total_max(m, x)?;
            }
            m
        }
        TanVariant::ConstrainedL1 => red::sum(&depths)?,
        TanVariant::ConstrainedL2 => red::sum_sq(&depths)?,
    };
    let p = map.inverse([tmax * n[0], tmax * n[1], tmax * n[2]])?;
    let feature = FittedFeature::Plane { p, n };
    let (contacts, max_violation, _) = contacts_and_violation(&depths, tp.side_tol)?;
    let cand = CandidateSet { contacts: contacts.clone(), objective, feature: feature.clone() };
    let rock = rock_report(&[cand], tp.rock_tol)?;
    let stats = ResidualStats::of(&depths)?;
    Ok(TanPlaneResult {
        algorithm: algorithm_id(variant),
        variant,
        feature,
        objective,
        residuals: depths,
        stats,
        contacts,
        max_violation,
        side_tol: tp.side_tol + 0.0,
        optimality: Optimality::ClosedForm,
        certificate: None,
        rock,
        seed: None,
        starts: 0,
        failed_starts: 0,
    })
}

/// The form scale of rule 5: the largest power of two not above `range` (the range of the `w`
/// column), or 1 when `range` is zero or subnormal.
fn form_scale(range: f64) -> f64 {
    if range.is_normal() && range > 0.0 {
        f64::from_bits(range.to_bits() & 0xFFF0_0000_0000_0000)
    } else {
        1.0
    }
}

/// The orthonormal local frame of rule 5.
struct Frame {
    e1: V3,
    e2: V3,
    n: V3,
}

fn frame(n: V3) -> Result<Frame, FitError> {
    let mut k = 0usize;
    for j in 1..3 {
        if n[j].abs() < n[k].abs() {
            k = j;
        }
    }
    let mut a = [0.0; 3];
    a[k] = 1.0;
    let e1 = unit_direction([a[0] - n[k] * n[0], a[1] - n[k] * n[1], a[2] - n[k] * n[2]])?;
    let e2 = unit_direction([n[1] * e1[2] - n[2] * e1[1], n[2] * e1[0] - n[0] * e1[2], n[0] * e1[1] - n[1] * e1[0]])?;
    Ok(Frame { e1, e2, n })
}

/// One certified start, mapped to metres.
struct Found {
    certified: Certified,
    cand: CandidateSet,
    depths: Vec<f64>,
    max_violation: f64,
    worst: usize,
}

fn candidate(
    prob: &PlaneProblem,
    fr: &Frame,
    map: &Normalisation,
    k: f64,
    certified: Certified,
    side_tol: f64,
) -> Result<Found, FitError> {
    let p = certified.params();
    let (a, b, c) = (p[0], p[1], p[2]);
    let s = prob.slope_norm(a, b)?;
    let (ka, kb) = (k * a, k * b);
    let n = unit_direction([
        (fr.n[0] - ka * fr.e1[0]) - kb * fr.e2[0],
        (fr.n[1] - ka * fr.e1[1]) - kb * fr.e2[1],
        (fr.n[2] - ka * fr.e1[2]) - kb * fr.e2[2],
    ])?;
    let d0 = (k * c) / s;
    let point = map.inverse([d0 * n[0], d0 * n[1], d0 * n[2]])?;
    let scaled = prob.depths(p)?;
    let mut depths = contract_alloc(scaled.len())?;
    for &x in &scaled {
        depths.push(map.inverse_length(k * x)?);
    }
    let (contacts, max_violation, worst) = contacts_and_violation(&depths, side_tol)?;
    let objective = map.inverse_length(k * certified.value())?;
    let feature = FittedFeature::Plane { p: point, n };
    Ok(Found { certified, cand: CandidateSet { contacts, objective, feature }, depths, max_violation, worst })
}

/// The driver constants (rule 7): `SeqConfig::new(CertKind::Tangential)`, the documented
/// defaults of `fid_solv::seq` and of the certificate pipeline.
pub fn seq_config() -> SeqConfig {
    SeqConfig::new(CertKind::Tangential)
}

fn seq_cause(e: &StartError<seq::SolverError>) -> &'static str {
    match e {
        StartError::Solver(s) => s.cause.code(),
        StartError::Numeric(_) => "numeric",
        StartError::ParamLength { .. } => "param_length",
    }
}

fn free_normal(
    pts: &PointsView<'_>,
    variant: TanVariant,
    form: Form,
    params: &FitParams,
    tp: &TanPlaneParams,
    seed: Option<u64>,
) -> Result<TanPlaneResult, FitError> {
    let (c, _) = check_points(Primitive::Plane, pts, &params.thresholds)?;
    let mut ls = FitParams::new(Method::LeastSquares);
    ls.thresholds = params.thresholds;
    let n0 = match PlaneLs.fit(pts, &ls, None)?.feature {
        FittedFeature::Plane { n, .. } => n,
        _ => return Err(FitError::InvalidParams { field: "method" }),
    };
    let n0 = orient(&tp.material_side, pts, n0, "material_side")?;
    let fr = frame(n0)?;
    let map = *c.map();
    let mut u = contract_alloc(pts.len())?;
    let mut v = contract_alloc(pts.len())?;
    let mut w = contract_alloc(pts.len())?;
    let (mut wmax, mut wmin) = (f64::NEG_INFINITY, f64::INFINITY);
    for q in c.xyz().chunks_exact(3) {
        u.push(red::dot(q, &fr.e1)?);
        v.push(red::dot(q, &fr.e2)?);
        let wi = red::dot(q, &fr.n)?;
        wmax = total_max(wmax, wi)?;
        wmin = total_min(wmin, wi)?;
        w.push(wi);
    }
    let k = form_scale(wmax - wmin);
    for wi in w.iter_mut() {
        *wi /= k;
    }
    let prob = PlaneProblem::new(form, u, v, w, k)?;

    // Rule 7: the tilt and the starts.
    let (cu, cv, cw) = prob.columns();
    let (mut wmax, mut wmin, mut ext) = (f64::NEG_INFINITY, f64::INFINITY, 0.0_f64);
    for i in 0..prob.len() {
        wmax = total_max(wmax, cw[i])?;
        wmin = total_min(wmin, cw[i])?;
        ext = total_max(ext, cu[i].abs())?;
        ext = total_max(ext, cv[i].abs())?;
    }
    let t = if ext > 0.0 { (wmax - wmin) / ext } else { 0.0 };
    let t = fid_num::check_finite(OP, t)?;
    let ring = [(t, 0.0), (0.0, t), (-t, 0.0), (0.0, -t)];
    let mut slopes: Vec<(f64, f64)> = contract_alloc(tp.starts as usize)?;
    slopes.push((0.0, 0.0));
    for &r in ring.iter().take((tp.starts - 1).min(RING_STARTS) as usize) {
        slopes.push(r);
    }
    let random = tp.starts.saturating_sub(1 + RING_STARTS);
    if let (true, Some(seed)) = (random > 0, seed) {
        let spec = StartSpec {
            primary: vec![0.0, 0.0],
            half_widths: vec![t, t],
            count: random + 1,
            seed,
            stream: STREAM,
            tolerances: vec![0.0, 0.0],
        };
        let gen = mstart::generate_starts(&spec).map_err(|_| FitError::InvalidParams { field: "starts" })?;
        for s in gen.iter().skip(1) {
            slopes.push((s[0], s[1]));
        }
    }
    let mut starts = contract_alloc(slopes.len())?;
    for &(a, b) in &slopes {
        starts.push(prob.lift(a, b)?);
    }

    let cfg = seq_config();
    let np = starts.first().map_or(0, Vec::len);
    let tolerances = vec![AGREE_TOL; np];
    let mut certified: Vec<Option<Certified>> = contract_alloc(starts.len())?;
    let run = mstart::run_listed(&starts, &tolerances, |start| match seq::solve(&prob, start, &cfg) {
        Ok(out) => {
            let value = out.certified.value();
            let params = out.certified.params().to_vec();
            certified.push(Some(out.certified));
            Ok(((), value, params))
        }
        Err(e) => {
            certified.push(None);
            Err(e)
        }
    });
    let out = match run {
        Ok(o) => o,
        Err(MultiStartError::AllFailed(errs)) => {
            let cause = errs.first().map_or("no_starts", seq_cause);
            return Err(FitError::SeqFailed { cause, starts: tp.starts });
        }
        Err(MultiStartError::InvalidSpec(_)) => return Err(FitError::InvalidParams { field: "starts" }),
    };
    let converged_but_failed =
        out.per_start.iter().any(|s| matches!(s, StartSummary::Failed(StartError::Solver(e)) if e.converged));
    let local_only = out.local_optimum_only || converged_but_failed;
    let failed = out.per_start.iter().filter(|s| matches!(s, StartSummary::Failed(_))).count();

    let mut found = contract_alloc::<Found>(certified.len())?;
    for c in certified.into_iter().flatten() {
        let f = candidate(&prob, &fr, &map, k, c, tp.side_tol)?;
        if f.max_violation > tp.side_tol {
            return Err(FitError::SideViolation { index: f.worst, violation: f.max_violation });
        }
        found.push(f);
    }
    let mut cands = contract_alloc(found.len())?;
    for f in &found {
        cands.push(f.cand.clone());
    }
    let rock = rock_report(&cands, tp.rock_tol)?;
    let sel = rock.selected().ok_or(FitError::InvalidParams { field: "candidates" })?;
    let pick = found
        .into_iter()
        .find(|f| {
            f.cand.contacts == sel.contacts
                && f.cand.objective.to_bits() == sel.objective.to_bits()
                && f.cand.feature == sel.feature
        })
        .ok_or(FitError::InvalidParams { field: "candidates" })?;
    let mut cert = pick.certified;
    if local_only {
        cert.set_local_optimum_only().map_err(|_| FitError::Num(NumError::Domain { op: OP, detail: "cert_flag" }))?;
    }
    let stats = ResidualStats::of(&pick.depths)?;
    Ok(TanPlaneResult {
        algorithm: algorithm_id(variant),
        variant,
        feature: pick.cand.feature,
        objective: pick.cand.objective,
        residuals: pick.depths,
        stats,
        contacts: pick.cand.contacts,
        max_violation: pick.max_violation,
        side_tol: tp.side_tol + 0.0,
        optimality: Optimality::StrictLocal,
        certificate: Some(cert.into_certificate()),
        rock,
        seed,
        starts: tp.starts,
        // At most MAX_STARTS.
        failed_starts: failed as u32,
    })
}

const fn fld(
    name: &'static str,
    ty: ValueType,
    quantity: Option<(&'static str, &'static str)>,
    required: bool,
) -> Field {
    Field { name, ty, quantity, required }
}

const D: Option<(&str, &str)> = Some(("dimensionless", "1"));
const L: Option<(&str, &str)> = Some(("length", "m"));
const C: Option<(&str, &str)> = Some(("count", "1"));

/// The tangential parameters, appended to the contract's common parameters.
pub const TAN_PARAMS: [Field; 6] = [
    fld("variant", ValueType::String, None, true),
    fld("material_side", ValueType::String, None, true),
    fld("material_side.outward", ValueType::NumberArray, D, false),
    fld("side_tol", ValueType::Number, L, true),
    fld("rock_tol", ValueType::Number, D, true),
    fld("starts", ValueType::Integer, C, true),
];

/// The tangential result fields, appended to the contract's common result fields.
pub const TAN_RESULT: [Field; 9] = [
    fld("variant", ValueType::String, None, true),
    fld("objective", ValueType::Number, L, true),
    fld("contacts", ValueType::IntegerArray, C, true),
    fld("max_violation", ValueType::Number, L, true),
    fld("optimality", ValueType::String, None, true),
    fld("certificate.digest", ValueType::String, None, false),
    fld("rock.selected", ValueType::Integer, C, true),
    fld("rock.rocking", ValueType::Boolean, None, true),
    fld("rock.tie_break", ValueType::String, None, true),
];

/// The command data row of `fit.tan_plane@1` (W2, dry run R1, D0 per seed): the contract's
/// row for `(plane, tangential)` (algorithm family `fit.plane.tan@1.0.0`; the result's
/// `algorithm` field names the variant's ID) with [`TAN_PARAMS`] and [`TAN_RESULT`] appended.
pub fn command_row() -> Result<CommandRow, FitError> {
    let mut row = contract::command_row(Primitive::Plane, Method::Tangential, VERSION, Determinism::D0)?;
    row.params
        .try_reserve_exact(TAN_PARAMS.len())
        .map_err(|_| FitError::Num(NumError::Domain { op: OP, detail: "allocation_failed" }))?;
    row.params.extend_from_slice(&TAN_PARAMS);
    row.result
        .try_reserve_exact(TAN_RESULT.len())
        .map_err(|_| FitError::Num(NumError::Domain { op: OP, detail: "allocation_failed" }))?;
    row.result.extend_from_slice(&TAN_RESULT);
    Ok(row)
}
