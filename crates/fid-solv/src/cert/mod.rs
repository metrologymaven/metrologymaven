//! Active-set certificates (spec node KRN.SOLV.CERT): solver iterates are not results.
//!
//! The certificate pipeline extracts candidate active sets from a converged conic solution
//! ([`active`], KRN.SOLV.CERT.AS), recomputes the optimum from a candidate, checks the KKT
//! conditions independently and packages a certificate. Modules land one spec leaf at a
//! time; see `spec/krn/solv/cert/`.

pub mod active;

use core::fmt;

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

/// Every way the certificate pipeline can fail. Later leaves of KRN.SOLV.CERT add variants,
/// so the enum is non-exhaustive.
#[derive(Debug, Clone, PartialEq, Eq)]
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
        }
    }
}

impl core::error::Error for CertError {}
