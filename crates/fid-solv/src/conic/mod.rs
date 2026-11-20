//! Kernel conic API (spec node KRN.SOLV.CONIC.WRAP): the only door from the kernel into the
//! vendored interior-point solver `fid-clarabel`.
//!
//! Canonical form: minimise `1/2 x'Px + q'x` subject to `Ax + s = b`, `s in K`, with `K` the
//! product of zero, nonnegative and second-order cones in the listed order. A problem is built
//! with the checked [`ConicProblemBuilder`] and solved by [`solve_lp`], [`solve_qp`] or
//! [`solve_socp`], which check the problem class and run the solver once, single-threaded,
//! with the limits compiled into this crate ([`ConicLimits::FIXED`], exported by
//! [`limits_descriptor`]). Only `Solved` with all-finite outputs gives a [`ConicSolution`];
//! every other end is a typed [`ConicError`] that carries no solution. No vendored type
//! appears in this API, and only `fid-clarabel`'s public API is used.

mod limits;
mod problem;

pub use limits::{limits_descriptor, ConicLimits, LimitValue, LimitsDescriptor, LIMITS_NAME};
pub use problem::{Cone, ConicProblem, ConicProblemBuilder, Csc, ProblemClass};

use core::fmt;
use fid_clarabel::algebra::CscMatrix;
use fid_clarabel::qdldl::{MAX_ORDERING_DIM, MAX_ORDERING_NNZ};
use fid_clarabel::solver::{DefaultSolver, IPSolver, SolverError as SetupError, SolverStatus, SupportedConeT};

/// Why the solver ended without a solution (REQ-KRN.SOLV.CONIC.WRAP-005).
///
/// The cause is read from the solver's *final* status (`solution.status`), after the vendored
/// solver's own post-processing: a solved, almost solved or infeasibility status whose values
/// of record are not finite has already been demoted to a numerical error there (patch 0003),
/// and is reported here as [`ConicCause::Numerical`].
///
/// The `Almost*` causes can come from a run that hit the iteration limit, stopped for lack of
/// progress or failed numerically: on those ends the solver checks its last iterate against
/// the reduced-accuracy tolerances and, if they pass, reports `Almost*` instead. So an
/// iteration-limit run does not always give [`ConicCause::IterationLimit`]. Either way it is
/// an error here; nothing reduced-accuracy is returned as a solution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConicCause {
    /// The solver found a certificate of primal infeasibility.
    PrimalInfeasible,
    /// The solver found a certificate of dual infeasibility (the problem is unbounded).
    DualInfeasible,
    /// The iteration limit was reached and the last iterate did not meet even the
    /// reduced-accuracy tolerances (else the cause is one of the `Almost*` causes).
    IterationLimit,
    /// The solver stopped for lack of progress.
    InsufficientProgress,
    /// The solver stopped on a numerical error (including a non-finite iterate, and a result
    /// or certificate the solver demoted because a value of record was not finite).
    Numerical,
    /// The solver reached only its reduced-accuracy tolerances for a solution. This is also
    /// how a run that hit the iteration limit, stalled or failed numerically ends when its
    /// last iterate passes the reduced tolerances.
    AlmostSolved,
    /// The solver reached only its reduced-accuracy tolerances for primal infeasibility
    /// (also possible after the iteration limit, a stall or a numerical failure).
    AlmostPrimalInfeasible,
    /// The solver reached only its reduced-accuracy tolerances for dual infeasibility (also
    /// possible after the iteration limit, a stall or a numerical failure).
    AlmostDualInfeasible,
    /// The solver reported that it did not run.
    NotRun,
    /// The solver reported `Solved` but an output value is not finite.
    NonFiniteOutput,
}

/// A solve that ended without a solution: the cause and the last diagnostics. A residual or
/// gap is `None` when the solver's last value is not finite (KNP-1: no NaN leaves the API).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SolverError {
    /// Why the solve ended.
    pub cause: ConicCause,
    /// Interior-point iterations performed.
    pub iterations: u32,
    /// Last primal residual.
    pub primal_residual: Option<f64>,
    /// Last dual residual.
    pub dual_residual: Option<f64>,
    /// Last absolute duality gap.
    pub gap: Option<f64>,
}

/// Every way a conic solve can fail.
#[derive(Debug, Clone, PartialEq)]
pub enum ConicError {
    /// The builder rejected a field (`"q"`, `"A"`, `"b"`, `"cones"` or `"P"`).
    InvalidProblem {
        /// The first failing field.
        field: &'static str,
    },
    /// The problem is not of the entry point's class (REQ-KRN.SOLV.CONIC.WRAP-002).
    WrongProblemClass {
        /// The class of the entry point that was called.
        entry: ProblemClass,
        /// Whether the problem has a quadratic term.
        has_p: bool,
        /// Whether the problem has a second-order cone.
        has_soc: bool,
    },
    /// The KKT system exceeds the size budget of the vendored solver's minimum-degree LDL
    /// ordering (`MAX_ORDERING_DIM` columns or `MAX_ORDERING_NNZ` stored entries; see
    /// [`limits_descriptor`]). Checked before the solver is set up.
    TooLarge {
        /// KKT dimension bound.
        kkt_dim: usize,
        /// Bound on the stored entries of the KKT upper triangle.
        kkt_nnz: usize,
    },
    /// The LDL factorisation could not be set up (the ordering's fill budget was exceeded or
    /// the logical factorisation failed); the solver was not run.
    FactorisationSetup,
    /// The vendored solver refused the problem at setup for a reason the builder does not
    /// check (none is known; kept so that such a refusal fails closed).
    SetupRejected {
        /// The solver's reason.
        reason: &'static str,
    },
    /// The solver ran and ended without a solution.
    Solver(SolverError),
}

impl fmt::Display for ConicError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConicError::InvalidProblem { field } => write!(f, "conic: invalid problem field {field}"),
            ConicError::WrongProblemClass { entry, has_p, has_soc } => {
                write!(f, "conic: wrong problem class for {entry:?} (P: {has_p}, second-order cone: {has_soc})")
            }
            ConicError::TooLarge { kkt_dim, kkt_nnz } => {
                write!(f, "conic: KKT system too large for the LDL ordering (dim {kkt_dim}, nnz {kkt_nnz})")
            }
            ConicError::FactorisationSetup => write!(f, "conic: LDL factorisation could not be set up"),
            ConicError::SetupRejected { reason } => write!(f, "conic: solver setup rejected the problem: {reason}"),
            ConicError::Solver(e) => write!(f, "conic: no solution ({:?} after {} iterations)", e.cause, e.iterations),
        }
    }
}

impl core::error::Error for ConicError {}

/// Final residuals of a solved problem.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ConicResiduals {
    /// Primal residual.
    pub primal: f64,
    /// Dual residual.
    pub dual: f64,
    /// Absolute duality gap.
    pub gap_abs: f64,
    /// Relative duality gap.
    pub gap_rel: f64,
}

/// A solved problem (REQ-KRN.SOLV.CONIC.WRAP-006); every value is finite.
#[derive(Debug, Clone, PartialEq)]
pub struct ConicSolution {
    /// Primal solution, length `n`.
    pub x: Vec<f64>,
    /// Dual solution (multipliers of `Ax + s = b`), length `m`.
    pub y: Vec<f64>,
    /// Slack, length `m`.
    pub s: Vec<f64>,
    /// Primal objective `1/2 x'Px + q'x`.
    pub objective: f64,
    /// Interior-point iterations performed.
    pub iterations: u32,
    /// Final residuals.
    pub residuals: ConicResiduals,
}

/// Solves a linear program: rejects `P` and second-order cones with
/// [`ConicError::WrongProblemClass`].
pub fn solve_lp(problem: &ConicProblem) -> Result<ConicSolution, ConicError> {
    check_class(problem, ProblemClass::Lp)?;
    solve_with(problem, &ConicLimits::FIXED)
}

/// Solves a convex quadratic program (`P` optional, assumed positive semidefinite): rejects
/// second-order cones with [`ConicError::WrongProblemClass`].
pub fn solve_qp(problem: &ConicProblem) -> Result<ConicSolution, ConicError> {
    check_class(problem, ProblemClass::Qp)?;
    solve_with(problem, &ConicLimits::FIXED)
}

/// Solves a second-order cone program: rejects `P` with [`ConicError::WrongProblemClass`].
pub fn solve_socp(problem: &ConicProblem) -> Result<ConicSolution, ConicError> {
    check_class(problem, ProblemClass::Socp)?;
    solve_with(problem, &ConicLimits::FIXED)
}

fn check_class(problem: &ConicProblem, entry: ProblemClass) -> Result<(), ConicError> {
    let has_p = problem.p.is_some();
    let has_soc = problem.has_soc();
    let ok = match entry {
        ProblemClass::Lp => !has_p && !has_soc,
        ProblemClass::Qp => !has_soc,
        ProblemClass::Socp => !has_p,
    };
    if ok {
        Ok(())
    } else {
        Err(ConicError::WrongProblemClass { entry, has_p, has_soc })
    }
}

/// The KKT size bounds the vendored solver checks at setup (patch 0006): dimension
/// `n + m` plus 2 per second-order cone; stored entries `nnz(P) + nnz(A) + dim` plus
/// `2 d + 2` per second-order cone of dimension `d`. Saturating, like the solver's check.
fn kkt_bounds(problem: &ConicProblem) -> (usize, usize) {
    let p_nnz = problem.p.as_ref().map_or(0, Csc::nnz);
    let mut dim = problem.n().saturating_add(problem.m());
    let mut nnz = p_nnz.saturating_add(problem.a.nnz()).saturating_add(dim);
    for c in &problem.cones {
        if let Cone::Soc(d) = c {
            dim = dim.saturating_add(2);
            nnz = nnz.saturating_add(d.saturating_mul(2)).saturating_add(2);
        }
    }
    (dim, nnz)
}

fn to_vendor(m: &Csc) -> CscMatrix {
    CscMatrix::new(m.nrows, m.ncols, m.colptr.clone(), m.rowval.clone(), m.nzval.clone())
}

fn finite(v: f64) -> Option<f64> {
    if v.is_finite() {
        Some(v)
    } else {
        None
    }
}

/// The one internal solve: budget check, conversion, a single solver run, status mapping and
/// output checks. `limits` is [`ConicLimits::FIXED`] outside this crate's own tests.
pub(crate) fn solve_with(problem: &ConicProblem, limits: &ConicLimits) -> Result<ConicSolution, ConicError> {
    let (kkt_dim, kkt_nnz) = kkt_bounds(problem);
    if kkt_dim > MAX_ORDERING_DIM || kkt_nnz > MAX_ORDERING_NNZ {
        return Err(ConicError::TooLarge { kkt_dim, kkt_nnz });
    }
    let n = problem.n();
    let p = match &problem.p {
        Some(p) => to_vendor(p),
        None => to_vendor(&Csc::zeros(n, n)),
    };
    let a = to_vendor(&problem.a);
    let cones: Vec<SupportedConeT> = problem
        .cones
        .iter()
        .map(|c| match *c {
            Cone::Zero(d) => SupportedConeT::ZeroConeT(d),
            Cone::Nonneg(d) => SupportedConeT::NonnegativeConeT(d),
            Cone::Soc(d) => SupportedConeT::SecondOrderConeT(d),
        })
        .collect();
    let mut solver =
        DefaultSolver::new(&p, &problem.q, &a, &problem.b, &cones, limits::settings(limits)).map_err(|e| match e {
            SetupError::BadInputData(reason) => ConicError::SetupRejected { reason },
            SetupError::SettingsError(_) => ConicError::SetupRejected { reason: "settings" },
        })?;
    // A KKT matrix always stores its diagonal, so zero stored entries means the logical
    // factorisation failed; the solver could only end in NumericalError, so do not run it.
    // Defensive: with the budget check above and the builder's checks, the only known way
    // here is an elimination graph past MAX_ORDERING_FILL (4 000 000 edges) within the
    // dimension and entry budgets. No test reaches it: building that much fill costs far
    // more than a debug or wasm32 CI test can spend.
    if solver.info.linsolver.nnzA == 0 {
        return Err(ConicError::FactorisationSetup);
    }
    solver.solve();

    let info = &solver.info;
    let sol = &solver.solution;
    let fail = |cause: ConicCause| {
        Err(ConicError::Solver(SolverError {
            cause,
            iterations: sol.iterations,
            primal_residual: finite(info.res_primal),
            dual_residual: finite(info.res_dual),
            gap: finite(info.gap_abs),
        }))
    };
    // The final status is `solution.status`: the solver's post-processing demotes a result
    // or certificate without finite values of record to NumericalError there, and only
    // there (`info.status` keeps the undemoted value).
    if let Some(cause) = cause_of(sol.status) {
        return fail(cause);
    }
    let residuals =
        ConicResiduals { primal: info.res_primal, dual: info.res_dual, gap_abs: info.gap_abs, gap_rel: info.gap_rel };
    let all_finite = sol.x.len() == n
        && sol.z.len() == problem.m()
        && sol.s.len() == problem.m()
        && sol.x.iter().chain(&sol.z).chain(&sol.s).all(|v| v.is_finite())
        && sol.obj_val.is_finite()
        && [residuals.primal, residuals.dual, residuals.gap_abs, residuals.gap_rel].iter().all(|v| v.is_finite());
    if !all_finite {
        return fail(ConicCause::NonFiniteOutput);
    }
    Ok(ConicSolution {
        x: sol.x.clone(),
        y: sol.z.clone(),
        s: sol.s.clone(),
        objective: sol.obj_val,
        iterations: sol.iterations,
        residuals,
    })
}

/// The cause for a final solver status, or `None` for `Solved` (the only status that can give
/// a solution). Exhaustive on purpose: a new vendored status must be mapped before this
/// compiles.
pub(crate) fn cause_of(status: SolverStatus) -> Option<ConicCause> {
    match status {
        SolverStatus::Solved => None,
        SolverStatus::PrimalInfeasible => Some(ConicCause::PrimalInfeasible),
        SolverStatus::DualInfeasible => Some(ConicCause::DualInfeasible),
        SolverStatus::AlmostSolved => Some(ConicCause::AlmostSolved),
        SolverStatus::AlmostPrimalInfeasible => Some(ConicCause::AlmostPrimalInfeasible),
        SolverStatus::AlmostDualInfeasible => Some(ConicCause::AlmostDualInfeasible),
        SolverStatus::MaxIterations => Some(ConicCause::IterationLimit),
        SolverStatus::NumericalError => Some(ConicCause::Numerical),
        SolverStatus::InsufficientProgress => Some(ConicCause::InsufficientProgress),
        SolverStatus::Unsolved => Some(ConicCause::NotRun),
    }
}

#[cfg(test)]
mod tests;
