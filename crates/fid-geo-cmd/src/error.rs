//! The error type of the crate: a closed set with stable codes.

use fid_dep::{EdgeError, PropError};
use fid_geo_types::{ElementKind, GeoTypeError};
use fid_project::IdError;
use fid_uuid::Uuid;
use std::fmt;

/// Longest untrusted text an error keeps, in bytes.
const MAX_KEPT_BYTES: usize = 64;

/// Every way this crate refuses. When one is returned by [`crate::register_element`] or
/// [`crate::replace_element`], the tree, the edge index and the element states are
/// unchanged; `geo.describe_element@1` never changes anything.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GeoCmdError {
    /// No element has this ID (REQ-005).
    NotFound(Uuid),
    /// The element exists but is not geometric (REQ-005).
    NotGeometry(Uuid),
    /// Command parameters do not match the pinned shape. Holds a reason.
    BadParams(&'static str),
    /// A tool or CLI subcommand this crate does not serve. Holds the name, cut to 64 bytes.
    UnknownCommand(String),
    /// The ID chosen for a registration is already taken.
    IdCollision(Uuid),
    /// An imported source ID is empty, too long or holds a control character.
    BadSourceId(&'static str),
    /// A derivation input is listed twice.
    DuplicateInput(Uuid),
    /// A derivation input is a geometric element whose current digest differs from the
    /// digest given for it.
    DigestMismatch(Uuid),
    /// A derivation output name is empty, too long or holds a control character.
    BadOutput(&'static str),
    /// The element's bounding box could not be computed (any error but `empty`).
    Geometry(GeoTypeError),
    /// A replacement would change the element's kind.
    KindMismatch { expected: ElementKind, found: ElementKind },
    /// A size budget was exceeded. Holds the name of the budget.
    InputLimit(&'static str),
    /// The dependency graph refused the edges.
    Edge(EdgeError),
    /// Change propagation refused.
    Prop(PropError),
    /// The ID service refused.
    Id(IdError),
    /// The element tree behaved against its contract.
    Internal(&'static str),
}

impl GeoCmdError {
    /// Stable code. `not_found` and `not_geometry` are the codes the leaf names; the edge,
    /// propagation and ID codes pass through unchanged.
    pub fn code(&self) -> &'static str {
        match self {
            GeoCmdError::NotFound(_) => "not_found",
            GeoCmdError::NotGeometry(_) => "not_geometry",
            GeoCmdError::BadParams(_) => "bad_params",
            GeoCmdError::UnknownCommand(_) => "unknown_command",
            GeoCmdError::IdCollision(_) => "id_collision",
            GeoCmdError::BadSourceId(_) => "bad_source_id",
            GeoCmdError::DuplicateInput(_) => "duplicate_input",
            GeoCmdError::DigestMismatch(_) => "digest_mismatch",
            GeoCmdError::BadOutput(_) => "bad_output",
            GeoCmdError::Geometry(e) => e.code(),
            GeoCmdError::KindMismatch { .. } => "kind_mismatch",
            GeoCmdError::InputLimit(_) => "input_limit",
            GeoCmdError::Edge(e) => e.code(),
            GeoCmdError::Prop(e) => e.code(),
            GeoCmdError::Id(e) => e.code(),
            GeoCmdError::Internal(_) => "internal",
        }
    }
}

/// Copies at most 64 bytes of untrusted text, cut at a char boundary.
pub(crate) fn kept(s: &str) -> String {
    let mut end = s.len().min(MAX_KEPT_BYTES);
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    s.get(..end).unwrap_or("").to_string()
}

impl fmt::Display for GeoCmdError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code())?;
        match self {
            GeoCmdError::NotFound(id)
            | GeoCmdError::NotGeometry(id)
            | GeoCmdError::IdCollision(id)
            | GeoCmdError::DuplicateInput(id)
            | GeoCmdError::DigestMismatch(id) => write!(f, ": {id}"),
            GeoCmdError::Geometry(e) => write!(f, ": {e}"),
            GeoCmdError::BadParams(s)
            | GeoCmdError::BadSourceId(s)
            | GeoCmdError::BadOutput(s)
            | GeoCmdError::InputLimit(s)
            | GeoCmdError::Internal(s) => write!(f, ": {s}"),
            GeoCmdError::UnknownCommand(s) => write!(f, ": {s:?}"),
            GeoCmdError::KindMismatch { expected, found } => write!(f, ": expected {expected}, found {found}"),
            GeoCmdError::Edge(e) => write!(f, ": {e}"),
            GeoCmdError::Prop(e) => write!(f, ": {e:?}"),
            GeoCmdError::Id(e) => write!(f, ": {e}"),
        }
    }
}

impl std::error::Error for GeoCmdError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_text_is_cut_at_a_char_boundary() {
        assert_eq!(kept("abc"), "abc");
        let long = "é".repeat(100);
        let k = kept(&long);
        assert!(k.len() <= MAX_KEPT_BYTES && !k.is_empty());
        assert!(long.starts_with(&k));
    }
}
