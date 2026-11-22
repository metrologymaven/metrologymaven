//! The fit error taxonomy (REQ-KRN.LSQ.TRAIT-004, Design "Errors").

use core::fmt;

use fid_num::NumError;
use fid_solv::nlls::{Diagnostics, NllsConfig, SolverError};

use super::primitive::{Method, Primitive};
// `TanVariant` lives in `tan_rock` because spec node KRN.ENV.TAN.ROCK owns it (with the
// defined-pairs table); the contract's error type names it only so that an undefined
// (primitive, variant) pair can be reported with both values (REQ-KRN.ENV.TAN.ROCK-004).
// `tan_rock` itself depends on the contract, never the other way round for behaviour: this is
// a type-only reference within one crate, so no module cycle in logic arises.
use crate::tan_rock::TanVariant;

/// Why a fit, a check or a contract operation returned no result. Every variant carries
/// the numbers that explain it; nothing is ever replaced by a silent best effort.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FitError {
    /// A NaN or an infinity in the coordinates of point `index` (zero-based point index).
    NonFinite { index: usize },
    /// A non-zero subnormal coordinate (magnitude below `2^-1022`) in point `index`: it
    /// carries fewer than 53 significant bits, so it is refused rather than used.
    Subnormal { index: usize },
    /// A NaN or an infinity in the normal of point `index`.
    NonFiniteNormal { index: usize },
    /// The normal of point `index` is not unit length within [`super::UNIT_NORMAL_TOL`].
    NonUnitNormal { index: usize },
    /// The coordinate buffer length is not a multiple of 3.
    BadLength { len: usize },
    /// The normal buffer length (`normals`) differs from the coordinate buffer length
    /// (`points`); both are element counts.
    LengthMismatch { points: usize, normals: usize },
    /// More points than [`super::MAX_POINTS`].
    TooLarge { n: usize, max: usize },
    /// Fewer points than the primitive needs.
    TooFewPoints { n: usize, need: usize },
    /// The centred data has numerical rank 0 or 1 (all points coincide or lie on a line)
    /// where the primitive needs more. `cond` is the ratio of the largest singular value to
    /// the one the primitive needs (`+inf` when that one is zero).
    Collinear { n: usize, rank: usize, cond: f64 },
    /// The centred data has numerical rank 2 (all points in a plane) where the primitive
    /// needs rank 3.
    Coplanar { n: usize, rank: usize, cond: f64 },
    /// The angular coverage of circle-type data (radians) is below the threshold `need`.
    InsufficientCoverage { n: usize, coverage: f64, need: f64 },
    /// A Jacobian condition estimate above the configured limit (`None`: beyond the `f64`
    /// range).
    IllConditioned { cond: Option<f64> },
    /// An iteration did not converge within its bound. `reason` says which bound; `last` holds
    /// the NLLS driver's last diagnostics when the driver hit it.
    NotConverged { iterations: usize, reason: NotConvergedReason, last: Option<Diagnostics> },
    /// The method is not defined for the primitive.
    MethodUndefined { primitive: Primitive, method: Method },
    /// The tangential variant is not defined for the primitive (`tan_rock::DEFINED_PAIRS`).
    /// Its code is `method_undefined`, like [`FitError::MethodUndefined`].
    TanMethodUndefined { primitive: Primitive, variant: TanVariant },
    /// Two or more candidates for the same contact set share the best objective bit for bit
    /// but carry different features (for example opposite-sign normals). There is no rule
    /// that may pick one, so the rocking report refuses them (`tan_rock::rock_report`).
    /// `first_contact` and `contacts` (the set's lowest point index and size) identify the set.
    AmbiguousCandidate { first_contact: u32, contacts: usize },
    /// Every start of a sequential-linearisation run (`fid_solv::seq`, used by the tangential
    /// fits) ended without a certified value. `cause` is the `SeqCause::code()` of the first
    /// start's failure (`certificate_failed` failures are `cert`), `starts` the number of starts
    /// tried. No iterate is ever returned in place of a certified result. Code `solver`.
    SeqFailed { cause: &'static str, starts: u32 },
    /// The tangential variant has no formulation the certificate pipeline can certify with a
    /// free normal (`tan_plane`: constrained L2, whose optimum is in general not a vertex). Use
    /// a fixed normal, or another variant. Code `not_certifiable`.
    NotCertifiable { variant: TanVariant },
    /// A certified tangential result leaves point `index` on the material-free side by
    /// `violation` metres, more than the recorded `side_tol`. Code `side_violation`.
    SideViolation { index: usize, violation: f64 },
    /// A parameter is out of range; `field` names it (for example `thresholds.rank_tol`).
    InvalidParams { field: &'static str },
    /// A direction vector is zero or too small to normalise (norm below `2^-1022`).
    InvalidDirection,
    /// An ill-posed cone (`ls_cone`, KRN.LSQ.AXIS.CONE): its fitted `half_angle` (radians) is
    /// within the configured threshold of 0 (near-cylinder) or of `pi / 2` (near-plane), or the
    /// condition estimate `cond` of the scaled Jacobian at the fit is above the cone's limit
    /// (for example a short axial extent), or a later solve round about the fit is
    /// ill-conditioned or rank deficient; through the `Fit` trait also an axis whose standard
    /// uncertainty is above the cone's limit. `cond` is `None` beyond the `f64` range. Code
    /// `ill_conditioned`, like [`FitError::IllConditioned`].
    IllPosedCone { half_angle: f64, cond: Option<f64> },
    /// A converged least-squares fit whose residual RMS `rms` is not below `bound` (by a
    /// relative margin), the RMS of the best plane its feature family tends to as the radius
    /// grows (both metres). The family's least-squares infimum is at most the bound's, so the fit
    /// is not the global optimum, or none exists (`ls_cyl`, KRN.LSQ.AXIS.CYL). Code
    /// `implausible_fit`.
    ImplausibleFit { rms: f64, bound: f64 },
    /// A fit that carries a warning flag, refused by the `Fit` trait so a generic caller never
    /// receives it unflagged (fail closed): `local_optimum_only` (the starts disagree) and
    /// `suspect_axis` (axis weakly determined or wrong, with its `rms_ratio`). The flagged fit
    /// itself is available from the method's own entry point (`ls_cyl::fit_cylinder`). Code
    /// `flagged_fit`.
    FlaggedFit { local_optimum_only: bool, suspect_axis: bool, rms_ratio: f64 },
    /// Every start of a multi-start run (`fid_solv::mstart`) failed. `starts` is the number
    /// of starts and `first` the code of the primary start's error. Code `multi_start`.
    MultiStartFailed { starts: u32, first: &'static str },
    /// The nonlinear least-squares driver failed (rank deficiency and the other driver
    /// errors that have no dedicated variant here).
    Solver(SolverError),
    /// A numeric kernel error (non-finite intermediate, overflow, allocation failure, ...).
    Num(NumError),
}

impl FitError {
    /// A stable machine-readable code.
    pub const fn code(&self) -> &'static str {
        match self {
            FitError::NonFinite { .. } => "non_finite",
            FitError::Subnormal { .. } => "subnormal",
            FitError::NonFiniteNormal { .. } => "non_finite_normal",
            FitError::NonUnitNormal { .. } => "non_unit_normal",
            FitError::BadLength { .. } => "bad_length",
            FitError::LengthMismatch { .. } => "length_mismatch",
            FitError::TooLarge { .. } => "too_large",
            FitError::TooFewPoints { .. } => "too_few_points",
            FitError::Collinear { .. } => "collinear",
            FitError::Coplanar { .. } => "coplanar",
            FitError::InsufficientCoverage { .. } => "insufficient_coverage",
            FitError::IllConditioned { .. } | FitError::IllPosedCone { .. } => "ill_conditioned",
            FitError::NotConverged { .. } => "not_converged",
            FitError::MethodUndefined { .. } | FitError::TanMethodUndefined { .. } => "method_undefined",
            FitError::AmbiguousCandidate { .. } => "ambiguous_candidate",
            FitError::SeqFailed { .. } => "solver",
            FitError::NotCertifiable { .. } => "not_certifiable",
            FitError::SideViolation { .. } => "side_violation",
            FitError::InvalidParams { .. } => "invalid_params",
            FitError::InvalidDirection => "invalid_direction",
            FitError::ImplausibleFit { .. } => "implausible_fit",
            FitError::FlaggedFit { .. } => "flagged_fit",
            FitError::MultiStartFailed { .. } => "multi_start",
            FitError::Solver(_) => "solver",
            FitError::Num(_) => "numeric",
        }
    }
}

impl fmt::Display for FitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FitError::NonFinite { index } => write!(f, "fit: non-finite coordinate in point {index}"),
            FitError::Subnormal { index } => write!(f, "fit: subnormal coordinate in point {index}"),
            FitError::NonFiniteNormal { index } => write!(f, "fit: non-finite normal at point {index}"),
            FitError::NonUnitNormal { index } => write!(f, "fit: normal at point {index} is not unit length"),
            FitError::BadLength { len } => write!(f, "fit: coordinate buffer length {len} is not a multiple of 3"),
            FitError::LengthMismatch { points, normals } => {
                write!(f, "fit: normals length {normals} differs from coordinates length {points}")
            }
            FitError::TooLarge { n, max } => write!(f, "fit: {n} points exceed the limit {max}"),
            FitError::TooFewPoints { n, need } => write!(f, "fit: {n} points, need at least {need}"),
            FitError::Collinear { n, rank, cond } => {
                write!(f, "fit: collinear data ({n} points, rank {rank}, cond {cond:e})")
            }
            FitError::Coplanar { n, rank, cond } => {
                write!(f, "fit: coplanar data ({n} points, rank {rank}, cond {cond:e})")
            }
            FitError::InsufficientCoverage { n, coverage, need } => {
                write!(f, "fit: angular coverage {coverage:e} rad below {need:e} rad ({n} points)")
            }
            FitError::IllConditioned { cond: Some(c) } => write!(f, "fit: ill-conditioned (cond {c:e})"),
            FitError::IllConditioned { cond: None } => write!(f, "fit: ill-conditioned (cond beyond range)"),
            FitError::IllPosedCone { half_angle, cond: Some(c) } => {
                write!(f, "fit: ill-posed cone (half-angle {half_angle:e} rad, cond {c:e})")
            }
            FitError::IllPosedCone { half_angle, cond: None } => {
                write!(f, "fit: ill-posed cone (half-angle {half_angle:e} rad, cond beyond range)")
            }
            FitError::NotConverged { iterations, reason, .. } => {
                write!(f, "fit: not converged after {iterations} iterations ({})", reason.as_str())
            }
            FitError::MethodUndefined { primitive, method } => {
                write!(f, "fit: method {} undefined for {}", method.slug(), primitive.slug())
            }
            FitError::TanMethodUndefined { primitive, variant } => {
                write!(f, "fit: tangential variant {} undefined for {}", variant.slug(), primitive.slug())
            }
            FitError::AmbiguousCandidate { first_contact, contacts } => write!(
                f,
                "fit: candidates for the contact set starting at point {first_contact} ({contacts} contacts) tie on the objective with different features"
            ),
            FitError::SeqFailed { cause, starts } => {
                write!(f, "fit: no certified result from {starts} starts (first failure: {cause})")
            }
            FitError::NotCertifiable { variant } => {
                write!(f, "fit: tangential variant {} cannot be certified with a free normal", variant.slug())
            }
            FitError::SideViolation { index, violation } => {
                write!(f, "fit: point {index} lies {violation:e} m on the material-free side, beyond side_tol")
            }
            FitError::InvalidParams { field } => write!(f, "fit: invalid parameter {field}"),
            FitError::InvalidDirection => write!(f, "fit: zero or too small direction vector"),
            FitError::ImplausibleFit { rms, bound } => {
                write!(f, "fit: residual RMS {rms:e} m is not below the bounding plane's {bound:e} m (not the global optimum, or none exists)")
            }
            FitError::FlaggedFit { local_optimum_only, suspect_axis, rms_ratio } => write!(
                f,
                "fit: flagged fit refused (local_optimum_only {local_optimum_only}, suspect_axis {suspect_axis}, rms ratio {rms_ratio:e})"
            ),
            FitError::MultiStartFailed { starts, first } => {
                write!(f, "fit: all {starts} starts failed (primary start: {first})")
            }
            FitError::Solver(e) => write!(f, "fit: {e}"),
            FitError::Num(e) => write!(f, "fit: {e}"),
        }
    }
}

impl core::error::Error for FitError {}

impl From<NumError> for FitError {
    fn from(e: NumError) -> Self {
        FitError::Num(e)
    }
}

/// Which bound an iteration hit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotConvergedReason {
    /// The NLLS driver accepted `max_iter` steps without a stop.
    MaxIter,
    /// The NLLS driver rejected `fid_solv::nlls::MAX_TRIALS` consecutive trial steps.
    TrialsExhausted,
    /// The degeneracy checks' Jacobi iteration used all its sweeps.
    JacobiSweeps,
}

impl NotConvergedReason {
    /// The wire string.
    pub const fn as_str(self) -> &'static str {
        match self {
            NotConvergedReason::MaxIter => "max_iter",
            NotConvergedReason::TrialsExhausted => "trials_exhausted",
            NotConvergedReason::JacobiSweeps => "jacobi_sweeps",
        }
    }
}

impl FitError {
    /// Maps an NLLS driver error run under `cfg`. `IterationLimit { last }` becomes
    /// [`FitError::NotConverged`] keeping `last`: reason [`NotConvergedReason::MaxIter`] when
    /// `last.iterations == cfg.max_iter` (the driver checks the step limit before trying a
    /// step, so only the limit can stop it there), else
    /// [`NotConvergedReason::TrialsExhausted`]. `IllConditioned` becomes
    /// [`FitError::IllConditioned`]; every other driver error is kept as [`FitError::Solver`].
    pub fn from_solver(e: SolverError, cfg: &NllsConfig) -> FitError {
        match e {
            SolverError::IterationLimit { last } => FitError::NotConverged {
                iterations: last.iterations,
                reason: if last.iterations == cfg.max_iter {
                    NotConvergedReason::MaxIter
                } else {
                    NotConvergedReason::TrialsExhausted
                },
                last: Some(last),
            },
            SolverError::IllConditioned { cond, .. } => FitError::IllConditioned { cond },
            other => FitError::Solver(other),
        }
    }
}

impl From<SolverError> for FitError {
    /// Without the configuration an `IterationLimit` cannot be told apart, so it is kept
    /// whole as [`FitError::Solver`] (with its diagnostics); use [`FitError::from_solver`] to
    /// get [`FitError::NotConverged`] with its reason. `IllConditioned` becomes
    /// [`FitError::IllConditioned`]; every other driver error is [`FitError::Solver`].
    fn from(e: SolverError) -> Self {
        match e {
            SolverError::IllConditioned { cond, .. } => FitError::IllConditioned { cond },
            other => FitError::Solver(other),
        }
    }
}

/// The error for a buffer that could not be reserved (`try_reserve_exact` failed).
pub(crate) fn alloc_failed(op: &'static str) -> FitError {
    FitError::Num(NumError::Domain { op, detail: "allocation_failed" })
}

/// An empty vector with capacity for exactly `n` elements, reserved fallibly.
pub(crate) fn alloc<T>(op: &'static str, n: usize) -> Result<Vec<T>, FitError> {
    let mut v = Vec::new();
    v.try_reserve_exact(n).map_err(|_| alloc_failed(op))?;
    Ok(v)
}
