//! Active-set certificates (spec node KRN.SOLV.CERT): solver iterates are not results.
//!
//! The certificate pipeline extracts candidate active sets from a converged conic solution
//! ([`active`], KRN.SOLV.CERT.AS), recomputes the optimum from a candidate, checks the KKT
//! conditions independently and packages a certificate. [`recomp`] (KRN.SOLV.CERT.RECOMP)
//! recomputes the optimum on one candidate from the problem data, with a proven forward error
//! bound. [`kkt`] (KRN.SOLV.CERT.KKT) proves or refutes the KKT conditions at the exact optimum
//! enclosed by a recomputation. [`ct`] (KRN.SOLV.CERT.CT) runs the pipeline ([`certify`]) and
//! packages the passing candidate as a [`Certificate`] with its digest. See
//! `spec/krn/solv/cert/`.

pub mod active;
pub mod ct;
pub mod kkt;
pub mod recomp;

use core::fmt;

use fid_num::NumError;

pub use active::{Proposal, RowLabel};
pub use ct::{certify, CertConfig, CertFlags, CertKind, Certificate, Certified, KktRecord, RecordedCondition};
pub use recomp::ActiveSetProblem;

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
    /// `certificate_failed` (REQ-KRN.SOLV.CERT-003, REQ-KRN.SOLV.CERT.CT-002): no candidate
    /// recomputed and passed the KKT check. No value is returned. Exactly one of
    /// `first_report` and `first_error` is set: the first candidate's KKT report, or the error
    /// that stopped it before a report existed.
    CertificateFailed {
        /// Candidates tried (all of them).
        tried: usize,
        /// The first candidate's KKT report, if it got that far.
        first_report: Option<Box<kkt::KktReport>>,
        /// The error of the first candidate's recomputation or check, if it had no report.
        first_error: Option<Box<CertError>>,
    },
    /// A serialised certificate is not a canonical `fid.cert@1` document (`detail` names the
    /// failed check, for example `syntax`, `unknown_member`, `digest_mismatch` or
    /// `non_canonical`).
    Malformed {
        /// What failed.
        detail: &'static str,
    },
    /// A certificate's recorded values contradict each other (`detail` names the first failed
    /// check of `Certificate::check_consistency`).
    Inconsistent {
        /// What failed.
        detail: &'static str,
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
            CertError::CertificateFailed { tried, first_report, first_error } => {
                write!(f, "certificate_failed: no candidate passed ({tried} tried")?;
                if let Some(r) = first_report {
                    write!(f, "; first: KKT {:?} {:?}", r.status(), r.reason())?;
                }
                if let Some(e) = first_error {
                    write!(f, "; first: {e}")?;
                }
                write!(f, ")")
            }
            CertError::Malformed { detail } => write!(f, "cert: malformed certificate ({detail})"),
            CertError::Inconsistent { detail } => write!(f, "cert: inconsistent certificate ({detail})"),
        }
    }
}

impl CertError {
    /// A stable snake_case code per variant, for logs and output pins (the fields are encoded
    /// by the caller; `Debug` output is not a stable format).
    pub fn code(&self) -> &'static str {
        match self {
            CertError::Shape { .. } => "shape",
            CertError::InvalidInput { .. } => "invalid_input",
            CertError::DuplicateLabel { .. } => "duplicate_label",
            CertError::ActiveSetUndetermined { .. } => "active_set_undetermined",
            CertError::RankDeficient { .. } => "rank_deficient",
            CertError::IllConditioned { .. } => "ill_conditioned",
            CertError::RecomputeNoConvergence { .. } => "recompute_no_convergence",
            CertError::Callback { .. } => "callback",
            CertError::Numeric(_) => "numeric",
            CertError::BoundNotProven { .. } => "bound_not_proven",
            CertError::CertificateFailed { .. } => "certificate_failed",
            CertError::Malformed { .. } => "malformed",
            CertError::Inconsistent { .. } => "inconsistent",
        }
    }
}

impl core::error::Error for CertError {}

impl From<NumError> for CertError {
    fn from(e: NumError) -> CertError {
        CertError::Numeric(e)
    }
}
