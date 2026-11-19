//! Nonlinear least-squares driver (Gauss-Newton and Levenberg-Marquardt). Spec node
//! `KRN.SOLV.NLLS`.
//!
//! [`solve`] minimises the sum of squared residuals `cost(p) = sum_i r_i(p)^2` of an
//! [`NllsProblem`] from a start `p0` under an explicit [`NllsConfig`]. Every step is a
//! least-squares solve by column-pivoted Householder QR of the column-scaled Jacobian (or of
//! the damped, augmented system); the cross-product matrix of the Jacobian is never formed
//! (REQ-KRN.SOLV.NLLS-002). The iterate sequence is a fixed function of (problem, `p0`,
//! config): single thread, no FMA, every inner product and norm through `fid_num::red`, the
//! square root through `fid_num::f`, and every rule below pinned in the leaf's Design section.
//! The Python model in `fixtures/solv/make_nlls_fixtures.py` reproduces it bit for bit.
//!
//! # Iteration (pinned)
//!
//! With `m` residuals and `n` parameters, each iterate `p` (with residuals `r`, Jacobian `J`,
//! `cost = red::sum_sq(r)`) is assessed:
//!
//! 1. **Scaling.** `d_j = red::norm2(J[.., j])`, replaced by `1.0` when zero; the scaled
//!    Jacobian is `Js[i][j] = J[i][j] / d_j`.
//! 2. **Factor.** Pivoted QR of `Js` with right-hand side `-r` (module `qr`). The rank is the
//!    first `k` with `|r_kk| <= rank_tol * max |r_jj|` (else `n`); the condition estimate is
//!    `fid_num::la::cond_triangular(R, 0)` (`None` when not estimable). Rank `< n` is
//!    [`SolverError::RankDeficient`]; an estimate that is `None` or `> cond_limit` is
//!    [`SolverError::IllConditioned`]. Both are checked at every iterate, the last included.
//! 3. **Stop tests, in this order:** a pending stop from the previous step (below) returns the
//!    result; `cost == 0` stops with [`StopReason::ZeroResidual`]; with `g_j = red::dot(Js[..,
//!    j], r)`, `max_j |g_j| <= tol_grad * red::norm2(r)` stops with [`StopReason::Gradient`];
//!    otherwise, if `max_iter` steps have been accepted, [`SolverError::IterationLimit`].
//! 4. **Step.** Gauss-Newton: `z` solves `Js z = -r` from step 2's factor. Levenberg-Marquardt:
//!    `lambda` starts at `lm_tau` (the scaled Jacobian has unit columns, so this is `lm_tau`
//!    times the largest diagonal entry of the scaled cross-product); each trial solves
//!    `[Js; sqrt(lambda) I] z = [-r; 0]` by a fresh pivoted QR. Up to [`MAX_TRIALS`] trials.
//! 5. **Trial.** `p_t[j] = p[j] + z[j] / d_j`; evaluate; `cost_t = red::sum_sq(r_t)`;
//!    `pred = red::sum_sq(Js z) + (2 * lambda) * red::sum_sq(z)` (`lambda = 0` for GN), the
//!    model reduction, which is non-negative by construction; `act = cost - cost_t`.
//!    Gauss-Newton accepts every trial. Levenberg-Marquardt, when `pred > 0`, takes
//!    `rho = act / pred`, accepts iff `rho > ETA` (`1e-4`), and sets `lambda = lambda / 3`
//!    if `rho > 0.75` or `lambda = lambda * 2` if `rho < 0.25`, clamped to
//!    `[LAMBDA_MIN, LAMBDA_MAX]` = `[1e-200, 1e200]`; `pred == 0` rejects and keeps `lambda`.
//! 6. **Pending stop.** After an accepted trial, `red::norm2(z) <= tol_step * (red::norm2(d *
//!    p_new) + tol_step)` sets [`StopReason::Step`]. Then, after any trial, if no stop is set
//!    and `pred <= tol_cost * cost` and `|act| <= tol_cost * cost` (`cost` before the trial),
//!    [`StopReason::Cost`]. A set stop, or an accepted trial, ends the step and the new (or
//!    unchanged) iterate is assessed again (step 1), so the reported rank, condition and
//!    gradient are those of the returned `p`. All [`MAX_TRIALS`] trials rejected without a stop
//!    is [`SolverError::IterationLimit`].
//!
//! Errors carry the last [`Diagnostics`] and no solution (REQ-KRN.SOLV.NLLS-005). Every
//! buffer is reserved with `try_reserve_exact`; allocation failure is
//! [`SolverError::Numeric`], never an abort.

// `SolverError` carries the last diagnostics by value (about 150 bytes). It is returned at most
// once per solve, never in a loop, and boxing it would add an infallible allocation on the
// error path, so the size lint is allowed for this module.
#![allow(clippy::result_large_err)]

mod qr;

use core::fmt;

use fid_num::{check_finite_slice, f, red, NumError};

/// Largest accepted `max_iter`, so a run is bounded.
pub const MAX_ITER: usize = 1_000_000;
/// Levenberg-Marquardt trials per accepted step.
pub const MAX_TRIALS: usize = 64;
/// Largest parameter count (the triangular factor is a `fid_num::la::Mat`).
pub const MAX_PARAMS: usize = fid_num::la::MAX_DIM;
/// Largest `(n_resid + n_params) * n_params`, the size of the augmented system (128 MiB of
/// `f64`); every other buffer is smaller or of length `n_resid` or `n_params`.
pub const MAX_ELEMS: usize = 1 << 24;
/// Acceptance threshold on the gain ratio.
pub const ETA: f64 = 1e-4;
/// Gain ratio above which `lambda` is divided by [`LAMBDA_DOWN`].
pub const RHO_GOOD: f64 = 0.75;
/// Gain ratio below which `lambda` is multiplied by [`LAMBDA_UP`].
pub const RHO_BAD: f64 = 0.25;
/// Divisor of `lambda` after a good step.
pub const LAMBDA_DOWN: f64 = 3.0;
/// Factor of `lambda` after a poor or rejected step.
pub const LAMBDA_UP: f64 = 2.0;
/// Lower clamp of `lambda`.
pub const LAMBDA_MIN: f64 = 1e-200;
/// Upper clamp of `lambda`.
pub const LAMBDA_MAX: f64 = 1e200;

const OP: &str = "solv.nlls";

/// A least-squares problem: residuals `r(p)` and their Jacobian `J(p)`.
pub trait NllsProblem {
    /// Number of parameters `n` (at least 1).
    fn n_params(&self) -> usize;
    /// Number of residuals `m` (at least `n`).
    fn n_resid(&self) -> usize;
    /// Writes `r(p)` into `r` (length `m`) and `J(p)` into `j` (row-major `m x n`,
    /// `j[i * n + k] = d r_i / d p_k`). Both buffers hold the previous values on entry and must
    /// be overwritten entirely. An `Err` stops the driver with [`SolverError::Numeric`].
    fn eval(&self, p: &[f64], r: &mut [f64], j: &mut [f64]) -> Result<(), NumError>;
}

/// Step method.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    /// Undamped Gauss-Newton: every step is accepted.
    GaussNewton,
    /// Levenberg-Marquardt with the fixed gain-ratio rules of the module documentation.
    LevenbergMarquardt,
}

/// Driver configuration. Every field is explicit and echoed in [`NllsResult::config`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NllsConfig {
    pub method: Method,
    /// Maximum number of accepted steps, `0..=MAX_ITER`.
    pub max_iter: usize,
    /// Gradient test: `max_j |Js[.., j] . r| <= tol_grad * ||r||` (finite, not negative).
    pub tol_grad: f64,
    /// Step test: `||z|| <= tol_step * (||d * p|| + tol_step)` (finite, not negative).
    pub tol_step: f64,
    /// Cost test: predicted and actual reduction both `<= tol_cost * cost` (finite, not
    /// negative).
    pub tol_cost: f64,
    /// Largest accepted condition estimate of the scaled Jacobian (finite, at least 1).
    pub cond_limit: f64,
    /// Relative tolerance of the rank rule (finite, in `[0, 1)`).
    pub rank_tol: f64,
    /// Initial Levenberg-Marquardt damping (finite, positive).
    pub lm_tau: f64,
}

impl Default for NllsConfig {
    /// The defaults pinned in the leaf's Design section.
    fn default() -> Self {
        NllsConfig {
            method: Method::LevenbergMarquardt,
            max_iter: 200,
            tol_grad: 1e-10,
            tol_step: 1e-12,
            tol_cost: 1e-14,
            cond_limit: 1e12,
            rank_tol: 1e-12,
            lm_tau: 1e-3,
        }
    }
}

/// Why the driver stopped with a solution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopReason {
    /// The residuals are exactly zero.
    ZeroResidual,
    /// The gradient test held.
    Gradient,
    /// The step test held after an accepted step.
    Step,
    /// The cost test held.
    Cost,
}

/// A converged solution with its diagnostics (REQ-KRN.SOLV.NLLS-004).
#[derive(Debug, Clone, PartialEq)]
pub struct NllsResult {
    pub p: Vec<f64>,
    pub residuals: Vec<f64>,
    /// `red::sum_sq(residuals)`.
    pub cost: f64,
    /// `||J^T r||` at `p` (each component one `red::dot` over a Jacobian column).
    pub grad_norm: f64,
    /// Numerical rank of the scaled Jacobian at `p` (always `n` on success).
    pub rank: usize,
    /// Condition estimate of the scaled Jacobian at `p`.
    pub cond: f64,
    /// Accepted steps.
    pub iterations: usize,
    /// Calls of [`NllsProblem::eval`].
    pub evaluations: usize,
    /// Final Levenberg-Marquardt damping (`None` for Gauss-Newton or when no step was tried).
    pub lambda: Option<f64>,
    pub stop_reason: StopReason,
    pub config: NllsConfig,
}

/// The last known state, carried by every [`SolverError`]. Fields not reached yet are `None`.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Diagnostics {
    pub iterations: usize,
    pub evaluations: usize,
    pub cost: Option<f64>,
    pub grad_norm: Option<f64>,
    pub rank: Option<usize>,
    pub cond: Option<f64>,
    pub lambda: Option<f64>,
}

/// Where a NaN or infinity appeared.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    /// The start vector.
    Start,
    /// Residuals returned by the problem.
    Residual,
    /// Jacobian returned by the problem (index is row-major).
    Jacobian,
    /// The sum of squared residuals overflowed.
    Cost,
    /// A step or trial parameter vector overflowed.
    Step,
}

/// Why no solution is returned (REQ-KRN.SOLV.NLLS-005).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SolverError {
    /// `max_iter` steps were accepted without a stop, or [`MAX_TRIALS`] trials were rejected.
    IterationLimit { last: Diagnostics },
    /// The scaled Jacobian's rank is below `n`.
    RankDeficient { rank: usize, cond: Option<f64>, last: Diagnostics },
    /// The condition estimate exceeds `cond_limit` (`None`: beyond the `f64` range).
    IllConditioned { cond: Option<f64>, last: Diagnostics },
    /// A NaN or infinity at `stage`, with the first offending index when known.
    NonFinite { stage: Stage, index: Option<usize>, last: Diagnostics },
    /// A configuration value or problem size is out of range; `field` names it.
    InvalidConfig { field: &'static str, last: Diagnostics },
    /// The problem's `eval` returned an error, a buffer could not be reserved, or an internal
    /// product overflowed.
    Numeric { error: NumError, last: Diagnostics },
}

impl SolverError {
    /// The diagnostics at the point of failure.
    pub fn last(&self) -> &Diagnostics {
        match self {
            SolverError::IterationLimit { last }
            | SolverError::RankDeficient { last, .. }
            | SolverError::IllConditioned { last, .. }
            | SolverError::NonFinite { last, .. }
            | SolverError::InvalidConfig { last, .. }
            | SolverError::Numeric { last, .. } => last,
        }
    }
}

impl fmt::Display for SolverError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SolverError::IterationLimit { last } => write!(f, "{OP}: iteration limit after {} steps", last.iterations),
            SolverError::RankDeficient { rank, .. } => write!(f, "{OP}: Jacobian rank deficient (rank {rank})"),
            SolverError::IllConditioned { cond: Some(c), .. } => write!(f, "{OP}: Jacobian ill-conditioned ({c:e})"),
            SolverError::IllConditioned { cond: None, .. } => write!(f, "{OP}: Jacobian condition beyond range"),
            SolverError::NonFinite { stage, index, .. } => {
                write!(f, "{OP}: non-finite value in {stage:?} at {index:?}")
            }
            SolverError::InvalidConfig { field, .. } => write!(f, "{OP}: invalid {field}"),
            SolverError::Numeric { error, .. } => write!(f, "{OP}: {error}"),
        }
    }
}

impl core::error::Error for SolverError {}

fn alloc_err() -> NumError {
    NumError::Domain { op: OP, detail: "allocation_failed" }
}

/// An empty vector with capacity for exactly `n` elements, reserved fallibly.
pub(crate) fn alloc(n: usize) -> Result<Vec<f64>, NumError> {
    let mut v = Vec::new();
    v.try_reserve_exact(n).map_err(|_| alloc_err())?;
    Ok(v)
}

fn zeros(n: usize) -> Result<Vec<f64>, NumError> {
    let mut v = alloc(n)?;
    v.resize(n, 0.0);
    Ok(v)
}

pub(crate) fn alloc_usize(n: usize) -> Result<Vec<usize>, NumError> {
    let mut v = Vec::new();
    v.try_reserve_exact(n).map_err(|_| alloc_err())?;
    v.resize(n, 0);
    Ok(v)
}

fn first_bad(xs: &[f64]) -> Option<usize> {
    xs.iter().position(|x| !x.is_finite())
}

/// What one assessment (steps 1-3) found.
struct Assessment {
    factor: qr::Factored,
    gmax: f64,
    rnorm: f64,
}

/// Run state; `diag` is what an error reports.
struct Run<'a, P: NllsProblem + ?Sized> {
    prob: &'a P,
    cfg: NllsConfig,
    m: usize,
    n: usize,
    diag: Diagnostics,
    p: Vec<f64>,
    r: Vec<f64>,
    jac: Vec<f64>,
    pt: Vec<f64>,
    rt: Vec<f64>,
    jt: Vec<f64>,
    js: Vec<f64>,
    d: Vec<f64>,
    z: Vec<f64>,
    work: Vec<f64>,
    rhs: Vec<f64>,
    v: Vec<f64>,
    colbuf: Vec<f64>,
    nbuf: Vec<f64>,
}

impl<P: NllsProblem + ?Sized> Run<'_, P> {
    fn err_num(&self, error: NumError) -> SolverError {
        SolverError::Numeric { error, last: self.diag }
    }

    fn non_finite(&self, stage: Stage, index: Option<usize>) -> SolverError {
        SolverError::NonFinite { stage, index, last: self.diag }
    }

    /// Evaluates at `pt` into `rt`/`jt` and checks the output.
    fn eval_trial(&mut self) -> Result<(), SolverError> {
        self.diag.evaluations += 1;
        if let Err(e) = self.prob.eval(&self.pt, &mut self.rt, &mut self.jt) {
            return Err(self.err_num(e));
        }
        if let Some(i) = first_bad(&self.rt) {
            return Err(self.non_finite(Stage::Residual, Some(i)));
        }
        if let Some(i) = first_bad(&self.jt) {
            return Err(self.non_finite(Stage::Jacobian, Some(i)));
        }
        Ok(())
    }

    fn sum_sq_cost(&self, r: &[f64]) -> Result<f64, SolverError> {
        red::sum_sq(r).map_err(|_| self.non_finite(Stage::Cost, None))
    }

    /// Steps 1-3 (scaling, factor, rank and condition checks, gradient).
    fn assess(&mut self) -> Result<Assessment, SolverError> {
        let (m, n) = (self.m, self.n);
        for c in 0..n {
            self.colbuf.clear();
            self.colbuf.extend((0..m).map(|i| self.jac[i * n + c]));
            let dc = red::norm2(&self.colbuf).map_err(|e| self.err_num(e))?;
            if let Some(slot) = self.d.get_mut(c) {
                *slot = if dc == 0.0 { 1.0 } else { dc };
            }
        }
        for i in 0..m {
            for c in 0..n {
                self.js[i * n + c] = self.jac[i * n + c] / self.d[c];
            }
        }
        // Column-major copy of Js and the right-hand side -r.
        for c in 0..n {
            for i in 0..m {
                self.work[c * m + i] = self.js[i * n + c];
            }
        }
        for (b, &ri) in self.rhs.iter_mut().zip(&self.r) {
            *b = -ri;
        }
        let factor =
            qr::factor(&mut self.work[..m * n], m, n, &mut self.rhs[..m], &mut self.v).map_err(|e| self.err_num(e))?;
        let rank = qr::rank(&factor, self.cfg.rank_tol);
        let cond = qr::cond(&factor).map_err(|e| self.err_num(e))?;
        // Gradient: scaled for the test, unscaled for the report.
        let mut gmax = 0.0_f64;
        for c in 0..n {
            self.colbuf.clear();
            self.colbuf.extend((0..m).map(|i| self.js[i * n + c]));
            let g = red::dot(&self.colbuf, &self.r).map_err(|e| self.err_num(e))?;
            if g.abs() > gmax {
                gmax = g.abs();
            }
        }
        for c in 0..n {
            self.colbuf.clear();
            self.colbuf.extend((0..m).map(|i| self.jac[i * n + c]));
            let g = red::dot(&self.colbuf, &self.r).map_err(|e| self.err_num(e))?;
            if let Some(slot) = self.nbuf.get_mut(c) {
                *slot = g;
            }
        }
        let grad_norm = red::norm2(&self.nbuf).map_err(|e| self.err_num(e))?;
        let rnorm = red::norm2(&self.r).map_err(|e| self.err_num(e))?;
        self.diag.grad_norm = Some(grad_norm);
        self.diag.rank = Some(rank);
        self.diag.cond = cond;
        if rank < n {
            return Err(SolverError::RankDeficient { rank, cond, last: self.diag });
        }
        match cond {
            Some(c) if c <= self.cfg.cond_limit => {}
            _ => return Err(SolverError::IllConditioned { cond, last: self.diag }),
        }
        Ok(Assessment { factor, gmax, rnorm })
    }

    /// The Levenberg-Marquardt trial step for damping `lambda` into `z`.
    fn lm_step(&mut self, lambda: f64) -> Result<(), SolverError> {
        let (m, n) = (self.m, self.n);
        let rows = m + n;
        let sl = f::sqrt(lambda).map_err(|e| self.err_num(e))?;
        for c in 0..n {
            for i in 0..m {
                self.work[c * rows + i] = self.js[i * n + c];
            }
            for i in 0..n {
                self.work[c * rows + m + i] = if i == c { sl } else { 0.0 };
            }
        }
        for (b, &ri) in self.rhs.iter_mut().zip(&self.r) {
            *b = -ri;
        }
        for x in &mut self.rhs[m..rows] {
            *x = 0.0;
        }
        let factor = qr::factor(&mut self.work, rows, n, &mut self.rhs, &mut self.v).map_err(|e| self.err_num(e))?;
        self.solve_into_z(&factor)
    }

    fn solve_into_z(&mut self, factor: &qr::Factored) -> Result<(), SolverError> {
        match qr::solve(factor, &mut self.z) {
            Ok(()) => Ok(()),
            Err(NumError::Infinite { .. } | NumError::Singular { .. }) => Err(self.non_finite(Stage::Step, None)),
            Err(e) => Err(self.err_num(e)),
        }
    }

    /// `||z||` and the model reduction `pred` for damping `lambda`.
    fn predicted(&mut self, lambda: f64) -> Result<f64, SolverError> {
        let n = self.n;
        self.colbuf.clear();
        for i in 0..self.m {
            let v = red::dot(&self.js[i * n..(i + 1) * n], &self.z).map_err(|e| self.err_num(e))?;
            self.colbuf.push(v);
        }
        let jz = red::sum_sq(&self.colbuf).map_err(|e| self.err_num(e))?;
        let zz = red::sum_sq(&self.z).map_err(|e| self.err_num(e))?;
        let pred = jz + (2.0 * lambda) * zz;
        if pred.is_finite() {
            Ok(pred)
        } else {
            Err(self.non_finite(Stage::Step, None))
        }
    }

    /// The result at the current iterate, just assessed (so `cond` and `grad_norm` are set).
    fn finish(&mut self, reason: StopReason, cost: f64, lambda: Option<f64>) -> Result<NllsResult, SolverError> {
        let (Some(cond), Some(grad_norm)) = (self.diag.cond, self.diag.grad_norm) else {
            return Err(SolverError::IllConditioned { cond: None, last: self.diag });
        };
        Ok(NllsResult {
            p: core::mem::take(&mut self.p),
            residuals: core::mem::take(&mut self.r),
            cost,
            grad_norm,
            rank: self.n,
            cond,
            iterations: self.diag.iterations,
            evaluations: self.diag.evaluations,
            lambda,
            stop_reason: reason,
            config: self.cfg,
        })
    }

    fn run(&mut self) -> Result<NllsResult, SolverError> {
        if let Err(e) = self.prob.eval(&self.p, &mut self.r, &mut self.jac) {
            self.diag.evaluations = 1;
            return Err(self.err_num(e));
        }
        self.diag.evaluations = 1;
        if let Some(i) = first_bad(&self.r) {
            return Err(self.non_finite(Stage::Residual, Some(i)));
        }
        if let Some(i) = first_bad(&self.jac) {
            return Err(self.non_finite(Stage::Jacobian, Some(i)));
        }
        let mut cost = self.sum_sq_cost(&self.r)?;
        self.diag.cost = Some(cost);
        let gn = self.cfg.method == Method::GaussNewton;
        let mut lambda: Option<f64> = None;
        let mut pending: Option<StopReason> = None;
        loop {
            let a = self.assess()?;
            if let Some(reason) = pending {
                return self.finish(reason, cost, lambda);
            }
            if cost == 0.0 {
                pending = Some(StopReason::ZeroResidual);
                continue;
            }
            if a.gmax <= self.cfg.tol_grad * a.rnorm {
                pending = Some(StopReason::Gradient);
                continue;
            }
            if self.diag.iterations >= self.cfg.max_iter {
                return Err(SolverError::IterationLimit { last: self.diag });
            }
            if gn {
                self.solve_into_z(&a.factor)?;
            }
            let mut accepted = false;
            for _ in 0..(if gn { 1 } else { MAX_TRIALS }) {
                let lam_t = if gn {
                    0.0
                } else {
                    let l = match lambda {
                        Some(l) => l,
                        None => self.cfg.lm_tau,
                    };
                    lambda = Some(l);
                    self.lm_step(l)?;
                    l
                };
                for (j, slot) in self.pt.iter_mut().enumerate() {
                    *slot = self.p[j] + self.z[j] / self.d[j];
                }
                if let Some(j) = first_bad(&self.pt) {
                    return Err(self.non_finite(Stage::Step, Some(j)));
                }
                self.eval_trial()?;
                let cost_t = self.sum_sq_cost(&self.rt)?;
                let pred = self.predicted(lam_t)?;
                let act = cost - cost_t;
                let tc = self.cfg.tol_cost * cost;
                if !gn {
                    if pred > 0.0 {
                        let rho = act / pred;
                        accepted = rho > ETA;
                        let mut l = lam_t;
                        if rho > RHO_GOOD {
                            l /= LAMBDA_DOWN;
                            if l < LAMBDA_MIN {
                                l = LAMBDA_MIN;
                            }
                        } else if rho < RHO_BAD {
                            l *= LAMBDA_UP;
                            if l > LAMBDA_MAX {
                                l = LAMBDA_MAX;
                            }
                        }
                        lambda = Some(l);
                    } else {
                        accepted = false;
                    }
                    self.diag.lambda = lambda;
                } else {
                    accepted = true;
                }
                if accepted {
                    core::mem::swap(&mut self.p, &mut self.pt);
                    core::mem::swap(&mut self.r, &mut self.rt);
                    core::mem::swap(&mut self.jac, &mut self.jt);
                    cost = cost_t;
                    self.diag.iterations += 1;
                    self.diag.cost = Some(cost);
                    for ((slot, &dj), &pj) in self.nbuf.iter_mut().zip(&self.d).zip(&self.p) {
                        *slot = dj * pj;
                    }
                    let dpn = red::norm2(&self.nbuf).map_err(|e| self.err_num(e))?;
                    let zn = red::norm2(&self.z).map_err(|e| self.err_num(e))?;
                    if zn <= self.cfg.tol_step * (dpn + self.cfg.tol_step) {
                        pending = Some(StopReason::Step);
                    }
                }
                if pending.is_none() && pred <= tc && act.abs() <= tc {
                    pending = Some(StopReason::Cost);
                }
                if accepted || pending.is_some() {
                    break;
                }
            }
            if !accepted && pending.is_none() {
                return Err(SolverError::IterationLimit { last: self.diag });
            }
        }
    }
}

fn check_config(cfg: &NllsConfig) -> Option<&'static str> {
    let nonneg = |x: f64| x.is_finite() && x >= 0.0;
    if cfg.max_iter > MAX_ITER {
        return Some("max_iter");
    }
    if !nonneg(cfg.tol_grad) {
        return Some("tol_grad");
    }
    if !nonneg(cfg.tol_step) {
        return Some("tol_step");
    }
    if !nonneg(cfg.tol_cost) {
        return Some("tol_cost");
    }
    if !(cfg.cond_limit.is_finite() && cfg.cond_limit >= 1.0) {
        return Some("cond_limit");
    }
    if !(nonneg(cfg.rank_tol) && cfg.rank_tol < 1.0) {
        return Some("rank_tol");
    }
    if !(cfg.lm_tau.is_finite() && cfg.lm_tau > 0.0) {
        return Some("lm_tau");
    }
    None
}

/// Minimises `sum_i r_i(p)^2` from `p0` (see the module documentation for the pinned rules).
///
/// Check order before the first evaluation: the configuration fields in declaration order,
/// then `n_params` (`1..=MAX_PARAMS`), `n_resid` (`>= n_params`, and `(n_resid + n_params) *
/// n_params <= MAX_ELEMS`), `p0.len() == n_params` (all [`SolverError::InvalidConfig`]),
/// then the first NaN or infinity in `p0` ([`SolverError::NonFinite`] at [`Stage::Start`]).
pub fn solve<P: NllsProblem + ?Sized>(prob: &P, p0: &[f64], cfg: &NllsConfig) -> Result<NllsResult, SolverError> {
    let last = Diagnostics::default();
    let invalid = |field| Err(SolverError::InvalidConfig { field, last });
    if let Some(field) = check_config(cfg) {
        return invalid(field);
    }
    let n = prob.n_params();
    let m = prob.n_resid();
    if n == 0 || n > MAX_PARAMS {
        return invalid("n_params");
    }
    let elems = m.checked_add(n).and_then(|t| t.checked_mul(n));
    if m < n || !matches!(elems, Some(e) if e <= MAX_ELEMS) {
        return invalid("n_resid");
    }
    if p0.len() != n {
        return invalid("p0");
    }
    if let Err(e) = check_finite_slice(OP, p0) {
        let index = match e {
            NumError::NotANumber { index, .. } | NumError::Infinite { index, .. } => index,
            _ => None,
        };
        return Err(SolverError::NonFinite { stage: Stage::Start, index, last });
    }
    let num = |error| SolverError::Numeric { error, last };
    let mut p = alloc(n).map_err(num)?;
    p.extend_from_slice(p0);
    let v = alloc(m + n).map_err(num)?;
    let mut run = Run {
        prob,
        cfg: *cfg,
        m,
        n,
        diag: last,
        p,
        r: zeros(m).map_err(num)?,
        jac: zeros(m * n).map_err(num)?,
        pt: zeros(n).map_err(num)?,
        rt: zeros(m).map_err(num)?,
        jt: zeros(m * n).map_err(num)?,
        js: zeros(m * n).map_err(num)?,
        d: zeros(n).map_err(num)?,
        z: zeros(n).map_err(num)?,
        work: zeros((m + n) * n).map_err(num)?,
        rhs: zeros(m + n).map_err(num)?,
        v,
        colbuf: alloc(m).map_err(num)?,
        nbuf: zeros(n).map_err(num)?,
    };
    run.run()
}
