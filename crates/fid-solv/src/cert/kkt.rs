//! Independent, proven KKT optimality check at the recomputed point (spec node
//! KRN.SOLV.CERT.KKT).
//!
//! [`check`] decides whether the optimum recomputed on a candidate active set
//! ([`Recomputed`], `KRN.SOLV.CERT.RECOMP`) satisfies the KKT conditions of the problem
//! `min f(p) subject to g_i(p) <= 0`, without trusting the solver: the multipliers are solved
//! from the stationarity equations at the recomputed point, never taken from a conic solution.
//!
//! **Proven, not estimated.** A [`Recomputed`] carries a box `X = params +- param_bounds` that
//! is proven to hold the exact root `p*` of the active-set equations. Every condition below is
//! evaluated over `X` in outward-rounded [`Interval`] arithmetic, from the problem's enclosure
//! methods, so each recorded residual is an interval `[lo, hi]` proven to contain the exact
//! (real-arithmetic) residual at `p*`. A condition is
//!
//! - [`Verdict::Satisfied`] when `hi <= threshold`,
//! - [`Verdict::Violated`] when `lo > threshold`,
//! - [`Verdict::Undecidable`] otherwise: the enclosure straddles the threshold, and a
//!   borderline case is never rounded into a pass.
//!
//! The report is [`KktStatus::Certified`] only when all four conditions are satisfied;
//! [`KktStatus::Violated`] when any is certainly violated (first in the order primal, dual,
//! stationarity, complementarity); [`KktStatus::Undecidable`] otherwise.
//!
//! # Procedure (pinned)
//!
//! 1. **Checks**, in this order: the tolerances (`InvalidInput { field: "tolerances" }`: each
//!    finite and `>= 0`, `rank_tol < 1`), [`ActiveSetProblem::data_scale`] (its error, by
//!    default `InvalidInput { field: "data_scale_unset" }`, is returned; a value that is not
//!    finite and `> 0` is `InvalidInput { field: "data_scale" }`), the recomputed set of size `n_params`
//!    (`BoundNotProven { NotSquare }` otherwise) drawn from the unique row labels
//!    (`DuplicateLabel`, `InvalidInput { field: "set" }`), and `params` of length `n_params`
//!    (`Shape`).
//! 2. `X = params +- param_bounds`, rounded outward.
//! 3. **Enclosures** (fail closed): [`ActiveSetProblem::constraint_enclosure`] over `X` (every
//!    row), [`ActiveSetProblem::objective_gradient_enclosure`] over `X` and
//!    [`ActiveSetProblem::constraint_gradient_enclosure`] of the set over `X`. A `None` is
//!    `BoundNotProven { NoEnclosure }`, a wrong length `Shape`.
//! 4. **Primal feasibility** (REQ-002): `primal = max_i g_i(p*)` over every row, enclosed by
//!    `[max_i lo(G_i), max_i hi(G_i)]`; threshold `tol_p * data_scale`.
//! 5. **Complementarity**: `max |g_i(p*)|` over the set's rows, enclosed by the largest
//!    mignitude and magnitude of their enclosures; threshold `tol_c * data_scale`.
//! 6. **Multipliers** (REQ-001): with the float gradients at `params`
//!    ([`ActiveSetProblem::objective_gradient`], [`ActiveSetProblem::constraint_gradients`]),
//!    `A = G^T` is factored once by `qr_pivoted(A, rank_tol)` and `lambda` solved from that
//!    factor with the arithmetic of `fid_num::la::lstsq(A, -grad f, rank_tol)`: the stationarity
//!    equations of the Lagrangian `grad f + G^T lambda = 0`. A rank below `n_params` is
//!    [`KktFail::DegenerateGradients`] (REQ-004): no multipliers, dual feasibility and
//!    stationarity undecidable. Otherwise a Krawczyk test on the interval linear system
//!    `[G(X)]^T lambda = -[grad f(X)]` (the same test and constants as the recomputation
//!    bound) proves that the exact multipliers `lambda*` at `p*` are unique and lie in
//!    `lambda +- multiplier_bounds`. A test that does not contract is
//!    [`KktFail::MultipliersNotProven`] (undecidable, not an error).
//! 7. **Dual feasibility**, per active row `i` against its own threshold
//!    `thr_i = tol_d * lo(||grad f(X)||_2) / hi(||grad g_i(X)||_2)`: the size a negative
//!    multiplier may have before its term `lambda_i grad g_i` is no longer negligible against
//!    `grad f`. Both norms are bounded over `X` from the enclosures (the lower bound of the
//!    objective gradient norm, the upper bound of the constraint gradient norm), and the
//!    product and quotient are rounded down, so the threshold is never larger than the exact
//!    one. If the lower bound of `||grad f||` is 0, `thr_i = 0`: the row passes only if
//!    `lambda*_i >= 0` is proven (fail closed toward violated or undecidable). A row whose
//!    gradient enclosure is identically zero (every entry `[0, 0]`, contradicting the full-rank
//!    float gradients) is `InvalidInput { field: "constraint_gradient_zero" }`; a threshold that
//!    overflows is `InvalidInput { field: "dual_threshold" }`. Because each `thr_i` is rounded
//!    down, a dual verdict of Violated is relative to the recorded threshold: the rounding can
//!    only produce false negatives (a row judged against a slightly smaller threshold), never a
//!    false pass. Row `i` is satisfied when `lo(Lambda_i) >= -thr_i`,
//!    violated when `hi(Lambda_i) < -thr_i`, undecidable otherwise; the condition is violated
//!    if any row is, satisfied if every row is, undecidable otherwise. Its recorded enclosure
//!    is that of `max(0, -min_i lambda*_i)` and its recorded threshold only the smallest
//!    `thr_i`, so a satisfied dual condition may have `hi` above that threshold: verifiers must
//!    recheck row by row against [`KktReport::dual_thresholds`] (which `KRN.SOLV.CERT.CT` puts
//!    into the certificate). (A threshold relative to
//!    `max |lambda|` would let a large multiplier excuse a materially negative one.)
//! 8. **Stationarity**: `|| G(p*)^T lambda + grad f(p*) ||_2` at the reported **float**
//!    multipliers `lambda` (not at `lambda*`), enclosed componentwise and then normed with
//!    upward (downward) rounding for `hi` (`lo`); threshold `tol_s * scale_gradient` with
//!    `scale_gradient = red::norm2(grad f(params))` (1 if 0). This verdict measures how accurate
//!    the float multipliers are, so that a verifier can recheck it from the recorded values;
//!    exact stationarity at `lambda*` is already proven by step 6. A false negative is possible:
//!    an inaccurate float solve can fail (or, at a tolerance of 0, certainly violate) this
//!    condition at a true KKT point.
//!
//! The thresholds of primal feasibility, complementarity and stationarity are the `f64`
//! product `tol * scale` (one rounding). Every threshold is recorded in the report and
//! compared exactly, so a verifier repeats the same decision; a threshold that is not finite
//! is `InvalidInput { field: "tolerances" }`.
//!
//! # Determinism
//!
//! Single thread, no FMA, no `f64::min`/`max` (total-order helpers of `fid_num::f`), no hash
//! containers, fixed-order loops; factorizations and norms go through `fid_num::la` and
//! `fid_num::red`, and the interval arithmetic uses only IEEE `+ - * /`, `abs` and
//! `fid_num::f::sqrt`, so the report is bit-identical on x86-64, ARM64 and wasm32 (REQ-005).

use fid_num::f::{self, total_max, total_min};
use fid_num::la::qr_pivoted;
use fid_num::red;

use super::recomp::interval::{add_up, mul_up, next_down, next_up};
use super::recomp::{
    approximate_inverse, boxed, check_vec, krawczyk, la_err, not_proven, solve_factored, ActiveSetProblem, Interval,
    Recomputed, DEFAULT_REL_TOL,
};
use super::{BoundFailure, CertError, RowLabel};

/// Default relative tolerance of each KKT condition ([unverified]).
pub const DEFAULT_KKT_TOL: f64 = 1e-9;

/// Relative tolerances of the four conditions and the rank tolerance of the multiplier solve.
/// The absolute thresholds are `tol_p * data_scale`, the per-row
/// `tol_d * ||grad f|| / ||grad g_i||`, `tol_s * scale_gradient` and `tol_c * data_scale` (see
/// the module docs).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct KktTolerances {
    /// Primal feasibility: `max_i g_i <= tol_p * data_scale`.
    pub tol_p: f64,
    /// Dual feasibility: `-lambda_i <= tol_d * ||grad f|| / ||grad g_i||` for every active row.
    pub tol_d: f64,
    /// Stationarity: `||G^T lambda + grad f||_2 <= tol_s * scale_gradient`.
    pub tol_s: f64,
    /// Complementarity: `max |g_i|` over the active rows `<= tol_c * data_scale`.
    pub tol_c: f64,
    /// Relative rank tolerance of the QR factorization of `G^T` (`0 <= rank_tol < 1`).
    pub rank_tol: f64,
}

impl Default for KktTolerances {
    fn default() -> KktTolerances {
        KktTolerances {
            tol_p: DEFAULT_KKT_TOL,
            tol_d: DEFAULT_KKT_TOL,
            tol_s: DEFAULT_KKT_TOL,
            tol_c: DEFAULT_KKT_TOL,
            rank_tol: DEFAULT_REL_TOL,
        }
    }
}

impl KktTolerances {
    pub(crate) fn check(&self) -> Result<(), CertError> {
        let ok_tol = |t: f64| t.is_finite() && t >= 0.0;
        let ok = ok_tol(self.tol_p)
            && ok_tol(self.tol_d)
            && ok_tol(self.tol_s)
            && ok_tol(self.tol_c)
            && ok_tol(self.rank_tol)
            && self.rank_tol < 1.0;
        if ok {
            Ok(())
        } else {
            Err(CertError::InvalidInput { field: "tolerances" })
        }
    }
}

/// The outcome of one condition. For primal feasibility, stationarity and complementarity it
/// compares the enclosure with the single recorded threshold. Dual feasibility is decided row
/// by row against [`KktReport::dual_thresholds`]; its `Satisfied` does not imply `hi <=
/// threshold` for the recorded (smallest) threshold, and its `Violated` is relative to the
/// rounded-down per-row thresholds (false negatives only).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// The exact residual is certainly at most the threshold.
    Satisfied,
    /// The exact residual is certainly above the threshold.
    Violated,
    /// The proven enclosure straddles the threshold (or no enclosure exists).
    Undecidable,
}

/// Why a report does not pass. Later leaves may add reasons, so the enum is non-exhaustive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum KktFail {
    /// Primal feasibility.
    Primal,
    /// Dual feasibility (multiplier signs).
    Dual,
    /// Stationarity.
    Stationarity,
    /// Complementarity.
    Complementarity,
    /// The active gradient matrix is rank deficient under `rank_tol` (REQ-004).
    DegenerateGradients,
    /// The multipliers could not be proven unique (the Krawczyk test did not contract).
    MultipliersNotProven,
}

/// The overall status of a report.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KktStatus {
    /// Every condition is certainly satisfied.
    Certified,
    /// At least one condition is certainly violated.
    Violated,
    /// No condition is certainly violated, but at least one is not certainly satisfied.
    Undecidable,
}

/// One condition: a proven enclosure `[lo, hi]` of its exact residual, the recorded absolute
/// threshold and the verdict. `hi` is `+inf` when no bound exists (degenerate gradients or
/// unproven multipliers). For the dual condition, `threshold` is only the smallest per-row
/// threshold and the verdict is decided row by row: verifiers must use
/// [`KktReport::dual_thresholds`], not `hi <= threshold`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Condition {
    lo: f64,
    hi: f64,
    threshold: f64,
    verdict: Verdict,
}

impl Condition {
    fn decide(lo: f64, hi: f64, threshold: f64) -> Condition {
        let verdict = if hi <= threshold {
            Verdict::Satisfied
        } else if lo > threshold {
            Verdict::Violated
        } else {
            Verdict::Undecidable
        };
        Condition { lo, hi, threshold, verdict }
    }
    fn unknown(threshold: f64) -> Condition {
        Condition { lo: 0.0, hi: f64::INFINITY, threshold, verdict: Verdict::Undecidable }
    }
    /// Proven lower bound of the exact residual.
    pub fn lo(&self) -> f64 {
        self.lo
    }
    /// Proven upper bound of the exact residual (`+inf` when none exists).
    pub fn hi(&self) -> f64 {
        self.hi
    }
    /// The absolute threshold (`tol * scale`; for the dual condition the smallest per-row
    /// threshold, see [`KktReport::dual_thresholds`]).
    pub fn threshold(&self) -> f64 {
        self.threshold
    }
    /// The verdict.
    pub fn verdict(&self) -> Verdict {
        self.verdict
    }
}

/// The scales the relative tolerances were multiplied by.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct KktScales {
    /// The caller's data scale (max |constraint data|), for primal feasibility and
    /// complementarity.
    pub data: f64,
    /// Proven lower bound of `||grad f||_2` over the box, for the dual thresholds (0 when the
    /// gradients are degenerate and no dual threshold is formed).
    pub gradient_lo: f64,
    /// `red::norm2` of the float objective gradient (1 if 0), for stationarity.
    pub gradient: f64,
}

/// The result of [`check`]. Made only by [`check`]; read through the accessors.
#[derive(Debug, Clone, PartialEq)]
pub struct KktReport {
    multipliers: Vec<f64>,
    multiplier_bounds: Vec<f64>,
    dual_thresholds: Vec<f64>,
    primal: Condition,
    dual: Condition,
    stationarity: Condition,
    complementarity: Condition,
    tolerances: KktTolerances,
    scales: KktScales,
    status: KktStatus,
    reason: Option<KktFail>,
}

impl KktReport {
    /// The multipliers solved at the recomputed point, one per active row in set order (empty
    /// for [`KktFail::DegenerateGradients`]).
    pub fn multipliers(&self) -> &[f64] {
        &self.multipliers
    }
    /// Proven bounds `|multipliers[i] - lambda*_i| <= multiplier_bounds[i]` for the exact
    /// multipliers at the exact optimum (`+inf` when not proven; empty when degenerate).
    pub fn multiplier_bounds(&self) -> &[f64] {
        &self.multiplier_bounds
    }
    /// The per-row dual thresholds `thr_i`, in set order (empty when degenerate).
    pub fn dual_thresholds(&self) -> &[f64] {
        &self.dual_thresholds
    }
    /// Primal feasibility: `max_i g_i(p*)` over every row.
    pub fn primal(&self) -> Condition {
        self.primal
    }
    /// Dual feasibility: `max(0, -min_i lambda*_i)`.
    pub fn dual(&self) -> Condition {
        self.dual
    }
    /// Stationarity: `||G(p*)^T lambda + grad f(p*)||_2` at the float [`Self::multipliers`]
    /// (measures their accuracy; see the module docs, step 8).
    pub fn stationarity(&self) -> Condition {
        self.stationarity
    }
    /// Complementarity: `max |g_i(p*)|` over the active rows.
    pub fn complementarity(&self) -> Condition {
        self.complementarity
    }
    /// The tolerances used.
    pub fn tolerances(&self) -> KktTolerances {
        self.tolerances
    }
    /// The scales used.
    pub fn scales(&self) -> KktScales {
        self.scales
    }
    /// The overall status.
    pub fn status(&self) -> KktStatus {
        self.status
    }
    /// Why the report does not pass (`None` exactly when certified).
    pub fn reason(&self) -> Option<KktFail> {
        self.reason
    }
    /// `true` exactly when [`Self::status`] is [`KktStatus::Certified`].
    pub fn pass(&self) -> bool {
        self.status == KktStatus::Certified
    }
}

/// Checks the KKT conditions at the exact optimum enclosed by `rec`, recomputed for `problem`.
/// The data scale of the primal and complementarity thresholds is
/// [`ActiveSetProblem::data_scale`]. See the module docs for the pinned procedure. Failing conditions are report content, not
/// errors.
///
/// # Errors
///
/// Input errors (`InvalidInput`, `Shape`, `DuplicateLabel`), [`CertError::BoundNotProven`] (a
/// missing enclosure, a non-square set, an overflow), [`CertError::Numeric`], and any error a
/// callback returns.
pub fn check<P: ActiveSetProblem + ?Sized>(
    problem: &P,
    rec: &Recomputed,
    tol: &KktTolerances,
) -> Result<KktReport, CertError> {
    tol.check()?;
    let data_scale = problem.data_scale()?;
    if !(data_scale.is_finite() && data_scale > 0.0) {
        return Err(CertError::InvalidInput { field: "data_scale" });
    }
    let n = problem.n_params();
    let set = &rec.set;
    if n == 0 || set.len() != n {
        return Err(not_proven(BoundFailure::NotSquare));
    }
    let rows = problem.row_labels();
    let active = active_indices(&rows, set)?;
    check_vec("params", &rec.params, n)?;
    if rec.param_bounds.len() != n {
        return Err(CertError::Shape { field: "param_bounds", expected: n, got: rec.param_bounds.len() });
    }
    let x = boxed(&rec.params, &rec.param_bounds)?;

    // 3. Enclosures (fail closed).
    let g = problem.constraint_enclosure(&x)?.ok_or(not_proven(BoundFailure::NoEnclosure))?;
    if g.len() != rows.len() {
        return Err(CertError::Shape { field: "constraint_enclosure", expected: rows.len(), got: g.len() });
    }
    let fe = problem.objective_gradient_enclosure(&x)?.ok_or(not_proven(BoundFailure::NoEnclosure))?;
    if fe.len() != n {
        return Err(CertError::Shape { field: "objective_gradient_enclosure", expected: n, got: fe.len() });
    }
    let ge = problem.constraint_gradient_enclosure(set, &x)?.ok_or(not_proven(BoundFailure::NoEnclosure))?;
    if ge.len() != n * n {
        return Err(CertError::Shape { field: "constraint_gradient_enclosure", expected: n * n, got: ge.len() });
    }

    // 4. Primal feasibility over every row.
    let thr_p = threshold(tol.tol_p, data_scale)?;
    let (mut plo, mut phi) = (f64::NEG_INFINITY, f64::NEG_INFINITY);
    for gi in &g {
        plo = total_max(plo, gi.lo())?;
        phi = total_max(phi, gi.hi())?;
    }
    let primal = Condition::decide(plo, phi, thr_p);

    // 5. Complementarity over the active rows.
    let thr_c = threshold(tol.tol_c, data_scale)?;
    let (mut clo, mut chi) = (0.0_f64, 0.0_f64);
    for &i in &active {
        let gi = g.get(i).copied().ok_or(CertError::InvalidInput { field: "set" })?;
        clo = total_max(clo, mig(gi))?;
        chi = total_max(chi, gi.mag())?;
    }
    let complementarity = Condition::decide(clo, chi, thr_c);

    // 6. Multipliers from the float gradients at the recomputed point.
    let grad = problem.objective_gradient(&rec.params)?;
    check_vec("objective_gradient", &grad, n)?;
    let gm = problem.constraint_gradients(set, &rec.params)?;
    if gm.rows() != n || gm.cols() != n {
        return Err(CertError::Shape { field: "constraint_gradients", expected: n * n, got: gm.rows() * gm.cols() });
    }
    let gnorm = red::norm2(&grad)?;
    let scale_gradient = if gnorm == 0.0 { 1.0 } else { gnorm };
    let thr_s = threshold(tol.tol_s, scale_gradient)?;
    let a = gm.transpose()?;
    let q = qr_pivoted(&a, tol.rank_tol).map_err(la_err)?;

    let mut out = KktReport {
        multipliers: Vec::new(),
        multiplier_bounds: Vec::new(),
        dual_thresholds: Vec::new(),
        primal,
        dual: Condition::unknown(0.0),
        stationarity: Condition::unknown(thr_s),
        complementarity,
        tolerances: *tol,
        scales: KktScales { data: data_scale, gradient_lo: 0.0, gradient: scale_gradient },
        status: KktStatus::Undecidable,
        reason: None,
    };
    let mut special = None;
    if q.diag().rank < n {
        out.dual = Condition::unknown(0.0);
        special = Some(KktFail::DegenerateGradients);
    } else {
        let neg: Vec<f64> = grad.iter().map(|v| -v).collect();
        let lambda = solve_factored(&q, &neg, n, tol.rank_tol)?;
        check_vec("multipliers", &lambda, n)?;
        // Per-row dual thresholds, rounded down: tol_d * lo||grad f(X)|| / hi||grad g_i(X)||.
        let (flo, _) = norm_bounds(&fe)?;
        out.scales.gradient_lo = flo;
        let num = down(tol.tol_d * flo);
        let mut thr_d = Vec::with_capacity(n);
        for i in 0..n {
            let row = ge.get(i * n..(i + 1) * n).ok_or(not_proven(BoundFailure::NoEnclosure))?;
            let (_, ghi) = norm_bounds(row)?;
            // The upper norm bound is rounded up and never 0, so test the entries themselves.
            if row.iter().all(|e| e.mag() == 0.0) {
                return Err(CertError::InvalidInput { field: "constraint_gradient_zero" });
            }
            let t = if num > 0.0 { down(num / ghi) } else { 0.0 };
            if !t.is_finite() {
                return Err(CertError::InvalidInput { field: "dual_threshold" });
            }
            thr_d.push(t);
        }
        let mut thr_min = f64::INFINITY;
        for &t in &thr_d {
            thr_min = total_min(thr_min, t)?;
        }

        // [A] = [G(X)]^T (row i = parameter, column j = active row), row-major n x n.
        let mut ai = Vec::with_capacity(n * n);
        for i in 0..n {
            for j in 0..n {
                ai.push(ge.get(j * n + i).copied().ok_or(not_proven(BoundFailure::NoEnclosure))?);
            }
        }
        // r = [grad f(X)] + [A] lambda: encloses every residual of the interval system at lambda.
        let mut r = Vec::with_capacity(n);
        for (i, fi) in fe.iter().enumerate() {
            let mut acc = *fi;
            for (j, &lj) in lambda.iter().enumerate() {
                let aij = ai.get(i * n + j).copied().ok_or(not_proven(BoundFailure::NoEnclosure))?;
                acc = acc.add(aij.mul(Interval::point(lj)?)?)?;
            }
            r.push(acc);
        }

        // 8. Stationarity at the float multipliers.
        let (slo, shi) = norm_bounds(&r)?;
        out.stationarity = Condition::decide(slo, shi, thr_s);

        // 7. Proven multiplier box and dual feasibility.
        let y = approximate_inverse(&q, tol.rank_tol)?;
        match krawczyk(&lambda, &r, &y, |_| Ok(ai.clone())) {
            Ok(k) => {
                let lb = boxed(&lambda, &k)?;
                let (mut min_lo, mut min_hi) = (f64::INFINITY, f64::INFINITY);
                for l in &lb {
                    min_lo = total_min(min_lo, l.lo())?;
                    min_hi = total_min(min_hi, l.hi())?;
                }
                let dlo = total_max(0.0, -min_hi)?;
                let dhi = total_max(0.0, -min_lo)?;
                let (mut any_violated, mut all_satisfied) = (false, true);
                for (l, &t) in lb.iter().zip(&thr_d) {
                    if l.hi() < -t {
                        any_violated = true;
                    }
                    if l.lo() < -t {
                        all_satisfied = false;
                    }
                }
                let verdict = if any_violated {
                    Verdict::Violated
                } else if all_satisfied {
                    Verdict::Satisfied
                } else {
                    Verdict::Undecidable
                };
                out.dual = Condition { lo: dlo, hi: dhi, threshold: thr_min, verdict };
                out.multiplier_bounds = k;
            }
            Err(CertError::BoundNotProven { reason: BoundFailure::NotContracting }) => {
                out.dual = Condition::unknown(thr_min);
                out.multiplier_bounds = vec![f64::INFINITY; n];
                special = Some(KktFail::MultipliersNotProven);
            }
            Err(e) => return Err(e),
        }
        out.multipliers = lambda;
        out.dual_thresholds = thr_d;
    }

    let conds = [
        (out.primal.verdict, KktFail::Primal),
        (out.dual.verdict, KktFail::Dual),
        (out.stationarity.verdict, KktFail::Stationarity),
        (out.complementarity.verdict, KktFail::Complementarity),
    ];
    let first = |v: Verdict| conds.iter().find(|(cv, _)| *cv == v).map(|(_, k)| *k);
    if let Some(k) = first(Verdict::Violated) {
        out.status = KktStatus::Violated;
        out.reason = Some(k);
    } else if let Some(k) = first(Verdict::Undecidable) {
        out.status = KktStatus::Undecidable;
        out.reason = Some(special.unwrap_or(k));
    } else {
        out.status = KktStatus::Certified;
        out.reason = None;
    }
    Ok(out)
}

/// `tol * scale` (one rounding); not finite is an input error (never a fail-open threshold).
fn threshold(tol: f64, scale: f64) -> Result<f64, CertError> {
    let t = tol * scale;
    if t.is_finite() {
        Ok(t)
    } else {
        Err(CertError::InvalidInput { field: "tolerances" })
    }
}

/// `x` rounded down to a lower bound of the exact real it approximates (one rounding), clamped
/// at 0 for a nonnegative quantity.
fn down(x: f64) -> f64 {
    let d = next_down(x);
    if d < 0.0 {
        0.0
    } else {
        d
    }
}

/// A lower bound of `|x|` over the interval (exact: 0 if it contains 0, else the smaller end
/// magnitude).
fn mig(x: Interval) -> f64 {
    if x.contains(0.0) {
        0.0
    } else {
        let (a, b) = (x.lo().abs(), x.hi().abs());
        if a < b {
            a
        } else {
            b
        }
    }
}

/// Proven lower and upper bounds of the 2-norm of every real vector in the box `r`.
fn norm_bounds(r: &[Interval]) -> Result<(f64, f64), CertError> {
    let (mut lo2, mut hi2) = (0.0_f64, 0.0_f64);
    for ri in r {
        let (m, g) = (mig(*ri), ri.mag());
        let sq_lo = next_down(m * m);
        lo2 = next_down(lo2 + if sq_lo < 0.0 { 0.0 } else { sq_lo });
        if lo2 < 0.0 {
            lo2 = 0.0;
        }
        hi2 = add_up(hi2, mul_up(g, g)?)?;
    }
    let lo = next_down(f::sqrt(lo2)?);
    let hi = next_up(f::sqrt(hi2)?);
    if !hi.is_finite() {
        return Err(not_proven(BoundFailure::Overflow));
    }
    Ok((if lo < 0.0 { 0.0 } else { lo }, hi))
}

/// Positions in `rows` of the labels of `set`, after checking that the row labels are unique
/// and that the set is drawn from them.
fn active_indices(rows: &[RowLabel], set: &[RowLabel]) -> Result<Vec<usize>, CertError> {
    let mut sorted: Vec<(RowLabel, usize)> = rows.iter().copied().zip(0..).collect();
    sorted.sort_unstable();
    for w in sorted.windows(2) {
        if let [(a, _), (b, _)] = w {
            if a == b {
                return Err(CertError::DuplicateLabel { label: *a });
            }
        }
    }
    let mut out = Vec::with_capacity(set.len());
    for l in set {
        let pos = sorted.binary_search_by(|(r, _)| r.cmp(l)).map_err(|_| CertError::InvalidInput { field: "set" })?;
        let idx = sorted.get(pos).map(|(_, i)| *i).ok_or(CertError::InvalidInput { field: "set" })?;
        out.push(idx);
    }
    Ok(out)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn verdicts_never_round_a_borderline_into_a_pass() {
        assert_eq!(Condition::decide(0.0, 1.0, 1.0).verdict(), Verdict::Satisfied);
        assert_eq!(Condition::decide(0.5, 1.5, 1.0).verdict(), Verdict::Undecidable);
        assert_eq!(Condition::decide(1.0, 1.5, 1.0).verdict(), Verdict::Undecidable);
        assert_eq!(Condition::decide(1.5, 2.0, 1.0).verdict(), Verdict::Violated);
        assert_eq!(Condition::unknown(1.0).verdict(), Verdict::Undecidable);
    }

    #[test]
    fn norm_bounds_enclose() {
        let iv = |a, b| Interval::new(a, b).unwrap();
        let (lo, hi) = norm_bounds(&[iv(3.0, 3.0), iv(4.0, 4.0)]).unwrap();
        assert!(lo <= 5.0 && 5.0 <= hi && hi - lo < 1e-14);
        let (lo, hi) = norm_bounds(&[iv(-1.0, 2.0), iv(0.0, 0.0)]).unwrap();
        assert_eq!(lo, 0.0);
        assert!(hi >= 2.0);
        assert_eq!(mig(iv(-3.0, -2.0)), 2.0);
        assert_eq!(mig(iv(-3.0, 2.0)), 0.0);
    }

    #[test]
    fn tolerance_checks() {
        assert!(KktTolerances::default().check().is_ok());
        let bad = KktTolerances { tol_d: -1.0, ..KktTolerances::default() };
        assert_eq!(bad.check(), Err(CertError::InvalidInput { field: "tolerances" }));
        let bad = KktTolerances { rank_tol: 1.0, ..KktTolerances::default() };
        assert!(bad.check().is_err());
        assert!(threshold(f64::MAX, 2.0).is_err());
    }
}
