//! Maximum inscribed 2D circle and sphere. Spec node `KRN.ENV.MIC.CIRSPH`.
//!
//! The largest circle (in a projection plane) or sphere that contains no point: the mating
//! envelope of a hole section or a spherical socket. The problem is non-convex and, on partial
//! data, unbounded, so the centre is held in the trust region of `KRN.ENV.MIC.TR`
//! ([`TrustRegion`]) about the least-squares fit, solved by sequential linearisation
//! (`fid_solv::seq`) from seeded starts (`fid_solv::mstart`), certified (`fid_solv::cert`) and
//! cross-checked for global optimality by a branch and bound over the region.
//! [`fit_mic_circle2d`] and [`fit_mic_sphere`] return the full [`MicFit`] record;
//! [`MicCircle2Fit`] and [`MicSphereFit`] implement [`Fit`] for `fit.circle2.mi@1.0.0` and
//! `fit.sphere.mi@1.0.0` and fail closed on a result whose global optimality is not proven.
//! The algorithm-ID and command segments are the shared contract's slugs (`circle2`, `mi`), as
//! for `ls_rnd` and `mic_tr`; the leaf's text says `circle2d` / `mic`.
//!
//! # Pinned procedure
//!
//! 1. **Parameters.** [`FitParams::validate`]; the method must be
//!    [`Method::MaximumInscribed`] and the weights uniform (`InvalidParams` `method` /
//!    `weights`). Constraints: a projection plane for the circle, none for the sphere
//!    (`InvalidParams` `constraints`). [`MicParams::check`]: `tr_fraction` finite and positive
//!    (`tr_fraction`, required, no default), `boundary_tol` finite (`boundary_tol`), `starts`
//!    from 1 to `mstart::MAX_STARTS` (`starts`), and `seq` passing `SeqConfig::check` with kind
//!    `Envelope` (`seq`).
//! 2. **Projection** (circle only), as `ls_rnd`'s circle: the points are projected onto the
//!    plane's frame `ls_lin::projection_frame` (subnormal coordinates flushed to `+0`), `z =
//!    0`, and the circle is reported in that frame's coordinates.
//! 3. **Least-squares start.** `ls_rnd::Circle2Ls` or `ls_rnd::SphereLs` on the (projected)
//!    points with the caller's thresholds and NLLS configuration; its errors pass through
//!    (`TooFewPoints`, `Collinear`, `Coplanar`, `InsufficientCoverage`, `IllConditioned`,
//!    `NotConverged`, ...).
//! 4. **Trust region.** `TrustRegion::from_feature(ls feature, tr_fraction)` in metres (the
//!    region of record), and `boundary_tol` at least its `boundary_tol_floor`
//!    (`InvalidParams` `boundary_tol` otherwise, `0` included). The fit runs on the centred,
//!    scaled data of [`Centred`] (`check_points`); the scaled region is
//!    `TrustRegion::from_feature` of the least-squares feature mapped by the centring map
//!    (centre `forward`, radius `forward_length`), so its rows are built by the same rules.
//! 5. **Formulation** (`problem`): parameters `(c, r)` in scaled units, rows `r - |q_i - c|
//!    <= 0` (label `(i, 0)`) and the region's facets `a_k . c <= b_k` (label `(k, 1)`),
//!    objective `-r`. Every `fid_solv::seq` step is the LP of the rows linearised about the
//!    current centre, `[(q_i - c) / rho_i, 1] . (dc, dr) <= rho_i - r`, plus the facet rows:
//!    the tangent plane of the convex `rho_i` lies below it, so every LP step is feasible. Step
//!    scales: the scaled region circumradius for every parameter. KKT data scale: the largest
//!    `|q_ij|` and `|b_k|`.
//! 6. **Recomputation start** (`start_from_set`): the closed-form centre equidistant from the
//!    set's points and on its facets (a `dim x dim` linear solve), radius the distance to the
//!    first point.
//! 7. **Starts.** `count = starts` (default [`DEFAULT_STARTS`]) from the `StartSpec` with the
//!    least-squares centre as primary, half widths `inradius / 2` on the centre (so every
//!    start lies in the region: `sqrt(3) / 2 < 1`) and `0` on the radius, the seed (or
//!    [`DEFAULT_SEED`]) and stream [`STREAM`]; each start's radius is then set to the smallest
//!    working-set point distance at its centre (a feasible start with an active row).
//!    Tolerances [`AGREE_TOL`] times the primary's radius. `fid_solv::seq::solve` with `seq`
//!    from each start through `mstart::run_listed`; every start is certified inside `seq` (fail
//!    closed); no certified start is [`FitError::SeqFailed`] with the first start's cause. The
//!    selected start is the one with the largest certified radius (smallest `-r`).
//! 8. **Working set.** The conic solver's KKT budget (20 000 columns) bounds the rows of one
//!    LP and its cost grows with the rows, so a cloud of more than [`WORKING_ALL`] points is
//!    solved on a working set: first an evenly strided subsample of [`WORKING_STRIDED`] points
//!    (so every direction is represented) and the [`WORKING_SEED`] nearest points of every
//!    start centre (ties by index); after
//!    each multi-start round, every point outside the set within `(1 + WORKING_MARGIN) r` of a
//!    converged start's certified centre (radius `r`) joins it, and the round is repeated until
//!    no start leaves such a point (at most [`MAX_ROUNDS`] rounds and [`WORKING_CAP`] points,
//!    else `Num(Domain{detail: "working_set_limit"})`). The rows of the points left out are then
//!    strictly inactive at every converged start (slack above `WORKING_MARGIN r`), so a strict
//!    local maximum certified on the working set is one of the whole problem. Row labels carry
//!    the points' indices in the whole cloud. A cloud of at most `WORKING_ALL` points uses
//!    every point in one round.
//! 9. **Boundary** (REQ-003). A facet row in the selected certificate's active set, or a final
//!    centre (in metres) that fails `TrustRegion::check_boundary(centre, boundary_tol)`, is
//!    [`FitError::UnboundedEnvelope`] with the boundary distance and no envelope. This check
//!    comes before every other result check.
//! 10. **Result.** Contacts: the point rows of the active set, ascending. The radius is
//!     **recomputed from the contacts and the certified centre** (the smallest contact
//!     distance). Residuals `rho_i - r` in metres for every point of the cloud (positive
//!     outside the envelope); the largest penetration `max(0, -min_i residual)` is
//!     `max_violation`, and above [`INSIDE_TOL`] it is [`FitError::SideViolation`] (fail
//!     closed).
//! 11. **Global cross-check** (`global`). The certificate proves a strict local maximum
//!     (`n` independent active rows with positive multipliers), not a global one. A branch and
//!     bound over the region on the 1-Lipschitz empty-ball radius `f(c) = min_i |q_i - c|`
//!     over every point decides, with `tol = CROSS_TOL * r +` the certificate's value bound,
//!     whether some centre in the region has `f > r + tol` (`better`), none has (`passed`), or
//!     the limits ran out (`inconclusive`). Its documentation has the bounds and rounding
//!     margins.
//! 12. **Flags.** `optimality` is [`Optimality::Global`] when the cross-check passed, else
//!     [`Optimality::StrictLocal`]; `local_optimum_only` is set exactly when it did not pass
//!     (global optimality not proven, REQ-002), on the record and on the certificate.
//!     `agreement` (the certified starts agree) is reported separately: starts that certify a
//!     smaller local maximum do not unset a proven global one.
//! 13. **Trait (fail closed).** [`Fit::fit`] refuses a `local_optimum_only` result as
//!     [`FitError::FlaggedFit`] (`suspect_axis` false, `rms_ratio` 0: no ratio applies);
//!     [`fit_mic_circle2d`] and [`fit_mic_sphere`] return it with its flags.
//!
//! # Agreement job (REQ-004)
//!
//! [`trust_polygon`] gives the circle's 32-gon region as vertices (consecutive facet lines
//! intersected), so `xchk::mic_circle2d_vor` solves on the same polygon, and
//! [`check_agreement`] compares the two paths: centre and radius within a tolerance, and the
//! boundary status (`UnboundedEnvelope` against `on_region_boundary`).
//!
//! # Determinism
//!
//! D0 for a given seed: single thread, no FMA, no `f64::min`/`max`, every sum, dot product and
//! norm through `fid_num::red`, the KNP-1 conic solver of `fid-solv`, no hash containers.

// Indexed loops over the coordinates mirror the formulas term by term.
#![allow(clippy::needless_range_loop)]

mod global;
mod problem;

pub use crate::mcc_cyl::{CrossOutcome, Optimality};

use core::cmp::Ordering;

use fid_kci::AlgorithmId;
use fid_num::{f, red, NumError};
use fid_solv::cert::{CertKind, Certificate};
use fid_solv::mstart::{self, MultiStartError, StartError, StartOrigin, StartSpec, StartSummary, MAX_STARTS};
use fid_solv::seq::{self, SeqConfig, SolverError};
use fid_types::Determinism;

use crate::contract::{
    self, algorithm_id, check_points, command_row, Centred, CommandRow, Constraint, Fit, FitDiagnostics, FitError,
    FitParams, FitResult, FittedFeature, Method, PointsView, Primitive, StopReason, Weights, V2, V3,
};
use crate::ls_lin::projection_frame;
use crate::ls_rnd::{Circle2Ls, SphereLs};
use crate::mic_tr::{TrustRegion, DIRS2};
use crate::xchk::Inscribed;
use problem::{Mic, Row};

/// Algorithm version of `fit.circle2.mi` and `fit.sphere.mi`.
pub const VERSION: (u32, u32, u32) = (1, 0, 0);
/// The primitives of this module, in the order of [`command_rows`] and [`algorithm_ids`].
pub const PRIMITIVES: [Primitive; 2] = [Primitive::Circle2, Primitive::Sphere];
/// Seed when `seed` is `None`.
pub const DEFAULT_SEED: u64 = 0;
/// Start count (the primary included).
pub const DEFAULT_STARTS: u32 = 8;
/// `fid_num::rng` stream of the starts (INIT uses 1 to 3, `mcc_cyl` 4).
pub const STREAM: u64 = 5;
/// Agreement tolerance of the certified starts, times the start radius \[unverified\].
pub const AGREE_TOL: f64 = 1e-9;
/// Largest accepted penetration of a point into the envelope, metres (rule 10).
pub const INSIDE_TOL: f64 = 1e-12;
/// Relative tolerance of the global cross-check, on top of the certificate's value bound
/// (rule 11).
pub const CROSS_TOL: f64 = 1e-9;

/// Clouds of at most this many points are solved with every point in the working set (rule
/// 8); the conic solver's KKT budget is 20 000 columns.
pub const WORKING_ALL: usize = 2048;
/// Points of the evenly strided subsample in the initial working set of a larger cloud (rule
/// 8): every `ceil(n / WORKING_STRIDED)`-th point from index 0.
pub const WORKING_STRIDED: usize = 1024;
/// Nearest points per start centre in the initial working set of a larger cloud (rule 8).
pub const WORKING_SEED: usize = 64;
/// Largest working set (rule 8).
pub const WORKING_CAP: usize = 16384;
/// Relative margin of the working-set check: a point outside the set within `(1 +
/// WORKING_MARGIN) r` of a converged start's centre joins the set (`2^-20`).
pub const WORKING_MARGIN: f64 = 1.0 / 1_048_576.0;
/// Most working-set rounds (rule 8).
pub const MAX_ROUNDS: u32 = 16;

const OP: &str = "fit.mic_cirsph";

/// The parameters of a maximum inscribed fit beyond [`FitParams`] (spec Design).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MicParams {
    /// Trust-region size as a fraction of the least-squares radius (required, no default).
    pub tr_fraction: f64,
    /// Boundary tolerance of the unbounded-envelope check, metres (at least the region's
    /// rounding floor).
    pub boundary_tol: f64,
    /// Number of starts (the primary included).
    pub starts: u32,
    /// The sequential-linearisation constants (kind `Envelope`).
    pub seq: SeqConfig,
}

impl MicParams {
    /// `tr_fraction` and `boundary_tol` as given, [`DEFAULT_STARTS`] starts and the default
    /// `SeqConfig` for an envelope.
    pub fn new(tr_fraction: f64, boundary_tol: f64) -> MicParams {
        MicParams { tr_fraction, boundary_tol, starts: DEFAULT_STARTS, seq: SeqConfig::new(CertKind::Envelope) }
    }

    /// Rule 1's checks of these fields (the region-dependent floor of `boundary_tol` is
    /// checked once the region exists, rule 4).
    pub fn check(&self) -> Result<(), FitError> {
        if !(self.tr_fraction.is_finite() && self.tr_fraction > 0.0) {
            return Err(FitError::InvalidParams { field: "tr_fraction" });
        }
        if !self.boundary_tol.is_finite() {
            return Err(FitError::InvalidParams { field: "boundary_tol" });
        }
        if self.starts == 0 || self.starts > MAX_STARTS {
            return Err(FitError::InvalidParams { field: "starts" });
        }
        if self.seq.kind != CertKind::Envelope || self.seq.check().is_err() {
            return Err(FitError::InvalidParams { field: "seq" });
        }
        Ok(())
    }
}

/// The global cross-check of a fit (rule 11).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CrossCheck {
    pub outcome: CrossOutcome,
    /// Cells examined, point distances evaluated and the deepest cell level reached.
    pub cells: u32,
    pub work: u64,
    pub depth: u32,
    /// `better`: a centre (metres; `z = 0` for the circle) in the region and the radius of the
    /// empty circle or sphere about it (metres), larger than the fit's by more than the
    /// tolerance. It is a feasible point, not a certified optimum.
    pub better: Option<(V3, f64)>,
}

/// A maximum inscribed circle or sphere with its record (REQ-KRN.ENV.MIC.CIRSPH-002).
#[derive(Debug, Clone, PartialEq)]
pub struct MicFit {
    pub algorithm: AlgorithmId,
    /// `Circle2` (projection-plane coordinates) or `Sphere`, metres, radius recomputed from
    /// the contacts (rule 10).
    pub feature: FittedFeature,
    /// `rho_i - r` per point, metres (positive outside the envelope; not below
    /// `-INSIDE_TOL`).
    pub residuals: Vec<f64>,
    /// Point indices of the certified active set, ascending.
    pub contacts: Vec<u32>,
    /// `max(0, -min_i residual)`, metres.
    pub max_violation: f64,
    /// The certificate of the selected start, `local_optimum_only` set when flagged.
    pub certificate: Certificate,
    /// What is proven: a strict local maximum, or the global one (cross-check passed).
    pub optimality: Optimality,
    /// The certified starts agree within `AGREE_TOL` (`mstart`).
    pub agreement: bool,
    /// Global optimality not proven (the cross-check did not pass).
    pub local_optimum_only: bool,
    pub cross_check: CrossCheck,
    /// The trust region of record (metres) and the centre's distance from its boundary.
    pub trust_region: TrustRegion,
    pub boundary_distance_m: f64,
    /// The least-squares start's empty radius at the least-squares centre (metres).
    pub start_radius: f64,
    /// The starts (scaled units) and one outcome per start.
    pub starts: Vec<Vec<f64>>,
    pub per_start: Vec<StartSummary<SolverError>>,
    pub best_index: u32,
    pub origin: StartOrigin,
}

/// `fit.circle2.mi@1.0.0` through the [`Fit`] trait (rule 13).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MicCircle2Fit {
    pub mic: MicParams,
}

/// `fit.sphere.mi@1.0.0` through the [`Fit`] trait (rule 13).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MicSphereFit {
    pub mic: MicParams,
}

/// Why the conic and the Voronoi circle disagree (REQ-KRN.ENV.MIC.CIRSPH-004).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Disagreement {
    /// One path reports a boundary optimum and the other does not.
    Boundary { conic_on_boundary: bool, xchk_on_boundary: bool },
    /// The largest centre coordinate difference (metres) exceeds the tolerance.
    Centre { delta: f64 },
    /// The radius difference (metres) exceeds the tolerance.
    Radius { delta: f64 },
    /// The conic fit failed with an error other than `unbounded_envelope` (its code).
    ConicFailed { code: &'static str },
    /// The tolerance is not finite and positive.
    InvalidTolerance,
}

fn alloc<T>(n: usize) -> Result<Vec<T>, FitError> {
    let mut v = Vec::new();
    v.try_reserve_exact(n).map_err(|_| FitError::Num(NumError::Domain { op: OP, detail: "allocation_failed" }))?;
    Ok(v)
}

fn cert_err(e: fid_solv::cert::CertError) -> FitError {
    match e {
        fid_solv::cert::CertError::Numeric(n) => FitError::Num(n),
        _ => FitError::Num(NumError::Domain { op: OP, detail: "problem" }),
    }
}

/// The algorithm ID of `p` (`circle2` or `sphere`).
pub fn algorithm(p: Primitive) -> AlgorithmId {
    algorithm_id(p, Method::MaximumInscribed, VERSION).unwrap_or_else(|_| AlgorithmId {
        module: "fit".to_string(),
        feature: p.slug().to_string(),
        method: Method::MaximumInscribed.slug().to_string(),
        version: VERSION,
    })
}

/// The two algorithm IDs, in [`PRIMITIVES`] order (for the spec-card check).
pub fn algorithm_ids() -> [AlgorithmId; 2] {
    [algorithm(Primitive::Circle2), algorithm(Primitive::Sphere)]
}

/// The command data rows `fit.mi_circle2@1` and `fit.mi_sphere@1` (W2, dry run R1, D0 per
/// seed), in [`PRIMITIVES`] order.
pub fn command_rows() -> Result<Vec<CommandRow>, FitError> {
    let mut rows = alloc(PRIMITIVES.len())?;
    for p in PRIMITIVES {
        rows.push(command_row(p, Method::MaximumInscribed, VERSION, Determinism::D0)?);
    }
    Ok(rows)
}

fn seq_cause(e: &StartError<SolverError>) -> &'static str {
    match e {
        StartError::Solver(s) => s.cause.code(),
        StartError::Numeric(_) => "numeric",
        StartError::ParamLength { .. } => "param_length",
    }
}

fn flush(x: f64) -> f64 {
    if x.is_subnormal() {
        0.0
    } else {
        x
    }
}

/// Rule 1.
fn check_params(p: Primitive, pts: &PointsView<'_>, params: &FitParams, mic: &MicParams) -> Result<(), FitError> {
    params.validate(pts)?;
    if params.method != Method::MaximumInscribed {
        return Err(FitError::InvalidParams { field: "method" });
    }
    if params.weights != Weights::Uniform {
        return Err(FitError::InvalidParams { field: "weights" });
    }
    let ok = |c: &Constraint| matches!(c, Constraint::ProjectionPlane { .. }) && p == Primitive::Circle2;
    if !params.constraints.iter().all(ok) {
        return Err(FitError::InvalidParams { field: "constraints" });
    }
    mic.check()
}

/// The maximum inscribed circle in the projection plane (rules 1 to 12), with its record.
///
/// # Errors
///
/// `InvalidParams` (rules 1 and 4), the least-squares start's errors, [`FitError::SeqFailed`]
/// when no start certifies, [`FitError::UnboundedEnvelope`] (rule 9), [`FitError::SideViolation`] (rule 10)
/// and `Num` `working_set_limit` (rule 8).
pub fn fit_mic_circle2d(
    pts: &PointsView<'_>,
    params: &FitParams,
    mic: &MicParams,
    seed: Option<u64>,
) -> Result<MicFit, FitError> {
    check_params(Primitive::Circle2, pts, params, mic)?;
    let (p0, e1, e2) = projection_frame(params)?;
    let mut buf = alloc(pts.xyz().len())?;
    for x in pts.xyz().chunks_exact(3) {
        let dx = [x[0] - p0[0], x[1] - p0[1], x[2] - p0[2]];
        buf.push(flush(red::dot(&dx, &e1)?));
        buf.push(flush(red::dot(&dx, &e2)?));
        buf.push(0.0);
    }
    let view = PointsView::new(&buf, None)?;
    run(Primitive::Circle2, &view, params, mic, seed)
}

/// The maximum inscribed sphere (rules 1 to 12), with its record.
///
/// # Errors
///
/// As [`fit_mic_circle2d`].
pub fn fit_mic_sphere(
    pts: &PointsView<'_>,
    params: &FitParams,
    mic: &MicParams,
    seed: Option<u64>,
) -> Result<MicFit, FitError> {
    check_params(Primitive::Sphere, pts, params, mic)?;
    run(Primitive::Sphere, pts, params, mic, seed)
}

/// Rule 3: the least-squares feature.
fn ls_feature(prim: Primitive, view: &PointsView<'_>, params: &FitParams) -> Result<FittedFeature, FitError> {
    let mut lp = FitParams::new(Method::LeastSquares);
    lp.thresholds = params.thresholds;
    lp.nlls = params.nlls;
    let r = if prim == Primitive::Circle2 { Circle2Ls.fit(view, &lp, None)? } else { SphereLs.fit(view, &lp, None)? };
    Ok(r.feature)
}

/// Rule 4: the least-squares feature in scaled units.
fn scaled_feature(feature: &FittedFeature, c: &Centred) -> Result<FittedFeature, FitError> {
    let map = c.map();
    match feature {
        FittedFeature::Circle2 { c: cc, r } => {
            let q = map.forward([cc[0], cc[1], 0.0])?;
            Ok(FittedFeature::Circle2 { c: [q[0], q[1]], r: map.forward_length(*r)? })
        }
        FittedFeature::Sphere { c: cc, r } => {
            Ok(FittedFeature::Sphere { c: map.forward(*cc)?, r: map.forward_length(*r)? })
        }
        _ => Err(FitError::InvalidParams { field: "start" }),
    }
}

/// The distance of every point of the cloud from the centre `c` (scaled).
fn dists_all(q: &[f64], dim: usize, c: V3) -> Result<Vec<f64>, FitError> {
    let mut out = alloc(q.len() / 3)?;
    for p in q.chunks_exact(3) {
        let mut w = [0.0; 3];
        for k in 0..dim {
            w[k] = p[k] - c[k];
        }
        out.push(red::norm2(&w[..dim])?);
    }
    Ok(out)
}

fn min_dist_all(q: &[f64], dim: usize, c: V3) -> Result<f64, FitError> {
    let mut lo = f64::INFINITY;
    for d in dists_all(q, dim, c)? {
        lo = f::total_min(lo, d)?;
    }
    Ok(lo)
}

/// Rule 8: every point for a cloud of at most [`WORKING_ALL`] points; otherwise the evenly
/// strided subsample of [`WORKING_STRIDED`] points and, for every start centre, its
/// [`WORKING_SEED`] nearest points (ties by index), ascending.
fn initial_working_set(q: &[f64], dim: usize, centres: &[Vec<f64>]) -> Result<Vec<u32>, FitError> {
    let n = q.len() / 3;
    if n <= WORKING_ALL {
        let mut all = alloc(n)?;
        all.extend(0..n as u32);
        return Ok(all);
    }
    let stride = n.div_ceil(WORKING_STRIDED);
    let mut set: Vec<u32> = alloc(WORKING_STRIDED + WORKING_SEED * centres.len())?;
    set.extend((0..n).step_by(stride).map(|i| i as u32));
    let mut order: Vec<(f64, u32)> = alloc(n)?;
    for st in centres {
        let mut cs = [0.0; 3];
        cs[..dim].copy_from_slice(st.get(..dim).ok_or(FitError::InvalidParams { field: "start_spec" })?);
        order.clear();
        for (i, d) in dists_all(q, dim, cs)?.into_iter().enumerate() {
            order.push((d, i as u32));
        }
        order.sort_unstable_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        set.extend(order.iter().take(WORKING_SEED).map(|&(_, i)| i));
    }
    set.sort_unstable();
    set.dedup();
    Ok(set)
}

/// Rule 8: the points outside the working set `idx` that lie within `(1 + WORKING_MARGIN) r`
/// of a converged start's centre `c`, ascending, without repeats.
fn violators(
    q: &[f64],
    dim: usize,
    idx: &[u32],
    per_start: &[StartSummary<SolverError>],
) -> Result<Vec<u32>, FitError> {
    let mut out: Vec<u32> = Vec::new();
    for s in per_start {
        let StartSummary::Converged { params, .. } = s else { continue };
        let (Some(cv), Some(&r)) = (params.get(..dim), params.get(dim)) else {
            return Err(FitError::InvalidParams { field: "start" });
        };
        let mut cs = [0.0; 3];
        cs[..dim].copy_from_slice(cv);
        let lim = r * (1.0 + WORKING_MARGIN);
        for (i, d) in dists_all(q, dim, cs)?.into_iter().enumerate() {
            if d <= lim && idx.binary_search(&(i as u32)).is_err() {
                out.try_reserve(1)
                    .map_err(|_| FitError::Num(NumError::Domain { op: OP, detail: "allocation_failed" }))?;
                out.push(i as u32);
            }
        }
    }
    out.sort_unstable();
    out.dedup();
    Ok(out)
}

fn merge(a: &[u32], b: &[u32]) -> Result<Vec<u32>, FitError> {
    let mut m = alloc(a.len() + b.len())?;
    m.extend_from_slice(a);
    m.extend_from_slice(b);
    m.sort_unstable();
    m.dedup();
    Ok(m)
}

fn run(
    prim: Primitive,
    view: &PointsView<'_>,
    params: &FitParams,
    mic: &MicParams,
    seed: Option<u64>,
) -> Result<MicFit, FitError> {
    let dim = if prim == Primitive::Circle2 { 2 } else { 3 };
    let ls = ls_feature(prim, view, params)?;
    let tr = TrustRegion::from_feature(&ls, mic.tr_fraction)?;
    if mic.boundary_tol < tr.boundary_tol_floor()? {
        return Err(FitError::InvalidParams { field: "boundary_tol" });
    }
    let (c, _) = check_points(prim, view, &params.thresholds)?;
    let map = *c.map();
    let tr_s = TrustRegion::from_feature(&scaled_feature(&ls, &c)?, mic.tr_fraction)?;
    let rows = tr_s.rows()?;
    let mut data_scale = 0.0_f64;
    for &x in c.xyz().iter().chain(&rows.b) {
        data_scale = f::total_max(data_scale, x.abs())?;
    }
    let centre_s = tr_s.centre();
    let n = c.len();
    let mut primary = alloc(dim + 1)?;
    primary.extend_from_slice(&centre_s[..dim]);
    primary.push(0.0);
    let mut half_widths = vec![tr_s.inradius_m() / 2.0; dim];
    half_widths.push(0.0);
    let mut spec = StartSpec {
        primary,
        half_widths,
        count: mic.starts,
        seed: seed.unwrap_or(DEFAULT_SEED),
        stream: STREAM,
        tolerances: vec![0.0; dim + 1],
    };
    let centres = mstart::generate_starts(&spec).map_err(|_| FitError::InvalidParams { field: "start_spec" })?;
    let mut idx = initial_working_set(c.xyz(), dim, &centres)?;
    let mut prob = Mic {
        q: c.xyz(),
        idx: Vec::new(),
        dim,
        a: rows.a,
        b: rows.b,
        len_scale: tr_s.radius_m(),
        data_scale,
        start: Vec::new(),
    };
    let r0 = min_dist_all(c.xyz(), dim, centre_s)?;
    spec.primary[dim] = r0;
    spec.tolerances = vec![AGREE_TOL * r0; dim + 1];
    let cfg = mic.seq;
    let mut round = 0u32;
    // Rule 8: solve on the working set until no converged start leaves a point of the cloud
    // inside its circle (or sphere) widened by `WORKING_MARGIN`.
    let out = loop {
        round += 1;
        prob.idx = idx.clone();
        prob.start = spec.primary.clone();
        // Rule 7: the seeded starts, each with a feasible radius (the nearest point distance).
        let mut starts = alloc(centres.len())?;
        for st in &centres {
            let mut cs = [0.0; 3];
            cs[..dim].copy_from_slice(&st[..dim]);
            let mut s0 = st.clone();
            s0[dim] = prob.min_dist(cs).map_err(cert_err)?;
            starts.push(s0);
        }
        let out = match mstart::run_listed(&starts, &spec.tolerances, |start| {
            let o = seq::solve(&prob, start, &cfg)?;
            let value = o.certified.value();
            let params = o.certified.params().to_vec();
            Ok((o, value, params))
        }) {
            Ok(o) => o,
            Err(MultiStartError::AllFailed(errs)) => {
                let cause = errs.first().map_or("no_starts", seq_cause);
                return Err(FitError::SeqFailed { cause, starts: mic.starts });
            }
            Err(MultiStartError::InvalidSpec(_)) => return Err(FitError::InvalidParams { field: "start_spec" }),
        };
        if idx.len() == n {
            break out;
        }
        let added = violators(c.xyz(), dim, &idx, &out.per_start)?;
        if added.is_empty() {
            break out;
        }
        if round >= MAX_ROUNDS || idx.len() + added.len() > WORKING_CAP {
            return Err(FitError::Num(NumError::Domain { op: OP, detail: "working_set_limit" }));
        }
        idx = merge(&idx, &added)?;
    };
    let certified = &out.best.certified;
    let p = certified.params().to_vec();
    let (cs, _) = prob.split(&p).map_err(cert_err)?;
    let centre_si = map.inverse(cs)?;
    let centre_si = if dim == 2 { [centre_si[0], centre_si[1], 0.0] } else { centre_si };

    // Rule 9: a facet in the active set, or a centre on the region's boundary.
    let mut contacts: Vec<u32> = alloc(certified.certificate().active_set().len())?;
    let mut on_facet = false;
    for l in certified.certificate().active_set() {
        match prob.row(*l).map_err(cert_err)? {
            Row::Point(i) => contacts.push(prob.index(i).map_err(cert_err)?),
            Row::Trust(_) => on_facet = true,
        }
    }
    if on_facet || contacts.is_empty() {
        return Err(FitError::UnboundedEnvelope { boundary_distance_m: tr.boundary_distance(centre_si)? });
    }
    let boundary_distance_m = tr.check_boundary(centre_si, mic.boundary_tol)?;
    contacts.sort_unstable();

    // Rule 10: the radius from the contacts and the certified centre; every point checked.
    let dists = dists_all(c.xyz(), dim, cs)?;
    let mut r = f64::INFINITY;
    for &k in &contacts {
        r = f::total_min(r, *dists.get(k as usize).ok_or(FitError::InvalidParams { field: "contacts" })?)?;
    }
    let mut res = alloc(dists.len())?;
    let mut worst = (0usize, 0.0_f64);
    for (i, &d) in dists.iter().enumerate() {
        let e = map.inverse_length(d - r)?;
        if f::total_cmp(-e, worst.1)? == Ordering::Greater {
            worst = (i, -e);
        }
        res.push(e);
    }
    if f::total_cmp(worst.1, INSIDE_TOL)? == Ordering::Greater {
        return Err(FitError::SideViolation { index: worst.0, violation: worst.1 });
    }
    let feature_s =
        if dim == 2 { FittedFeature::Circle2 { c: [cs[0], cs[1]], r } } else { FittedFeature::Sphere { c: cs, r } };
    let feature = map.inverse_feature(&feature_s)?.canonical()?;

    // Rule 11: the global cross-check.
    let tol = CROSS_TOL * r + certified.certificate().value_bound();
    let region = global::Region { q: c.xyz(), dim, a: &prob.a, b: &prob.b, centre: centre_s, radius: tr_s.radius_m() };
    let s = global::search(&region, r, tol)?;
    let better = match s.better {
        Some((m, fb)) => {
            let ms = map.inverse(m)?;
            Some((if dim == 2 { [ms[0], ms[1], 0.0] } else { ms }, map.inverse_length(fb)?))
        }
        None => None,
    };
    let cross_check = CrossCheck { outcome: s.outcome, cells: s.cells, work: s.work, depth: s.depth, better };

    // Rule 12.
    let passed = s.outcome == CrossOutcome::Passed;
    let local_optimum_only = !passed;
    let mut certificate = out.best.certified.clone().into_certificate();
    if local_optimum_only && !certificate.flags().local_optimum_only {
        certificate
            .set_local_optimum_only()
            .map_err(|_| FitError::Num(NumError::Domain { op: OP, detail: "cert_flag" }))?;
    }
    Ok(MicFit {
        algorithm: algorithm(prim),
        feature,
        residuals: res,
        contacts,
        max_violation: worst.1,
        certificate,
        optimality: if passed { Optimality::Global } else { Optimality::StrictLocal },
        agreement: out.agreement,
        local_optimum_only,
        cross_check,
        trust_region: tr,
        boundary_distance_m,
        start_radius: map.inverse_length(r0)?,
        starts: out.starts,
        per_start: out.per_start,
        best_index: out.best_index,
        origin: out.origin,
    })
}

/// Rule 13: the contract summary of a fit, refusing one whose global optimality is not proven
/// ([`FitError::FlaggedFit`]). `scale` is the centring scale of the fitted points
/// (`Normalisation::scale`), for the certificate's value bound in metres. Diagnostics: rank
/// the parameter count, `cond` the certificate's condition estimate, `grad_norm` 0 and
/// `iterations` 0 (iterates are not values of record), stop `exchange`; the contract
/// certificate has the contacts as support and the value bound as gap.
///
/// # Errors
///
/// [`FitError::FlaggedFit`] for a flagged fit, and the contract's assembly errors.
pub fn to_fit_result(m: MicFit, scale: f64) -> Result<FitResult, FitError> {
    if m.local_optimum_only || m.optimality != Optimality::Global {
        return Err(FitError::FlaggedFit { local_optimum_only: true, suspect_axis: false, rms_ratio: 0.0 });
    }
    let diagnostics = FitDiagnostics {
        grad_norm: 0.0,
        rank: m.certificate.params().len(),
        cond: m.certificate.cond(),
        iterations: 0,
        stop: StopReason::Exchange,
    };
    let mut support = alloc(m.contacts.len())?;
    support.extend(m.contacts.iter().map(|&i| i as usize));
    let cert = contract::Certificate { support, gap: m.certificate.value_bound() * scale };
    FitResult::assemble(&m.feature, m.residuals, diagnostics, Some(cert), m.algorithm)
}

/// The centring scale of the points a fit of `p` runs on.
fn fit_scale(p: Primitive, pts: &PointsView<'_>, params: &FitParams) -> Result<f64, FitError> {
    if p == Primitive::Sphere {
        return Ok(Centred::new(pts)?.map().scale());
    }
    let (p0, e1, e2) = projection_frame(params)?;
    let mut buf = alloc(pts.xyz().len())?;
    for x in pts.xyz().chunks_exact(3) {
        let dx = [x[0] - p0[0], x[1] - p0[1], x[2] - p0[2]];
        buf.push(flush(red::dot(&dx, &e1)?));
        buf.push(flush(red::dot(&dx, &e2)?));
        buf.push(0.0);
    }
    Ok(Centred::new(&PointsView::new(&buf, None)?)?.map().scale())
}

impl Fit for MicCircle2Fit {
    fn algorithm_id(&self) -> AlgorithmId {
        algorithm(Primitive::Circle2)
    }

    /// Rule 13: fails closed on a flagged fit ([`to_fit_result`]).
    fn fit(&self, pts: &PointsView<'_>, params: &FitParams, seed: Option<u64>) -> Result<FitResult, FitError> {
        let m = fit_mic_circle2d(pts, params, &self.mic, seed)?;
        to_fit_result(m, fit_scale(Primitive::Circle2, pts, params)?)
    }
}

impl Fit for MicSphereFit {
    fn algorithm_id(&self) -> AlgorithmId {
        algorithm(Primitive::Sphere)
    }

    /// Rule 13: fails closed on a flagged fit ([`to_fit_result`]).
    fn fit(&self, pts: &PointsView<'_>, params: &FitParams, seed: Option<u64>) -> Result<FitResult, FitError> {
        let m = fit_mic_sphere(pts, params, &self.mic, seed)?;
        to_fit_result(m, fit_scale(Primitive::Sphere, pts, params)?)
    }
}

/// The circle's trust region as a counter-clockwise polygon (metres): vertex `k` is the
/// intersection of facet lines `k` and `k + 1` (mod 32) of `TrustRegion::rows`, by Cramer's
/// rule (each value one product difference over the facet pair's determinant). For the
/// agreement job's Voronoi path (`xchk::mic_circle2d_vor`), which then solves on the same
/// polygon. `InvalidParams{field: "primitive"}` for a region that is not a circle's.
pub fn trust_polygon(tr: &TrustRegion) -> Result<Vec<V2>, FitError> {
    if tr.primitive() != Primitive::Circle2 {
        return Err(FitError::InvalidParams { field: "primitive" });
    }
    let rows = tr.rows()?;
    let m = DIRS2.len();
    let mut out = alloc(m)?;
    for k in 0..m {
        let (Some(a), Some(c), Some(&b0), Some(&b1)) =
            (rows.a.get(k), rows.a.get((k + 1) % m), rows.b.get(k), rows.b.get((k + 1) % m))
        else {
            return Err(FitError::InvalidParams { field: "region" });
        };
        let det = a[0] * c[1] - a[1] * c[0];
        let x = (b0 * c[1] - b1 * a[1]) / det;
        let y = (a[0] * b1 - c[0] * b0) / det;
        if !(x.is_finite() && y.is_finite()) {
            return Err(FitError::Num(NumError::Infinite { op: OP, index: Some(k) }));
        }
        out.push([x, y]);
    }
    Ok(out)
}

/// The agreement test of REQ-KRN.ENV.MIC.CIRSPH-004 between the conic circle `conic` and the
/// Voronoi circle `vor` on the same polygon ([`trust_polygon`]): `Ok` when both report a
/// boundary optimum (`UnboundedEnvelope` and `on_region_boundary`), or neither does and the
/// centres (largest coordinate difference) and radii differ by at most `tol` metres.
pub fn check_agreement(conic: &Result<MicFit, FitError>, vor: &Inscribed, tol: f64) -> Result<(), Disagreement> {
    if !(tol.is_finite() && tol > 0.0) {
        return Err(Disagreement::InvalidTolerance);
    }
    let fit = match conic {
        Err(FitError::UnboundedEnvelope { .. }) => {
            return if vor.on_region_boundary {
                Ok(())
            } else {
                Err(Disagreement::Boundary { conic_on_boundary: true, xchk_on_boundary: false })
            };
        }
        Err(e) => return Err(Disagreement::ConicFailed { code: e.code() }),
        Ok(m) => m,
    };
    if vor.on_region_boundary {
        return Err(Disagreement::Boundary { conic_on_boundary: false, xchk_on_boundary: true });
    }
    let (cc, r) = match fit.feature {
        FittedFeature::Circle2 { c, r } => (c, r),
        _ => return Err(Disagreement::ConicFailed { code: "invalid_params" }),
    };
    let dc = f::total_max((cc[0] - vor.centre[0]).abs(), (cc[1] - vor.centre[1]).abs())
        .map_err(|_| Disagreement::Centre { delta: f64::INFINITY })?;
    if dc.is_nan() || dc > tol {
        return Err(Disagreement::Centre { delta: dc });
    }
    let dr = (r - vor.radius).abs();
    if dr.is_nan() || dr > tol {
        return Err(Disagreement::Radius { delta: dr });
    }
    Ok(())
}
