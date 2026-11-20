//! Active-set certificates (spec node KRN.SOLV.CERT): solver iterates are not results.
//!
//! The certificate pipeline extracts candidate active sets from a converged conic solution
//! ([`active`], KRN.SOLV.CERT.AS), recomputes the optimum from a candidate, checks the KKT
//! conditions independently and packages a certificate. [`recomp`] (KRN.SOLV.CERT.RECOMP)
//! recomputes the optimum on one candidate from the problem data, with a proven forward error
//! bound. [`kkt`] (KRN.SOLV.CERT.KKT) proves or refutes the KKT conditions at the exact optimum
//! enclosed by a recomputation. Modules land one spec leaf at a time; see `spec/krn/solv/cert/`.

pub mod active;
pub mod kkt;
pub mod recomp;

use core::fmt;

use fid_num::NumError;

pub use active::{Proposal, RowLabel};

/// Why no candidate list could be returned (REQ-KRN.SOLV.CERT.AS-004).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Undetermined {
    /// Fewer rows have small slack (strongly active plus ambiguous) than the expected size.
    TooFewRows,
    /// More candidate sets exist than [`active::MAX_CANDIDATES`].
    OverCap,
}

/// Why [`recomp::recompute`] could not prove a forward error bound for its result. The
/// recomputation fails closed: without a proven bound there is no result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum BoundFailure {
    /// The active-set system is not square (more equations than parameters), so it has no
    /// isolated root to enclose.
    NotSquare,
    /// The problem supplies no enclosure that the bound needs (`value_enclosure`, or for a
    /// nonlinear problem `residual_enclosure` or `jacobian_enclosure`).
    NoEnclosure,
    /// The Krawczyk test did not contract within its fixed number of attempts.
    NotContracting,
    /// An enclosure overflowed, or an interval division or square root met 0 or a negative
    /// argument.
    Overflow,
}

/// Every way the certificate pipeline can fail. Later leaves of KRN.SOLV.CERT add variants,
/// so the enum is non-exhaustive. (Not `Eq`: [`CertError::IllConditioned`] and
/// [`CertError::RecomputeNoConvergence`] carry an `f64`.)
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum CertError {
    /// An input has the wrong length: `field` (`"labels"`, `"y"` or `"s"`) has `got`
    /// entries where the problem has `expected` rows.
    Shape {
        /// The input with the wrong length.
        field: &'static str,
        /// Rows of the problem.
        expected: usize,
        /// Entries supplied.
        got: usize,
    },
    /// An input is outside its domain: `"thresholds"` (a threshold that is negative or not
    /// finite), `"expected"` (an expected size of 0), `"cones"` (a cone other than the
    /// nonnegative orthant), `"y"` or `"s"` (a value that is not finite).
    InvalidInput {
        /// The failing input.
        field: &'static str,
    },
    /// Two rows carry the same label, so a set of labels would not name a set of rows.
    DuplicateLabel {
        /// The repeated label.
        label: RowLabel,
    },
    /// No candidate list could be formed (REQ-KRN.SOLV.CERT.AS-004). Both lists are sorted.
    ActiveSetUndetermined {
        /// Rows with small slack and a clearly positive multiplier.
        strongly_active: Vec<RowLabel>,
        /// Rows with small slack and a multiplier that is not clearly positive.
        ambiguous: Vec<RowLabel>,
        /// Why no list was returned.
        cause: Undetermined,
    },
    /// The active-set system has numerical rank `rank` below the parameter count under the
    /// recorded tolerance (REQ-KRN.SOLV.CERT.RECOMP-002).
    RankDeficient {
        /// Numerical rank of the active-set system.
        rank: usize,
    },
    /// The condition estimate of the active-set system exceeds the configured threshold
    /// (REQ-KRN.SOLV.CERT.RECOMP-004); no value is returned.
    IllConditioned {
        /// The condition estimate (`+inf` when beyond the `f64` range).
        cond: f64,
    },
    /// The last Gauss-Newton step on a nonlinear active-set system is longer than `4 u` times
    /// the parameter norm (REQ-KRN.SOLV.CERT.RECOMP-003).
    RecomputeNoConvergence {
        /// 2-norm of the last step.
        step_norm: f64,
    },
    /// An `ActiveSetProblem` implementation refused its input.
    Callback {
        /// The implementation's reason.
        reason: &'static str,
    },
    /// A `fid-num` operation failed (overflow, allocation, a non-finite value).
    Numeric(NumError),
    /// No forward error bound could be proven, so no result is returned (fail closed).
    BoundNotProven {
        /// Why.
        reason: BoundFailure,
    },
}

impl fmt::Display for CertError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CertError::Shape { field, expected, got } => {
                write!(f, "cert: {field} has {got} entries, the problem has {expected} rows")
            }
            CertError::InvalidInput { field } => write!(f, "cert: invalid input {field}"),
            CertError::DuplicateLabel { label } => {
                write!(f, "cert: duplicate row label (point {}, side {})", label.point, label.side)
            }
            CertError::ActiveSetUndetermined { strongly_active, ambiguous, cause } => write!(
                f,
                "cert: active set undetermined ({cause:?}; {} strongly active, {} ambiguous rows)",
                strongly_active.len(),
                ambiguous.len()
            ),
            CertError::RankDeficient { rank } => write!(f, "cert: active-set system is rank deficient (rank {rank})"),
            CertError::IllConditioned { cond } => {
                write!(f, "cert: active-set system is ill-conditioned (cond {cond:e})")
            }
            CertError::RecomputeNoConvergence { step_norm } => {
                write!(f, "cert: recomputation did not converge (last step norm {step_norm:e})")
            }
            CertError::Callback { reason } => write!(f, "cert: active-set problem: {reason}"),
            CertError::Numeric(e) => write!(f, "cert: {e}"),
            CertError::BoundNotProven { reason } => write!(f, "cert: no proven error bound ({reason:?})"),
        }
    }
}

impl core::error::Error for CertError {}

impl From<NumError> for CertError {
    fn from(e: NumError) -> CertError {
        CertError::Numeric(e)
    }
}
