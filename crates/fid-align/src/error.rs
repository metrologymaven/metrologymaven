//! The error type of the crate: a closed set with stable codes.

use crate::solver::SolverError;
use fid_dep::EdgeError;
use fid_geo_frame::{UnitsError, XformError};
use fid_project::IdError;
use fid_uuid::Uuid;
use std::fmt;

/// Longest error code a solver may return, in bytes.
pub const MAX_CODE_BYTES: usize = 64;

/// Every way this crate refuses an input. Nothing is created, changed or journaled when one
/// is returned, except where [`crate::AlignmentModel::recompute`] records a failure as a
/// status (REQ-006, REQ-007).
#[derive(Debug, Clone, PartialEq)]
pub enum AlignError {
    /// The algorithm ID and version is not registered (REQ-007). Holds the ID text.
    MethodUnavailable(String),
    /// An input element is not known to the project or to the edge index.
    InputMissing(Uuid),
    /// An alignment (the target of `get`, or a parent) does not exist.
    NotFound(Uuid),
    /// The solver refused; its code is passed through unchanged (REQ-006).
    Solver(SolverError),
    /// The same input is listed twice.
    DuplicateInput(Uuid),
    /// The parent alignment's status is not `ok`, so it cannot carry a child.
    ParentUnusable(Uuid),
    /// A freshly drawn ID is already taken.
    IdCollision(Uuid),
    /// Command parameters do not match the pinned shape. Holds a reason.
    BadParams(&'static str),
    /// A serialised alignment or a value inside it does not match the pinned shape.
    BadElement(&'static str),
    /// An enumerated value this build does not know (element status, principal kind,
    /// provenance state, transform kind, unit, command). Holds the value, cut to 64 bytes.
    UnknownKind(String),
    /// A NaN or infinite value. Holds where it was found.
    NonFinite(&'static str),
    /// A transform that is not a valid rigid or similarity transform (degenerate geometry).
    Degenerate(XformError),
    /// An authored quantity that is not a valid decimal of the pinned grammar.
    BadQuantity(UnitsError),
    /// A stored digest differs from the digest of the stored content.
    DigestMismatch,
    /// A solver for this algorithm ID and version is already registered.
    DuplicateAlgorithm(String),
    /// A size budget was exceeded. Holds the name of the budget.
    InputLimit(&'static str),
    /// The dependency graph refused the edges.
    Edge(EdgeError),
    /// The ID service refused.
    Id(IdError),
    /// A fault that cannot be the caller's mistake.
    Internal(&'static str),
}

impl AlignError {
    /// Stable code. `method_unavailable`, `input_missing` and `not_found` are the codes the
    /// leaf spec names; solver codes pass through unchanged; the rest refuse malformed input.
    pub fn code(&self) -> &str {
        match self {
            AlignError::MethodUnavailable(_) => "method_unavailable",
            AlignError::InputMissing(_) => "input_missing",
            AlignError::NotFound(_) => "not_found",
            AlignError::Solver(e) => e.code(),
            AlignError::DuplicateInput(_) => "duplicate_input",
            AlignError::ParentUnusable(_) => "parent_unusable",
            AlignError::IdCollision(_) => "id_collision",
            AlignError::BadParams(_) => "bad_params",
            AlignError::BadElement(_) => "bad_element",
            AlignError::UnknownKind(_) => "unknown_kind",
            AlignError::NonFinite(_) => "non_finite",
            AlignError::Degenerate(_) => "degenerate_transform",
            AlignError::BadQuantity(_) => "bad_quantity",
            AlignError::DigestMismatch => "digest_mismatch",
            AlignError::DuplicateAlgorithm(_) => "duplicate_algorithm",
            AlignError::InputLimit(_) => "input_limit",
            AlignError::Edge(e) => e.code(),
            AlignError::Id(e) => e.code(),
            AlignError::Internal(_) => "internal",
        }
    }
}

/// Copies at most 64 bytes of untrusted text, cut at a char boundary.
pub(crate) fn kept(s: &str) -> String {
    let mut end = s.len().min(MAX_CODE_BYTES);
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    s.get(..end).unwrap_or("").to_string()
}

impl fmt::Display for AlignError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code())?;
        match self {
            AlignError::MethodUnavailable(s) | AlignError::DuplicateAlgorithm(s) | AlignError::UnknownKind(s) => {
                write!(f, ": {s:?}")
            }
            AlignError::InputMissing(id)
            | AlignError::NotFound(id)
            | AlignError::DuplicateInput(id)
            | AlignError::ParentUnusable(id)
            | AlignError::IdCollision(id) => write!(f, ": {id}"),
            AlignError::BadParams(s)
            | AlignError::BadElement(s)
            | AlignError::NonFinite(s)
            | AlignError::InputLimit(s)
            | AlignError::Internal(s) => write!(f, ": {s}"),
            AlignError::Degenerate(e) => write!(f, ": {e}"),
            AlignError::BadQuantity(e) => write!(f, ": {e}"),
            AlignError::Edge(e) => write!(f, ": {e}"),
            AlignError::Id(e) => write!(f, ": {e}"),
            AlignError::Solver(_) | AlignError::DigestMismatch => Ok(()),
        }
    }
}

impl std::error::Error for AlignError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_text_is_cut_at_a_char_boundary() {
        assert_eq!(kept("abc"), "abc");
        let long = "é".repeat(100);
        let k = kept(&long);
        assert!(k.len() <= MAX_CODE_BYTES && !k.is_empty());
        assert!(long.starts_with(&k));
    }
}
