//! Sequential linearisation driver (spec node KRN.SOLV.SEQ).
//!
//! Minimum-zone, maximum-inscribed and tangential problems for curved features are non-convex
//! in the axis or centre. [`solve`] linearises such a problem about the current point, solves
//! the local LP (or SOCP) with a step bound, accepts or rejects the step, and repeats until the
//! active set is stable. On success it hands the final subproblem solution to the certificate
//! pipeline ([`crate::cert::certify`], KRN.SOLV.CERT.CT) and returns **only** the certified
//! result: the iterate never leaves this module as a value. [`solve_multi`] runs [`solve`] from
//! the seeded starts of [`crate::mstart`] and flags the certificate `local_optimum_only` when the
//! certified results of the starts disagree.
//!
//! # Procedure (pinned)
//!
//! 1. **Checks**, in this order (each failure is [`SeqCause::InvalidInput`] naming the field,
//!    except a callback's own error, which is [`SeqCause::Callback`]): the config
//!    ([`SeqConfig::check`]); `n_params() >= 1`; `scales()` of length `n`, each finite and
//!    positive; the start of length `n` and finite; the outer region, if any, of length `n`
//!    with `lo <= hi` (either end may be infinite, neither NaN) and the start inside it;
//!    `data_scale()` finite and positive; the row labels; `objective(start)` finite.
//! 2. **Iteration** `k = 1, 2, ..`: if `radius < radius_min` the result is
//!    [`SeqCause::StepBoundCollapsed`]. Otherwise `local_problem(p)` gives the local problem in
//!    the step `dp` (`n` variables, no quadratic term, row `i` the constraint labelled
//!    `row_labels()[i]`) and the model objective at `dp = 0`, `m0`; the model is
//!    `m(dp) = m0 + q'dp`.
//! 3. **Step bound** (REQ-006): with `t_j = radius * scale_j`, the box
//!    `max(-t_j, lo_j - p_j) <= dp_j <= min(t_j, hi_j - p_j)` (the outer region, when given,
//!    intersected in) is appended as the last `2 n` rows, one nonnegative cone: row `m + 2 j`
//!    is `dp_j <= upper_j`, row `m + 2 j + 1` is `-dp_j <= -lower_j`. An LP stays an LP.
//! 4. **Solve** with `conic::solve_lp` (`conic::solve_socp` when the local problem has a
//!    second-order cone). A primal infeasibility status (also the reduced-accuracy one) is
//!    [`SeqCause::SubproblemInfeasible`]; every other conic failure is [`SeqCause::Conic`].
//! 5. **Classify**: the step-bound rows are stripped and `cert::active::extract` runs on the
//!    local problem with `cfg.thresholds`; its **strongly active** rows (sorted) are this
//!    iteration's active set (also when the extraction reports `ActiveSetUndetermined`, which
//!    carries them).
//! 6. **Step test.** `predicted = f(p) - m(dp)` with `f` = [`LinearisableProblem::objective`];
//!    the step bound is *active* when some `|dp_j| >= BOUND_ACTIVE_FRAC * t_j`. A step whose
//!    bound is inactive and whose `predicted <= decrease_tol_rel * data_scale` is a **null
//!    step**: the model offers no decrease beyond the conic solver's accuracy, so the point
//!    stays, the radius stays and the step taken is 0. Otherwise `p+ = p + dp` (clamped into
//!    the outer region), `rho = (f(p) - f(p+)) / predicted` (undefined when `predicted <= 0`),
//!    the step is accepted iff `rho > 0.1`, and the radius is updated by REQ-007:
//!    `rho < 0.25` (or undefined) gives `radius / 4`; `rho > 0.75` with the bound active gives
//!    `min(2 radius, radius_max)`; otherwise it is unchanged.
//! 7. **Stop rule** (REQ-002): `stable` counts the consecutive iterations whose active set
//!    equals the previous iteration's. The driver stops with success when
//!    `stable >= stable_iters` and the step norm `max_j |dp_j| / scale_j` (0 for a null step)
//!    is at most `step_tol_rel * (1 + max_j |p_j| / scale_j)`. After `max_iter` iterations
//!    without success the result is [`SeqCause::IterationLimit`].
//! 8. **Certify** (REQ-005): `cert::certify` on the local problem of the last iteration and its
//!    solution with the step-bound rows removed (REQ-006), with `cfg.kind` and `cfg.cert`. Its
//!    error is [`SeqCause::Cert`] and no value (fail closed: a `certificate_failed` is never
//!    replaced by the iterate). `on_trust_boundary` (REQ-004) is set when an outer region is
//!    given and the final point lies within `boundary_tol_rel * scale_j` of one of its finite
//!    ends; it is reported on the outcome and on a certification error.
//!
//! Every error carries the last active set, the radius, the iteration count and the
//! `on_trust_boundary` flag, and no value (REQ-003).
//!
//! # What reaches a digest
//!
//! Only the [`Certified`] result, whose certificate is built from the recomputed active-set
//! optimum (REQ-KRN.SOLV-006). [`SeqOutcome::iterations`], [`SeqOutcome::on_trust_boundary`]
//! and [`SeqOutcome::history`] come from the iterates; they are diagnostics, never values of
//! record, and callers must not journal them as results. [`solve_multi`] compares the starts by
//! their **certified** values and parameters, so `local_optimum_only` does not depend on an
//! iterate either.
//!
//! # Determinism
//!
//! D0 (REQ-010): single-threaded, no FMA, no `f64::min`/`max` (total-order helpers of
//! `fid_num::f`), no hash containers, no I/O; the dot product of the model goes through
//! `fid_num::red`. The conic solver is the KNP-1 patched `fid-clarabel`, whose results are
//! bit-identical on x86-64, ARM64 and wasm32, so the iterate path and the final active set are
//! too. Reference: Conn, Gould and Toint, Trust-Region Methods, SIAM 2000 [unverified chapter].

use core::fmt;

use fid_num::f::{total_max, total_min};
use fid_num::{red, NumError};

use crate::cert::active::{extract, Thresholds};
use crate::cert::ct::DEFAULT_THRESHOLD_REL;
use crate::cert::{certify, ActiveSetProblem, CertConfig, CertError, CertKind, Certified, RowLabel};
use crate::conic::{
    solve_lp, solve_socp, Cone, ConicCause, ConicError, ConicProblem, ConicProblemBuilder, ConicSolution, Csc,
};
use crate::mstart::{self, MultiStartError, MultiStartOutcome, StartSpec};

/// Default iteration limit.
pub const DEFAULT_MAX_ITER: u32 = 100;
/// Default number of consecutive iterations with an unchanged active set (REQ-008,
/// [unverified]).
pub const DEFAULT_STABLE_ITERS: u32 = 3;
/// Default relative step tolerance (REQ-008, [unverified]).
pub const DEFAULT_STEP_TOL_REL: f64 = 1e-12;
/// Default relative decrease tolerance of the null-step rule, relative to the problem's data
/// scale ([unverified]). Above the conic solver's gap and feasibility tolerances (`1e-10`), so
/// a step that only moves within the solver's own accuracy is a null step.
pub const DEFAULT_DECREASE_TOL_REL: f64 = 1e-9;
/// Default relative distance (in units of `scale_j`) within which the final point counts as on
/// the outer region's boundary ([unverified]).
pub const DEFAULT_BOUNDARY_TOL_REL: f64 = 1e-6;
/// Default initial radius, in units of the scales.
pub const DEFAULT_RADIUS_0: f64 = 1.0;
/// Default smallest radius, in units of the scales (`2^-40`).
pub const DEFAULT_RADIUS_MIN: f64 = 1.0 / 1_099_511_627_776.0;
/// Default largest radius, in units of the scales.
pub const DEFAULT_RADIUS_MAX: f64 = 1024.0;

/// A step is accepted only when `rho` exceeds this (REQ-007, [unverified]).
pub const RHO_ACCEPT: f64 = 0.1;
/// Below this `rho` the radius shrinks (REQ-007, [unverified]).
pub const RHO_SHRINK: f64 = 0.25;
/// Above this `rho`, with the bound active, the radius grows (REQ-007, [unverified]).
pub const RHO_EXPAND: f64 = 0.75;
/// Shrink divisor (REQ-007).
pub const SHRINK_DIVISOR: f64 = 4.0;
/// Growth factor (REQ-007).
pub const EXPAND_FACTOR: f64 = 2.0;
/// The step bound counts as active when some `|dp_j|` reaches this fraction of its bound (the
/// interior-point solution stays strictly inside the box).
pub const BOUND_ACTIVE_FRAC: f64 = 0.999;

/// A non-convex problem the driver can linearise: the [`ActiveSetProblem`] that certifies the
/// result plus the local model. Implemented by `KRN.MZ`, `KRN.ENV`, `GDT.ZONE.POSE.LIN` and
/// test problems.
pub trait LinearisableProblem: ActiveSetProblem {
    /// Per-parameter scales (finite, positive) of the step bound `|dp_j| <= radius * scale_j`
    /// and of the step norm.
    fn scales(&self) -> Vec<f64>;

    /// The local problem about `p`, in the step `dp` (`n_params` variables): minimise `q'dp`
    /// subject to the linearised constraints, with **row `i` the constraint labelled
    /// `row_labels()[i]`** (the certificate pipeline reads the rows in that order), every row of
    /// the nonnegative cone for certification, and no quadratic term. The `f64` is the model
    /// objective at zero step, `m0`, so that `m0 + q'dp` models the objective at `p + dp`.
    fn local_problem(&self, p: &[f64]) -> Result<(ConicProblem, f64), CertError>;

    /// The objective the driver minimises, evaluated at any point (for a zone problem, the zone
    /// width at the given axis or centre, with the radius parameters eliminated, so that it is
    /// defined at points where the radius parameters are slightly infeasible). At a feasible
    /// point it equals the model at zero step.
    fn objective(&self, p: &[f64]) -> Result<f64, CertError>;

    /// An optional outer trust region: per-parameter absolute bounds `(lo, hi)` (either end may
    /// be infinite) that every iterate must satisfy, for example the MIC centre region of
    /// `KRN.ENV`. `None` (the default) for none.
    fn outer_region(&self) -> Option<Vec<(f64, f64)>> {
        None
    }
}

/// Constants of the driver. All of them are the caller's (REQ-008); [`SeqConfig::new`] gives
/// the documented defaults.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SeqConfig {
    /// Iteration limit (at least 1).
    pub max_iter: u32,
    /// Consecutive iterations with an unchanged active set needed to stop (at least 1).
    pub stable_iters: u32,
    /// Relative step tolerance of the stop rule (finite, not negative).
    pub step_tol_rel: f64,
    /// Relative decrease tolerance of the null-step rule, times the data scale (finite, not
    /// negative).
    pub decrease_tol_rel: f64,
    /// Relative boundary tolerance of `on_trust_boundary`, times `scale_j` (finite, not
    /// negative).
    pub boundary_tol_rel: f64,
    /// Initial radius (finite, positive).
    pub radius_0: f64,
    /// Smallest radius; below it the run ends with `StepBoundCollapsed` (finite, not negative).
    pub radius_min: f64,
    /// Largest radius (finite, at least `radius_0`).
    pub radius_max: f64,
    /// Relative thresholds of the per-iteration active-set classification.
    pub thresholds: Thresholds,
    /// What the certified optimum is.
    pub kind: CertKind,
    /// Constants of the certificate pipeline.
    pub cert: CertConfig,
}

impl SeqConfig {
    /// The defaults (REQ-008: 100 iterations, 3 stable iterations, `1e-12` relative step
    /// tolerance) for an optimum of `kind`.
    pub fn new(kind: CertKind) -> SeqConfig {
        SeqConfig {
            max_iter: DEFAULT_MAX_ITER,
            stable_iters: DEFAULT_STABLE_ITERS,
            step_tol_rel: DEFAULT_STEP_TOL_REL,
            decrease_tol_rel: DEFAULT_DECREASE_TOL_REL,
            boundary_tol_rel: DEFAULT_BOUNDARY_TOL_REL,
            radius_0: DEFAULT_RADIUS_0,
            radius_min: DEFAULT_RADIUS_MIN,
            radius_max: DEFAULT_RADIUS_MAX,
            thresholds: Thresholds { slack_rel: DEFAULT_THRESHOLD_REL, mult_rel: DEFAULT_THRESHOLD_REL },
            kind,
            cert: CertConfig::default(),
        }
    }

    /// Checks the config. `radius_min` above `radius_0` is allowed: the run then ends at once
    /// with `StepBoundCollapsed`.
    ///
    /// # Errors
    ///
    /// [`SeqCause::InvalidInput`] naming the first failing field.
    pub fn check(&self) -> Result<(), SeqCause> {
        let invalid = |field: &'static str| Err(SeqCause::InvalidInput { field });
        let nonneg = |v: f64| v.is_finite() && v >= 0.0;
        if self.max_iter == 0 {
            return invalid("max_iter");
        }
        if self.stable_iters == 0 {
            return invalid("stable_iters");
        }
        if !nonneg(self.step_tol_rel) {
            return invalid("step_tol_rel");
        }
        if !nonneg(self.decrease_tol_rel) {
            return invalid("decrease_tol_rel");
        }
        if !nonneg(self.boundary_tol_rel) {
            return invalid("boundary_tol_rel");
        }
        if !(self.radius_0.is_finite() && self.radius_0 > 0.0) {
            return invalid("radius_0");
        }
        if !nonneg(self.radius_min) {
            return invalid("radius_min");
        }
        if !(self.radius_max.is_finite() && self.radius_max >= self.radius_0) {
            return invalid("radius_max");
        }
        if !nonneg(self.thresholds.slack_rel) || !nonneg(self.thresholds.mult_rel) {
            return invalid("thresholds");
        }
        Ok(())
    }
}

/// Why a run ended without a value.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum SeqCause {
    /// An input or config value is outside its domain (`field` names it).
    InvalidInput {
        /// The failing input.
        field: &'static str,
    },
    /// A [`LinearisableProblem`] or [`ActiveSetProblem`] callback returned an error.
    Callback(CertError),
    /// A `fid-num` operation failed (a non-finite value).
    Numeric(NumError),
    /// The iteration limit was reached without the stop rule holding (REQ-003).
    IterationLimit,
    /// A step-bounded subproblem is primal infeasible (REQ-003).
    SubproblemInfeasible,
    /// The radius fell below `radius_min` (REQ-003).
    StepBoundCollapsed,
    /// A subproblem solve failed for another reason.
    Conic(ConicError),
    /// The active-set classification or the certificate pipeline failed (including
    /// `certificate_failed`); no value is returned.
    Cert(CertError),
}

impl SeqCause {
    /// A stable snake_case code per variant, for logs and output pins (`Debug` output is not a
    /// stable format).
    pub fn code(&self) -> &'static str {
        match self {
            SeqCause::InvalidInput { .. } => "invalid_input",
            SeqCause::Callback(_) => "callback",
            SeqCause::Numeric(_) => "numeric",
            SeqCause::IterationLimit => "iteration_limit",
            SeqCause::SubproblemInfeasible => "subproblem_infeasible",
            SeqCause::StepBoundCollapsed => "step_bound_collapsed",
            SeqCause::Conic(_) => "conic",
            SeqCause::Cert(_) => "cert",
        }
    }
}

/// A run that ended without a value (REQ-003): the cause and the last diagnostics.
#[derive(Debug, Clone, PartialEq)]
pub struct SolverError {
    /// Why the run ended.
    pub cause: SeqCause,
    /// The strongly active rows of the last classified subproblem, sorted (empty before the
    /// first one).
    pub active_set: Vec<RowLabel>,
    /// The radius at the end.
    pub radius: f64,
    /// Iterations completed.
    pub iterations: u32,
    /// The final point lies on the outer region's boundary (only ever set on a certification
    /// error after convergence).
    pub on_trust_boundary: bool,
}

impl fmt::Display for SolverError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "seq: {} after {} iterations (radius {:e}, {} active rows",
            self.cause.code(),
            self.iterations,
            self.radius,
            self.active_set.len()
        )?;
        if self.on_trust_boundary {
            write!(f, ", on the outer trust boundary")?;
        }
        write!(f, ")")?;
        match &self.cause {
            SeqCause::InvalidInput { field } => write!(f, ": {field}"),
            SeqCause::Callback(e) | SeqCause::Cert(e) => write!(f, ": {e}"),
            SeqCause::Numeric(e) => write!(f, ": {e}"),
            SeqCause::Conic(e) => write!(f, ": {e}"),
            SeqCause::IterationLimit | SeqCause::SubproblemInfeasible | SeqCause::StepBoundCollapsed => Ok(()),
        }
    }
}

impl core::error::Error for SolverError {}

/// Diagnostics of one iteration. Never part of a digest or a value of record.
#[derive(Debug, Clone, PartialEq)]
pub struct IterSummary {
    /// Iteration number, from 1.
    pub iteration: u32,
    /// Radius the subproblem was solved with.
    pub radius: f64,
    /// `f(p) - m(dp)`.
    pub predicted: f64,
    /// `f(p) - f(p + dp)` (`None` for a null step).
    pub actual: Option<f64>,
    /// `actual / predicted` (`None` for a null step or when `predicted <= 0`).
    pub rho: Option<f64>,
    /// `max_j |dp_j| / scale_j` of the proposed step.
    pub step_norm: f64,
    /// The step bound was active.
    pub bound_active: bool,
    /// A null step (see the module docs).
    pub null_step: bool,
    /// The step was accepted.
    pub accepted: bool,
    /// Size of the strongly active set.
    pub active_rows: usize,
    /// Consecutive iterations with an unchanged active set, after this one.
    pub stable: u32,
}

/// A successful run: the certified result and diagnostics.
#[derive(Debug, Clone, PartialEq)]
pub struct SeqOutcome {
    /// The certified result (the only value of record).
    pub certified: Certified,
    /// Iterations performed (diagnostic).
    pub iterations: u32,
    /// The final point lies on the outer region's boundary (REQ-004), so the caller can raise
    /// its own typed error.
    pub on_trust_boundary: bool,
    /// Per-iteration diagnostics; never enter a digest.
    pub history: Vec<IterSummary>,
}

/// A [`solve_multi`] run that produced no value.
#[derive(Debug, Clone, PartialEq)]
pub enum MultiError {
    /// The multi-start run failed (every start failed, or the start spec was rejected).
    Start(MultiStartError<SolverError>),
    /// Setting `local_optimum_only` on the selected certificate failed (a defect; fail closed).
    Flag(CertError),
}

impl fmt::Display for MultiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MultiError::Start(e) => write!(f, "seq: {e}"),
            MultiError::Flag(e) => write!(f, "seq: local_optimum_only could not be set: {e}"),
        }
    }
}

impl core::error::Error for MultiError {}

/// The mutable state of a run, so that every error can carry the last diagnostics.
struct State {
    active_set: Vec<RowLabel>,
    radius: f64,
    iterations: u32,
}

impl State {
    fn fail(&self, cause: SeqCause) -> SolverError {
        SolverError {
            cause,
            active_set: self.active_set.clone(),
            radius: self.radius,
            iterations: self.iterations,
            on_trust_boundary: false,
        }
    }
}

/// The last subproblem of a converged run: the local problem and its solution without the
/// step-bound rows.
struct Final {
    local: ConicProblem,
    solution: ConicSolution,
    p: Vec<f64>,
}

/// Runs the driver from `start` (see the module docs for the pinned procedure) and returns the
/// certified result only.
///
/// # Errors
///
/// [`SolverError`] with the cause, the last active set and diagnostics, and no value.
pub fn solve<P: LinearisableProblem + ?Sized>(
    problem: &P,
    start: &[f64],
    cfg: &SeqConfig,
) -> Result<SeqOutcome, SolverError> {
    let mut st = State { active_set: Vec::new(), radius: cfg.radius_0, iterations: 0 };
    let mut history = Vec::new();
    let fin = iterate(problem, start, cfg, &mut st, &mut history)?;
    let on_trust_boundary = match problem.outer_region() {
        Some(region) => on_boundary(&fin.p, &problem.scales(), &region, cfg.boundary_tol_rel),
        None => false,
    };
    match certify(problem, &fin.local, &fin.solution, cfg.kind, &cfg.cert) {
        Ok(certified) => Ok(SeqOutcome { certified, iterations: st.iterations, on_trust_boundary, history }),
        Err(e) => {
            let mut err = st.fail(SeqCause::Cert(e));
            err.on_trust_boundary = on_trust_boundary;
            Err(err)
        }
    }
}

/// Runs [`solve`] from every start of `spec` in index order through [`mstart::run`]
/// (REQ-009). Starts are compared by their **certified** value and parameters; the selected
/// start's certificate gets `local_optimum_only` when the certified results disagree within
/// `spec.tolerances`.
///
/// # Errors
///
/// [`MultiError::Start`] when the spec is rejected or every start fails (with each start's
/// [`SolverError`]), [`MultiError::Flag`] if the flag cannot be set.
pub fn solve_multi<P: LinearisableProblem + ?Sized>(
    problem: &P,
    spec: &StartSpec,
    cfg: &SeqConfig,
) -> Result<MultiStartOutcome<SeqOutcome, SolverError>, MultiError> {
    let mut out = mstart::run(spec, |start| {
        let outcome = solve(problem, start, cfg)?;
        let value = outcome.certified.value();
        let params = outcome.certified.params().to_vec();
        Ok((outcome, value, params))
    })
    .map_err(MultiError::Start)?;
    if out.local_optimum_only {
        out.best.certified.set_local_optimum_only().map_err(MultiError::Flag)?;
    }
    Ok(out)
}

fn invalid(field: &'static str) -> SeqCause {
    SeqCause::InvalidInput { field }
}

/// Step 1 of the procedure: input checks. Returns the scales, the outer region, the data
/// scale and the row labels.
#[allow(clippy::type_complexity)]
fn check_inputs<P: LinearisableProblem + ?Sized>(
    problem: &P,
    start: &[f64],
    cfg: &SeqConfig,
) -> Result<(Vec<f64>, Option<Vec<(f64, f64)>>, f64, Vec<RowLabel>), SeqCause> {
    cfg.check()?;
    let n = problem.n_params();
    if n == 0 {
        return Err(invalid("n_params"));
    }
    let scales = problem.scales();
    if scales.len() != n || !scales.iter().all(|s| s.is_finite() && *s > 0.0) {
        return Err(invalid("scales"));
    }
    if start.len() != n || !start.iter().all(|v| v.is_finite()) {
        return Err(invalid("start"));
    }
    let region = problem.outer_region();
    if let Some(r) = &region {
        if r.len() != n || !r.iter().all(|(lo, hi)| !lo.is_nan() && !hi.is_nan() && lo <= hi) {
            return Err(invalid("outer_region"));
        }
        if !r.iter().zip(start).all(|((lo, hi), x)| lo <= x && x <= hi) {
            return Err(invalid("start"));
        }
    }
    let data_scale = problem.data_scale().map_err(SeqCause::Callback)?;
    if !(data_scale.is_finite() && data_scale > 0.0) {
        return Err(invalid("data_scale"));
    }
    Ok((scales, region, data_scale, problem.row_labels()))
}

fn objective_at<P: LinearisableProblem + ?Sized>(problem: &P, p: &[f64]) -> Result<f64, SeqCause> {
    let f = problem.objective(p).map_err(SeqCause::Callback)?;
    fid_num::check_finite("seq.objective", f).map_err(SeqCause::Numeric)
}

/// Steps 1 to 7: the loop. Returns the last subproblem of a converged run.
fn iterate<P: LinearisableProblem + ?Sized>(
    problem: &P,
    start: &[f64],
    cfg: &SeqConfig,
    st: &mut State,
    history: &mut Vec<IterSummary>,
) -> Result<Final, SolverError> {
    let (scales, region, data_scale, labels) = check_inputs(problem, start, cfg).map_err(|c| st.fail(c))?;
    let n = scales.len();
    let mut p = start.to_vec();
    let mut f_p = objective_at(problem, &p).map_err(|c| st.fail(c))?;
    let dec_tol = cfg.decrease_tol_rel * data_scale;
    let mut prev_set: Option<Vec<RowLabel>> = None;
    let mut stable: u32 = 0;
    while st.iterations < cfg.max_iter {
        if st.radius < cfg.radius_min {
            return Err(st.fail(SeqCause::StepBoundCollapsed));
        }
        let (local, m0) = problem.local_problem(&p).map_err(|e| st.fail(SeqCause::Callback(e)))?;
        if local.n() != n || local.p().is_some() {
            return Err(st.fail(invalid("local_problem")));
        }
        fid_num::check_finite("seq.model0", m0).map_err(|e| st.fail(SeqCause::Numeric(e)))?;
        let radius = st.radius;
        let bounds: Vec<f64> = scales.iter().map(|s| radius * s).collect();
        let (lower, upper) = step_box(&p, &bounds, region.as_deref()).map_err(|c| st.fail(c))?;
        let full = append_box(&local, &lower, &upper).map_err(|c| st.fail(c))?;
        let solved = if full.has_soc() { solve_socp(&full) } else { solve_lp(&full) };
        let sol = solved.map_err(|e| st.fail(conic_cause(e)))?;
        st.iterations += 1;
        let stripped = strip(&local, &sol).map_err(|c| st.fail(c))?;
        let set = strongly_active(problem, &local, &stripped, &labels, &cfg.thresholds).map_err(|c| st.fail(c))?;
        st.active_set = set;

        let dp = &stripped.x;
        let qdp = red::dot(local.q(), dp).map_err(|e| st.fail(SeqCause::Numeric(e)))?;
        let predicted = f_p - (m0 + qdp);
        let step_norm = scaled_norm(dp, &scales).map_err(|e| st.fail(SeqCause::Numeric(e)))?;
        let bound_active = dp.iter().zip(&bounds).any(|(d, b)| d.abs() >= BOUND_ACTIVE_FRAC * b);
        let null_step = !bound_active && predicted <= dec_tol;
        let (mut actual, mut rho, mut accepted) = (None, None, false);
        let taken_norm = if null_step {
            0.0
        } else {
            let p_new = advance(&p, dp, region.as_deref()).map_err(|c| st.fail(c))?;
            let f_new = objective_at(problem, &p_new).map_err(|c| st.fail(c))?;
            let act = f_p - f_new;
            actual = Some(act);
            rho = if predicted > 0.0 { Some(act / predicted) } else { None };
            accepted = rho.is_some_and(|r| r > RHO_ACCEPT);
            st.radius = match rho {
                Some(r) if r >= RHO_SHRINK => {
                    if r > RHO_EXPAND && bound_active {
                        total_min(EXPAND_FACTOR * st.radius, cfg.radius_max)
                            .map_err(|e| st.fail(SeqCause::Numeric(e)))?
                    } else {
                        st.radius
                    }
                }
                _ => st.radius / SHRINK_DIVISOR,
            };
            if accepted {
                p = p_new;
                f_p = f_new;
            }
            step_norm
        };
        stable = if prev_set.as_ref() == Some(&st.active_set) { stable.saturating_add(1) } else { 0 };
        prev_set = Some(st.active_set.clone());
        history.push(IterSummary {
            iteration: st.iterations,
            radius,
            predicted,
            actual,
            rho,
            step_norm,
            bound_active,
            null_step,
            accepted,
            active_rows: st.active_set.len(),
            stable,
        });
        let p_norm = scaled_norm(&p, &scales).map_err(|e| st.fail(SeqCause::Numeric(e)))?;
        let step_tol = cfg.step_tol_rel * (1.0 + p_norm);
        if stable >= cfg.stable_iters && taken_norm <= step_tol {
            return Ok(Final { local, solution: stripped, p });
        }
    }
    Err(st.fail(SeqCause::IterationLimit))
}

fn conic_cause(e: ConicError) -> SeqCause {
    match e {
        ConicError::Solver(s)
            if matches!(s.cause, ConicCause::PrimalInfeasible | ConicCause::AlmostPrimalInfeasible) =>
        {
            SeqCause::SubproblemInfeasible
        }
        e => SeqCause::Conic(e),
    }
}

/// `max_j |x_j| / scale_j` (in total order; every value finite).
fn scaled_norm(x: &[f64], scales: &[f64]) -> Result<f64, NumError> {
    x.iter().zip(scales).try_fold(0.0_f64, |acc, (v, s)| total_max(acc, v.abs() / s))
}

/// The step box of step 3: `[max(-t_j, lo_j - p_j), min(t_j, hi_j - p_j)]`.
fn step_box(p: &[f64], bounds: &[f64], region: Option<&[(f64, f64)]>) -> Result<(Vec<f64>, Vec<f64>), SeqCause> {
    let mut lower = Vec::with_capacity(p.len());
    let mut upper = Vec::with_capacity(p.len());
    for (j, (&x, &t)) in p.iter().zip(bounds).enumerate() {
        if !t.is_finite() {
            return Err(invalid("radius"));
        }
        let (mut lo, mut hi) = (-t, t);
        if let Some(&(rlo, rhi)) = region.and_then(|r| r.get(j)) {
            lo = total_max(lo, rlo - x).map_err(SeqCause::Numeric)?;
            hi = total_min(hi, rhi - x).map_err(SeqCause::Numeric)?;
        }
        if lo > hi {
            return Err(SeqCause::SubproblemInfeasible);
        }
        lower.push(lo);
        upper.push(hi);
    }
    Ok((lower, upper))
}

/// `p + dp`, clamped into the outer region, every value finite.
fn advance(p: &[f64], dp: &[f64], region: Option<&[(f64, f64)]>) -> Result<Vec<f64>, SeqCause> {
    let mut out = Vec::with_capacity(p.len());
    for (j, (&x, &d)) in p.iter().zip(dp).enumerate() {
        let mut y = x + d;
        if let Some(&(lo, hi)) = region.and_then(|r| r.get(j)) {
            if y < lo {
                y = lo;
            }
            if y > hi {
                y = hi;
            }
        }
        out.push(fid_num::check_finite("seq.point", y).map_err(SeqCause::Numeric)?);
    }
    Ok(out)
}

/// The local problem with the step-bound rows appended (step 3): rows `m + 2 j` (`dp_j <=
/// upper_j`) and `m + 2 j + 1` (`-dp_j <= -lower_j`), one nonnegative cone of `2 n` rows.
fn append_box(local: &ConicProblem, lower: &[f64], upper: &[f64]) -> Result<ConicProblem, SeqCause> {
    let a = local.a();
    let (m, n) = (a.nrows, a.ncols);
    let mut colptr = Vec::with_capacity(n + 1);
    let mut rowval = Vec::with_capacity(a.nnz() + 2 * n);
    let mut nzval = Vec::with_capacity(a.nnz() + 2 * n);
    colptr.push(0);
    for j in 0..n {
        let (Some(&lo), Some(&hi)) = (a.colptr.get(j), a.colptr.get(j + 1)) else {
            return Err(invalid("local_problem"));
        };
        let (Some(rows), Some(vals)) = (a.rowval.get(lo..hi), a.nzval.get(lo..hi)) else {
            return Err(invalid("local_problem"));
        };
        rowval.extend_from_slice(rows);
        nzval.extend_from_slice(vals);
        rowval.extend([m + 2 * j, m + 2 * j + 1]);
        nzval.extend([1.0, -1.0]);
        colptr.push(rowval.len());
    }
    let mut b = local.b().to_vec();
    for (lo, hi) in lower.iter().zip(upper) {
        b.extend([*hi, -*lo]);
    }
    let mut cones = local.cones().to_vec();
    cones.push(Cone::Nonneg(2 * n));
    ConicProblemBuilder::new(local.q().to_vec(), Csc::new(m + 2 * n, n, colptr, rowval, nzval), b, cones)
        .build()
        .map_err(SeqCause::Conic)
}

/// The solution without the step-bound rows (REQ-006).
fn strip(local: &ConicProblem, sol: &ConicSolution) -> Result<ConicSolution, SeqCause> {
    let m = local.m();
    let (Some(y), Some(s)) = (sol.y.get(..m), sol.s.get(..m)) else {
        return Err(invalid("solution"));
    };
    Ok(ConicSolution {
        x: sol.x.clone(),
        y: y.to_vec(),
        s: s.to_vec(),
        objective: sol.objective,
        iterations: sol.iterations,
        residuals: sol.residuals,
    })
}

/// The strongly active rows of a stripped subproblem solution (step 5).
fn strongly_active<P: ActiveSetProblem + ?Sized>(
    problem: &P,
    local: &ConicProblem,
    solution: &ConicSolution,
    labels: &[RowLabel],
    thresholds: &Thresholds,
) -> Result<Vec<RowLabel>, SeqCause> {
    match extract(local, solution, labels, problem.expected_active(), thresholds) {
        Ok(proposal) => Ok(proposal.strongly_active().to_vec()),
        Err(CertError::ActiveSetUndetermined { strongly_active, .. }) => Ok(strongly_active),
        Err(e) => Err(SeqCause::Cert(e)),
    }
}

/// Whether `p` lies within `tol * scale_j` of a finite end of the outer region.
fn on_boundary(p: &[f64], scales: &[f64], region: &[(f64, f64)], tol: f64) -> bool {
    p.iter().zip(scales).zip(region).any(|((&x, &s), &(lo, hi))| {
        let near = tol * s;
        (lo.is_finite() && x - lo <= near) || (hi.is_finite() && hi - x <= near)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn step_box_intersects_the_outer_region() {
        let (lo, hi) = step_box(&[0.0, 1.0], &[0.5, 0.5], Some(&[(-0.25, 10.0), (0.0, 1.25)])).unwrap_or_default();
        assert_eq!(lo, vec![-0.25, -0.5]);
        assert_eq!(hi, vec![0.5, 0.25]);
        assert_eq!(step_box(&[0.0], &[f64::INFINITY], None), Err(invalid("radius")));
    }

    #[test]
    fn boundary_test_uses_finite_ends_only() {
        let inf = f64::INFINITY;
        assert!(on_boundary(&[1.0], &[1.0], &[(-inf, 1.0)], 0.0));
        assert!(!on_boundary(&[0.5], &[1.0], &[(-inf, inf)], 1e300));
        assert!(on_boundary(&[0.9], &[1.0], &[(0.0, 1.0)], 0.1));
        assert!(!on_boundary(&[0.5], &[1.0], &[(0.0, 1.0)], 0.1));
    }

    #[test]
    fn config_checks() {
        let ok = SeqConfig::new(CertKind::Zone);
        assert_eq!(ok.check(), Ok(()));
        let field = |c: SeqConfig| match c.check() {
            Err(SeqCause::InvalidInput { field }) => field,
            _ => "ok",
        };
        assert_eq!(field(SeqConfig { max_iter: 0, ..ok }), "max_iter");
        assert_eq!(field(SeqConfig { stable_iters: 0, ..ok }), "stable_iters");
        assert_eq!(field(SeqConfig { step_tol_rel: f64::NAN, ..ok }), "step_tol_rel");
        assert_eq!(field(SeqConfig { decrease_tol_rel: -1.0, ..ok }), "decrease_tol_rel");
        assert_eq!(field(SeqConfig { boundary_tol_rel: f64::INFINITY, ..ok }), "boundary_tol_rel");
        assert_eq!(field(SeqConfig { radius_0: 0.0, ..ok }), "radius_0");
        assert_eq!(field(SeqConfig { radius_min: -1.0, ..ok }), "radius_min");
        assert_eq!(field(SeqConfig { radius_max: 0.5, ..ok }), "radius_max");
        assert_eq!(field(SeqConfig { radius_min: 2.0, ..ok }), "ok");
    }
}
