//! `ActiveSetProblem` callback and deterministic recomputation (spec node
//! KRN.SOLV.CERT.RECOMP).
//!
//! [`recompute`] takes one candidate of an uncertified [`Proposal`] and computes the optimum
//! that the candidate defines **from the problem data alone**: the caller's
//! [`ActiveSetProblem`] turns the set into its defining equations, and this module solves them
//! by a fixed procedure. Its inputs are the sorted candidate set, the problem and the
//! [`RecompConfig`] constants; it never reads a conic solution or a solver iterate (a
//! `Proposal` holds neither: only labels, thresholds and scales), so two solver paths that
//! reach the same set give bit-identical results (REQ-KRN.SOLV.CERT.RECOMP-001, -005).
//!
//! The result is a [`Recomputed`]: still uncertified. Only `certify` (`KRN.SOLV.CERT.CT`),
//! after the KKT check (`KRN.SOLV.CERT.KKT`), turns a candidate into a certified active set.
//!
//! # Procedure (pinned)
//!
//! 1. **Checks**, in this order: the config (`InvalidInput { field: "config" }`: `rel_tol` and
//!    `cond_max` finite, `0 <= rel_tol < 1`, `cond_max >= 1`, `gn_steps >= 1`); the candidate
//!    index (`InvalidInput { field: "candidate" }`); `n_params() >= 1` and
//!    `expected_active() >= n_params()` (`InvalidInput`); the problem's row labels unique
//!    (`DuplicateLabel`); the set, sorted ascending, of length `expected_active()` (`Shape`),
//!    duplicate-free (`DuplicateLabel`) and drawn from the row labels
//!    (`InvalidInput { field: "set" }`). Every callback output is checked for length (`Shape`)
//!    and finiteness (`InvalidInput`). The callbacks always receive the **sorted** set, so the
//!    order in which a conic solution listed its rows cannot reach the arithmetic.
//! 2. **Linear** (`is_linear()`): `(r0, J) = residuals_and_jacobian(set, 0)`. The system is
//!    `J p + r0 = 0`, solved once by `fid_num::la::lstsq(J, -r0, rel_tol)` (column-pivoted
//!    Householder QR). Rank below `n_params` is [`CertError::RankDeficient`]. `steps = 1`.
//! 3. **Nonlinear**: `p = start_from_set(set)`, then exactly `gn_steps` Gauss-Newton steps
//!    `p_j <- p_j - x_j` with `x = lstsq(J(p), r(p), rel_tol).x` (no step control, no early
//!    exit), then `J` at the final `p` is factored by `qr_pivoted(J, rel_tol)` (rank as above).
//!    `steps = gn_steps`.
//! 4. **Condition**: the condition estimate of the final factorization (`Factorization::cond`,
//!    `+inf` beyond range) above `cond_max` is [`CertError::IllConditioned`], and no value.
//! 5. **Convergence** (nonlinear only): with `s` = `red::norm2` of the last step and
//!    `thr = (4 u) * red::norm2(p)` (`u = 2^-53`, one rounding), `s > thr` is
//!    [`CertError::RecomputeNoConvergence`].
//! 6. **Proven forward error bound** (below); then `value = value(p)`.
//!
//! # Proven forward error bound (fail closed)
//!
//! Every [`Recomputed`] carries `param_bounds` and `value_bound` with
//! `|params[i] - p*_i| <= param_bounds[i]` and `|value - value(p*)| <= value_bound`, where `p*`
//! is the exact (real-arithmetic) root of the active-set equations, proven unique in the box
//! `params +- param_bounds`. The proof is a Krawczyk test in outward-rounded interval
//! arithmetic ([`Interval`]), valid under IEEE-754 round-to-nearest without FMA:
//!
//! - `Y` is an approximate inverse of the final `J` (columns solved from the same pivoted QR).
//! - `[f]` encloses the exact residuals at `p`. Linear: `J p + r0` evaluated in intervals from
//!   the exact data `J`, `r0` (the contract of a linear problem is that the Jacobian and the
//!   residuals it returns at `p = 0` are the system's exact coefficients, so the exact residual
//!   at any `p` is `J p + r0`). Nonlinear: [`ActiveSetProblem::residual_enclosure`].
//! - `z = Y [f]`; `rho_i = 2 |z_i| + floor`, `floor = 2^-50 max_j |p_j| + 2^-1022`
//!   (outward). For at most [`KRAWCZYK_ATTEMPTS`] attempts: `X = p +- rho` (outward);
//!   `[J(X)]` encloses the exact Jacobian over `X` (linear: `J` itself; nonlinear:
//!   [`ActiveSetProblem::jacobian_enclosure`]); `C = I - Y [J(X)]`;
//!   `k_i = |z_i| + sum_j |C_ij| d_j` with `d_j >= |x_j - p_j|` over `X`, every operation
//!   rounded up. If `k_i < rho_i` for every `i`, the Krawczyk operator maps `X` into its
//!   interior, so `X` holds exactly one root `p*`, it lies in `p +- k`, and
//!   `param_bounds = k`. Otherwise `rho_i = 2 k_i + floor` and the next attempt runs.
//! - `value_bound` is the largest distance from `value` to
//!   [`ActiveSetProblem::value_enclosure`] over the box `p +- param_bounds`.
//!
//! Without a proof there is no result: a non-square system (`expected_active > n_params`), a
//! missing enclosure, an overflow or a test that does not contract is
//! [`CertError::BoundNotProven`].
//!
//! # Determinism
//!
//! Single thread, no FMA, no `f64::min`/`max` (total-order helpers of `fid_num::f`), no hash
//! containers; every factorization, solve and norm goes through `fid_num::la` and
//! `fid_num::red`, whose results are bit-identical on x86-64, ARM64 and wasm32. The interval
//! arithmetic uses only IEEE `+ - * /`, `abs` and `fid_num::f::sqrt` with a fixed operation
//! order.

mod interval;

pub use interval::Interval;

use fid_num::f::total_max;
use fid_num::la::{lstsq, qr_pivoted, solve_upper, LaError, Mat, Qr};
use fid_num::{red, NumError};

use interval::{add_up, mul_up, next_down, next_up};

use super::active::Proposal;
use super::{BoundFailure, CertError, RowLabel};

/// The unit roundoff `u = 2^-53` of IEEE-754 binary64.
pub const UNIT_ROUNDOFF: f64 = f64::EPSILON / 2.0;

/// Default number of Gauss-Newton steps (REQ-KRN.SOLV.CERT.RECOMP-003, [unverified]).
pub const DEFAULT_GN_STEPS: u32 = 8;

/// Default condition threshold (REQ-KRN.SOLV.CERT.RECOMP-004, [unverified]).
pub const DEFAULT_COND_MAX: f64 = 1e12;

/// Default relative rank tolerance of the QR factorizations: `2^-50` (8 u). Below `1 /
/// DEFAULT_COND_MAX` by a wide margin, so a system with condition near `1e14` is reported as
/// ill-conditioned, not rank deficient.
pub const DEFAULT_REL_TOL: f64 = 1.0 / 1_125_899_906_842_624.0;

/// Gauss-Newton convergence factor: the last step may be at most `4 u` times the parameter
/// norm (REQ-KRN.SOLV.CERT.RECOMP-003).
pub const STEP_TOL: f64 = 4.0 * UNIT_ROUNDOFF;

/// Krawczyk attempts (epsilon inflations) before the bound is declared not proven.
pub const KRAWCZYK_ATTEMPTS: u32 = 4;

/// `2^-50`: relative floor of the Krawczyk box radius.
const FLOOR_REL: f64 = DEFAULT_REL_TOL;

/// Constants of the recomputation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RecompConfig {
    /// Relative rank tolerance of every QR factorization (`0 <= rel_tol < 1`).
    pub rel_tol: f64,
    /// Largest accepted condition estimate of the active-set system (`>= 1`).
    pub cond_max: f64,
    /// Number of Gauss-Newton steps for a nonlinear system (`>= 1`).
    pub gn_steps: u32,
}

impl Default for RecompConfig {
    fn default() -> RecompConfig {
        RecompConfig { rel_tol: DEFAULT_REL_TOL, cond_max: DEFAULT_COND_MAX, gn_steps: DEFAULT_GN_STEPS }
    }
}

impl RecompConfig {
    fn check(&self) -> Result<(), CertError> {
        let ok = self.rel_tol.is_finite()
            && (0.0..1.0).contains(&self.rel_tol)
            && self.cond_max.is_finite()
            && self.cond_max >= 1.0
            && self.gn_steps >= 1;
        if ok {
            Ok(())
        } else {
            Err(CertError::InvalidInput { field: "config" })
        }
    }
}

/// A primitive's active-set equations, implemented by the caller (`KRN.MZ`, `KRN.ENV`,
/// `GDT.ZONE.POSE.LIN`, and test problems). Row labels are the same [`RowLabel`]s given to
/// [`super::active::extract`]. Every method that takes a set receives it sorted ascending.
pub trait ActiveSetProblem {
    /// Number of parameters `n` (at least 1).
    fn n_params(&self) -> usize;

    /// Size of an active set (at least `n_params`; equal to it for a proven bound).
    fn expected_active(&self) -> usize;

    /// Labels of every constraint row of the problem (unique).
    fn row_labels(&self) -> Vec<RowLabel>;

    /// Whether the active-set equations are linear in the parameters. For a linear problem the
    /// Jacobian and the residuals returned at `p = 0` must be the exact coefficients of the
    /// system (`r(p) = J p + r(0)` in real arithmetic); the error bound rests on this.
    fn is_linear(&self) -> bool;

    /// The Gauss-Newton start of a nonlinear problem, derived from the set and the problem
    /// data only (never from a solver iterate). Not called for a linear problem.
    fn start_from_set(&self, set: &[RowLabel]) -> Result<Vec<f64>, CertError>;

    /// Residuals (one per set row, in set order) and Jacobian (`set.len() x n_params`) of the
    /// active-set equations at `p`.
    fn residuals_and_jacobian(&self, set: &[RowLabel], p: &[f64]) -> Result<(Vec<f64>, Mat), CertError>;

    /// The objective value at `p` (for example the zone width).
    fn value(&self, p: &[f64]) -> Result<f64, CertError>;

    /// Every constraint value `g_i(p)` (feasible when `<= 0`), in [`Self::row_labels`] order.
    /// Used by `KRN.SOLV.CERT.KKT`.
    fn constraint_values(&self, p: &[f64]) -> Result<Vec<f64>, CertError>;

    /// The objective gradient at `p`. Used by `KRN.SOLV.CERT.KKT`.
    fn objective_gradient(&self, p: &[f64]) -> Result<Vec<f64>, CertError>;

    /// Gradients of the set's constraints at `p` (`set.len() x n_params`). Used by
    /// `KRN.SOLV.CERT.KKT`.
    fn constraint_gradients(&self, set: &[RowLabel], p: &[f64]) -> Result<Mat, CertError>;

    /// An enclosure of the exact (real-arithmetic) residuals of the set's equations at the
    /// float point `p`. Needed for the bound of a nonlinear problem; `None` (the default)
    /// makes the recomputation fail closed.
    fn residual_enclosure(&self, set: &[RowLabel], p: &[f64]) -> Result<Option<Vec<Interval>>, CertError> {
        let _ = (set, p);
        Ok(None)
    }

    /// An enclosure, row-major `set.len() x n_params`, of the exact Jacobian of the set's
    /// equations at every point of the box `b`. Needed for the bound of a nonlinear problem;
    /// `None` (the default) makes the recomputation fail closed.
    fn jacobian_enclosure(&self, set: &[RowLabel], b: &[Interval]) -> Result<Option<Vec<Interval>>, CertError> {
        let _ = (set, b);
        Ok(None)
    }

    /// An enclosure of the exact objective value at every point of the box `b`. Needed for
    /// every bound; `None` (the default) makes the recomputation fail closed.
    fn value_enclosure(&self, b: &[Interval]) -> Result<Option<Interval>, CertError> {
        let _ = b;
        Ok(None)
    }
}

/// The optimum recomputed on one candidate set, with its proven forward error bound. Still
/// **uncertified**: the KKT check and `certify` decide whether the set is the active set.
/// Made only by [`recompute`].
#[derive(Debug, Clone, PartialEq)]
pub struct Recomputed {
    pub(crate) set: Vec<RowLabel>,
    pub(crate) params: Vec<f64>,
    pub(crate) value: f64,
    pub(crate) cond: f64,
    pub(crate) steps: u32,
    pub(crate) param_bounds: Vec<f64>,
    pub(crate) value_bound: f64,
}

impl Recomputed {
    /// The candidate set, sorted ascending.
    pub fn set(&self) -> &[RowLabel] {
        &self.set
    }
    /// The recomputed parameters.
    pub fn params(&self) -> &[f64] {
        &self.params
    }
    /// The objective value at [`Self::params`].
    pub fn value(&self) -> f64 {
        self.value
    }
    /// Condition estimate of the final active-set system.
    pub fn cond(&self) -> f64 {
        self.cond
    }
    /// QR solves performed: 1 for a linear system, `gn_steps` for a nonlinear one.
    pub fn steps(&self) -> u32 {
        self.steps
    }
    /// Proven bounds: `|params[i] - p*_i| <= param_bounds[i]` for the unique exact root `p*`
    /// of the active-set equations in the box `params +- param_bounds`.
    pub fn param_bounds(&self) -> &[f64] {
        &self.param_bounds
    }
    /// Proven bound: `|value - value(p*)| <= value_bound`.
    pub fn value_bound(&self) -> f64 {
        self.value_bound
    }
}

/// Recomputes the optimum on candidate `candidate` (an index into
/// [`Proposal::uncertified_sets`]) of `proposal`, from `problem`'s data and `cfg` only. See the
/// module docs for the pinned procedure and the proven bound.
///
/// # Errors
///
/// Input errors (`InvalidInput`, `Shape`, `DuplicateLabel`), [`CertError::RankDeficient`],
/// [`CertError::IllConditioned`], [`CertError::RecomputeNoConvergence`],
/// [`CertError::BoundNotProven`], [`CertError::Numeric`], and any error a callback returns.
pub fn recompute<P: ActiveSetProblem + ?Sized>(
    problem: &P,
    proposal: &Proposal,
    candidate: usize,
    cfg: &RecompConfig,
) -> Result<Recomputed, CertError> {
    cfg.check()?;
    let set = proposal.sets.get(candidate).ok_or(CertError::InvalidInput { field: "candidate" })?;
    recompute_set(problem, set, cfg)
}

/// [`recompute`] on a set given directly (crate-internal: outside the crate a set reaches the
/// recomputation only as a candidate of a [`Proposal`]).
pub(crate) fn recompute_set<P: ActiveSetProblem + ?Sized>(
    problem: &P,
    set: &[RowLabel],
    cfg: &RecompConfig,
) -> Result<Recomputed, CertError> {
    cfg.check()?;
    let n = problem.n_params();
    let k = problem.expected_active();
    if n == 0 {
        return Err(CertError::InvalidInput { field: "n_params" });
    }
    if k < n {
        return Err(CertError::InvalidInput { field: "expected_active" });
    }
    let set = checked_set(problem, set, k)?;

    let (params, qr, steps, last_step) = if problem.is_linear() {
        let zeros = vec![0.0; n];
        let (r0, j) = system(problem, &set, &zeros, n)?;
        let neg: Vec<f64> = r0.iter().map(|v| -v).collect();
        let x = lstsq(&j, &neg, cfg.rel_tol).map_err(la_err)?.x;
        let q = qr_pivoted(&j, cfg.rel_tol).map_err(la_err)?;
        (x, (q, j, r0), 1, None)
    } else {
        let mut p = problem.start_from_set(&set)?;
        check_vec("start", &p, n)?;
        let mut step = Vec::new();
        for _ in 0..cfg.gn_steps {
            let (r, j) = system(problem, &set, &p, n)?;
            step = lstsq(&j, &r, cfg.rel_tol).map_err(la_err)?.x;
            for (pi, xi) in p.iter_mut().zip(&step) {
                *pi -= xi;
            }
            check_vec("params", &p, n)?;
        }
        let (r, j) = system(problem, &set, &p, n)?;
        let q = qr_pivoted(&j, cfg.rel_tol).map_err(la_err)?;
        (p, (q, j, r), cfg.gn_steps, Some(step))
    };
    let (q, j, r) = qr;
    let diag = q.diag();
    if diag.rank < n {
        return Err(CertError::RankDeficient { rank: diag.rank });
    }
    if diag.cond > cfg.cond_max || diag.cond.is_nan() {
        return Err(CertError::IllConditioned { cond: diag.cond });
    }
    if let Some(step) = last_step {
        let step_norm = red::norm2(&step)?;
        let thr = STEP_TOL * red::norm2(&params)?;
        if step_norm > thr {
            return Err(CertError::RecomputeNoConvergence { step_norm });
        }
    }

    if k != n {
        return Err(not_proven(BoundFailure::NotSquare));
    }
    let y = approximate_inverse(&q, cfg.rel_tol)?;
    let param_bounds = if problem.is_linear() {
        let f = linear_residual_enclosure(&j, &r, &params)?;
        let jp = points(j.data())?;
        krawczyk(&params, &f, &y, |_| Ok(jp.clone()))?
    } else {
        let f = problem.residual_enclosure(&set, &params)?.ok_or(not_proven(BoundFailure::NoEnclosure))?;
        if f.len() != k {
            return Err(CertError::Shape { field: "residual_enclosure", expected: k, got: f.len() });
        }
        krawczyk(&params, &f, &y, |b| {
            let e = problem.jacobian_enclosure(&set, b)?.ok_or(not_proven(BoundFailure::NoEnclosure))?;
            if e.len() != k * n {
                return Err(CertError::Shape { field: "jacobian_enclosure", expected: k * n, got: e.len() });
            }
            Ok(e)
        })?
    };

    let value = problem.value(&params)?;
    if !value.is_finite() {
        return Err(CertError::InvalidInput { field: "value" });
    }
    let bx = boxed(&params, &param_bounds)?;
    let enc = problem.value_enclosure(&bx)?.ok_or(not_proven(BoundFailure::NoEnclosure))?;
    let value_bound = enc.dist_from(value)?;
    Ok(Recomputed { set, params, value, cond: diag.cond, steps, param_bounds, value_bound })
}

fn not_proven(reason: BoundFailure) -> CertError {
    CertError::BoundNotProven { reason }
}

/// `lstsq`'s rank deficiency becomes [`CertError::RankDeficient`]; anything else is numeric.
fn la_err(e: NumError) -> CertError {
    match LaError::of(&e) {
        Some(LaError::RankDeficient { rank }) => CertError::RankDeficient { rank },
        _ => CertError::Numeric(e),
    }
}

/// The set sorted ascending, after the checks of the module docs (step 1).
fn checked_set<P: ActiveSetProblem + ?Sized>(
    problem: &P,
    set: &[RowLabel],
    k: usize,
) -> Result<Vec<RowLabel>, CertError> {
    let mut rows = problem.row_labels();
    rows.sort_unstable();
    if let Some(label) = first_duplicate(&rows) {
        return Err(CertError::DuplicateLabel { label });
    }
    let mut set = set.to_vec();
    set.sort_unstable();
    if set.len() != k {
        return Err(CertError::Shape { field: "set", expected: k, got: set.len() });
    }
    if let Some(label) = first_duplicate(&set) {
        return Err(CertError::DuplicateLabel { label });
    }
    if !set.iter().all(|l| rows.binary_search(l).is_ok()) {
        return Err(CertError::InvalidInput { field: "set" });
    }
    Ok(set)
}

fn first_duplicate(sorted: &[RowLabel]) -> Option<RowLabel> {
    sorted.windows(2).find_map(|w| match w {
        [a, b] if a == b => Some(*a),
        _ => None,
    })
}

fn check_vec(field: &'static str, v: &[f64], len: usize) -> Result<(), CertError> {
    if v.len() != len {
        return Err(CertError::Shape { field, expected: len, got: v.len() });
    }
    if !v.iter().all(|x| x.is_finite()) {
        return Err(CertError::InvalidInput { field });
    }
    Ok(())
}

/// Residuals and Jacobian at `p`, checked for shape and finiteness.
fn system<P: ActiveSetProblem + ?Sized>(
    problem: &P,
    set: &[RowLabel],
    p: &[f64],
    n: usize,
) -> Result<(Vec<f64>, Mat), CertError> {
    let (r, j) = problem.residuals_and_jacobian(set, p)?;
    check_vec("residuals", &r, set.len())?;
    if j.rows() != set.len() {
        return Err(CertError::Shape { field: "jacobian", expected: set.len(), got: j.rows() });
    }
    if j.cols() != n {
        return Err(CertError::Shape { field: "jacobian_cols", expected: n, got: j.cols() });
    }
    Ok((r, j))
}

/// `Y ~ J^-1` (row-major `n x n`), column `c` solved from the pivoted factor: `Q^T e_c`, back
/// substitution, un-permutation.
fn approximate_inverse(q: &Qr, rel_tol: f64) -> Result<Vec<f64>, CertError> {
    let n = q.cols();
    let r = q.r()?;
    let mut y = vec![0.0; n * n];
    let mut e = vec![0.0; n];
    for c in 0..n {
        for (i, ei) in e.iter_mut().enumerate() {
            *ei = if i == c { 1.0 } else { 0.0 };
        }
        let t = q.apply_qt(&e)?;
        let z = solve_upper(&r, t.get(..n).unwrap_or(&[]), rel_tol)?;
        for (&pi, &zi) in q.perm().iter().zip(&z) {
            if let Some(slot) = y.get_mut(pi * n + c) {
                *slot = zi;
            }
        }
    }
    Ok(y)
}

fn points(xs: &[f64]) -> Result<Vec<Interval>, CertError> {
    xs.iter().map(|&x| Interval::point(x)).collect()
}

/// The exact residuals `J p + r0` of a linear system, enclosed (sum in ascending column order,
/// then `+ r0`).
fn linear_residual_enclosure(j: &Mat, r0: &[f64], p: &[f64]) -> Result<Vec<Interval>, CertError> {
    let n = j.cols();
    let mut out = Vec::with_capacity(r0.len());
    for (i, &ri) in r0.iter().enumerate() {
        let row = j.row(i)?;
        let mut acc = Interval::point(ri)?;
        for (&a, &x) in row.iter().zip(p).take(n) {
            acc = acc.add(Interval::point(a)?.mul(Interval::point(x)?)?)?;
        }
        out.push(acc);
    }
    Ok(out)
}

/// The box `p +- rad`, rounded outward.
fn boxed(p: &[f64], rad: &[f64]) -> Result<Vec<Interval>, CertError> {
    p.iter()
        .zip(rad)
        .map(|(&c, &r)| {
            let (lo, hi) = (next_down(c - r), next_up(c + r));
            if lo.is_finite() && hi.is_finite() {
                Interval::new(lo, hi)
            } else {
                Err(not_proven(BoundFailure::Overflow))
            }
        })
        .collect()
}

/// The Krawczyk test of the module docs. Returns `k` (the proven per-parameter bounds).
fn krawczyk<F>(p: &[f64], f: &[Interval], y: &[f64], mut jac: F) -> Result<Vec<f64>, CertError>
where
    F: FnMut(&[Interval]) -> Result<Vec<Interval>, CertError>,
{
    let n = p.len();
    // z = Y [f] and its magnitudes.
    let mut zmag = Vec::with_capacity(n);
    for i in 0..n {
        let mut acc = Interval::point(0.0)?;
        for (jj, fj) in f.iter().enumerate().take(n) {
            let yij = y.get(i * n + jj).copied().unwrap_or(0.0);
            acc = acc.add(Interval::point(yij)?.mul(*fj)?)?;
        }
        zmag.push(acc.mag());
    }
    let mut pmax = 0.0_f64;
    for &v in p {
        pmax = total_max(pmax, v.abs())?;
    }
    let floor = add_up(mul_up(pmax, FLOOR_REL)?, f64::MIN_POSITIVE)?;
    let mut rho = Vec::with_capacity(n);
    for &zm in &zmag {
        rho.push(add_up(mul_up(zm, 2.0)?, floor)?);
    }
    for _ in 0..KRAWCZYK_ATTEMPTS {
        let bx = boxed(p, &rho)?;
        let mut d = Vec::with_capacity(n);
        for (b, &c) in bx.iter().zip(p) {
            d.push(b.dist_from(c)?);
        }
        let jx = jac(&bx)?;
        let mut k = Vec::with_capacity(n);
        for i in 0..n {
            let mut ki = zmag.get(i).copied().unwrap_or(0.0);
            for (col, &dj) in d.iter().enumerate() {
                // C_ij = delta_ij - sum_l Y_il [J]_lj
                let mut acc = Interval::point(if i == col { 1.0 } else { 0.0 })?;
                for l in 0..n {
                    let yil = y.get(i * n + l).copied().unwrap_or(0.0);
                    let jl = jx.get(l * n + col).copied().ok_or(not_proven(BoundFailure::NoEnclosure))?;
                    acc = acc.sub(Interval::point(yil)?.mul(jl)?)?;
                }
                ki = add_up(ki, mul_up(acc.mag(), dj)?)?;
            }
            k.push(ki);
        }
        if k.iter().zip(&rho).all(|(ki, ri)| ki < ri) {
            return Ok(k);
        }
        for (ri, &ki) in rho.iter_mut().zip(&k) {
            *ri = add_up(mul_up(ki, 2.0)?, floor)?;
        }
    }
    Err(not_proven(BoundFailure::NotContracting))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    /// A 2x2 linear system `p0 + p1 = 3`, `p0 - p1 = 1` (`p = (2, 1)`), rows labelled 0 and 1.
    struct Tiny {
        value_enc: bool,
    }

    impl ActiveSetProblem for Tiny {
        fn n_params(&self) -> usize {
            2
        }
        fn expected_active(&self) -> usize {
            2
        }
        fn row_labels(&self) -> Vec<RowLabel> {
            vec![RowLabel { point: 0, side: 0 }, RowLabel { point: 1, side: 0 }, RowLabel { point: 2, side: 0 }]
        }
        fn is_linear(&self) -> bool {
            true
        }
        fn start_from_set(&self, _: &[RowLabel]) -> Result<Vec<f64>, CertError> {
            Err(CertError::Callback { reason: "linear" })
        }
        fn residuals_and_jacobian(&self, _: &[RowLabel], p: &[f64]) -> Result<(Vec<f64>, Mat), CertError> {
            let j = Mat::from_rows(&[[1.0, 1.0], [1.0, -1.0]])?;
            Ok((vec![p[0] + p[1] - 3.0, p[0] - p[1] - 1.0], j))
        }
        fn value(&self, p: &[f64]) -> Result<f64, CertError> {
            Ok(p[0])
        }
        fn constraint_values(&self, _: &[f64]) -> Result<Vec<f64>, CertError> {
            Ok(vec![0.0; 3])
        }
        fn objective_gradient(&self, _: &[f64]) -> Result<Vec<f64>, CertError> {
            Ok(vec![1.0, 0.0])
        }
        fn constraint_gradients(&self, _: &[RowLabel], _: &[f64]) -> Result<Mat, CertError> {
            Ok(Mat::from_rows(&[[1.0, 1.0], [1.0, -1.0]])?)
        }
        fn value_enclosure(&self, b: &[Interval]) -> Result<Option<Interval>, CertError> {
            Ok(if self.value_enc { Some(b[0]) } else { None })
        }
    }

    fn l(point: u32) -> RowLabel {
        RowLabel { point, side: 0 }
    }

    #[test]
    fn tiny_linear_system() {
        let r = recompute_set(&Tiny { value_enc: true }, &[l(1), l(0)], &RecompConfig::default()).unwrap();
        assert_eq!(r.set(), &[l(0), l(1)]);
        // One QR solve is accurate to a few ulps (here 2 - 7e-16), inside the proven bound.
        for (g, w) in r.params().iter().zip([2.0, 1.0]) {
            assert!(r.param_bounds().iter().any(|&b| (g - w).abs() <= b), "{g}");
            assert!((g - w).abs() < 1e-15);
        }
        assert_eq!(r.steps(), 1);
        assert!(r.param_bounds().iter().all(|&b| b > 0.0 && b < 1e-13));
        assert!(r.value_bound() < 1e-13);
    }

    #[test]
    fn tiny_input_errors() {
        let cfg = RecompConfig::default();
        let p = Tiny { value_enc: true };
        assert_eq!(recompute_set(&p, &[l(0)], &cfg), Err(CertError::Shape { field: "set", expected: 2, got: 1 }));
        assert_eq!(recompute_set(&p, &[l(0), l(0)], &cfg), Err(CertError::DuplicateLabel { label: l(0) }));
        assert_eq!(recompute_set(&p, &[l(0), l(7)], &cfg), Err(CertError::InvalidInput { field: "set" }));
        let bad = RecompConfig { gn_steps: 0, ..cfg };
        assert_eq!(recompute_set(&p, &[l(0), l(1)], &bad), Err(CertError::InvalidInput { field: "config" }));
        let bad = RecompConfig { cond_max: f64::NAN, ..cfg };
        assert_eq!(recompute_set(&p, &[l(0), l(1)], &bad), Err(CertError::InvalidInput { field: "config" }));
        assert_eq!(
            recompute_set(&Tiny { value_enc: false }, &[l(0), l(1)], &cfg),
            Err(CertError::BoundNotProven { reason: BoundFailure::NoEnclosure })
        );
    }

    #[test]
    fn constants() {
        assert_eq!(DEFAULT_REL_TOL, 2f64.powi(-50));
        assert_eq!(STEP_TOL, 2f64.powi(-51));
        assert_eq!(UNIT_ROUNDOFF, 2f64.powi(-53));
    }
}
