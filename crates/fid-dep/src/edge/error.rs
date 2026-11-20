//! The error type of the edge module: a closed set with stable codes and reasons.

use super::EntityId;
use std::fmt;

/// Longest untrusted text kept in an error, in bytes; longer text is cut at a char boundary.
const KEEP: usize = 128;

/// Every way the edge module refuses an input. Nothing is committed when one is returned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EdgeError {
    /// The commit would close a cycle. The element IDs of the cycle, in edge direction,
    /// starting from the smallest ID (REQ-CORE.DEP.EDGE-003).
    Cycle(Vec<EntityId>),
    /// The element is not in the index (query target, or an update of an unknown element).
    UnknownElement(EntityId),
    /// An input of `element` is not a known element (a dangling edge).
    DanglingInput { element: EntityId, input: EntityId },
    /// A create names an element that already exists.
    AlreadyExists(EntityId),
    /// One commit sets the same element twice.
    DuplicateElement(EntityId),
    /// An [`super::EdgesSet`] lists the same input twice (a duplicate edge).
    DuplicateEdge { element: EntityId, input: EntityId },
    /// An [`super::EdgesSet`] lists its inputs out of ascending ID order.
    UnsortedInputs(EntityId),
    /// An `x-ref` parameter value is not a canonical lowercase UUID string. Holds the
    /// JSON path of the value.
    BadReference(String),
    /// The parameter schema has an `x-ref` where the walker cannot follow it, so some
    /// edges could be missed. Holds a short reason.
    UnsupportedSchema(&'static str),
    /// `project.show_dependencies@1` parameters do not match the schema. Holds a reason.
    BadParams(&'static str),
    /// An `EdgesSet` event does not have the pinned shape. Holds a reason.
    BadEvent(&'static str),
    /// A size or depth budget was exceeded. Holds the name of the budget.
    InputLimit(&'static str),
    /// A fault that cannot be the caller's mistake (canonical JSON refused our own value).
    Internal(&'static str),
}

impl EdgeError {
    /// Stable code. `cycle` and `not_found` are the two codes the leaf spec names; the
    /// others refuse malformed input (fail closed).
    pub fn code(&self) -> &'static str {
        match self {
            EdgeError::Cycle(_) => "cycle",
            EdgeError::UnknownElement(_) | EdgeError::DanglingInput { .. } => "not_found",
            EdgeError::AlreadyExists(_) => "already_exists",
            EdgeError::DuplicateElement(_) => "duplicate_element",
            EdgeError::DuplicateEdge { .. } => "duplicate_edge",
            EdgeError::UnsortedInputs(_) => "unsorted_inputs",
            EdgeError::BadReference(_) => "bad_reference",
            EdgeError::UnsupportedSchema(_) => "unsupported_schema",
            EdgeError::BadParams(_) => "bad_params",
            EdgeError::BadEvent(_) => "bad_event",
            EdgeError::InputLimit(_) => "input_limit",
            EdgeError::Internal(_) => "internal",
        }
    }

    /// Stable reason: distinguishes the two `not_found` cases, equals the code otherwise.
    pub fn reason(&self) -> &'static str {
        match self {
            EdgeError::UnknownElement(_) => "unknown_element",
            EdgeError::DanglingInput { .. } => "dangling_input",
            other => other.code(),
        }
    }
}

/// Copies at most [`KEEP`] bytes of `s`, cut at a char boundary.
pub(crate) fn kept(s: &str) -> String {
    if s.len() <= KEEP {
        return s.to_string();
    }
    let mut end = KEEP;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    s.get(..end).unwrap_or("").to_string()
}

impl fmt::Display for EdgeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.code(), self.reason())?;
        match self {
            EdgeError::Cycle(ids) => {
                f.write_str(": [")?;
                for (i, id) in ids.iter().enumerate() {
                    if i > 0 {
                        f.write_str(", ")?;
                    }
                    write!(f, "{id}")?;
                }
                f.write_str("]")
            }
            EdgeError::UnknownElement(id)
            | EdgeError::AlreadyExists(id)
            | EdgeError::DuplicateElement(id)
            | EdgeError::UnsortedInputs(id) => write!(f, ": {id}"),
            EdgeError::DanglingInput { element, input } | EdgeError::DuplicateEdge { element, input } => {
                write!(f, ": {input} -> {element}")
            }
            EdgeError::BadReference(path) => write!(f, ": {path:?}"),
            EdgeError::UnsupportedSchema(s)
            | EdgeError::BadParams(s)
            | EdgeError::BadEvent(s)
            | EdgeError::InputLimit(s)
            | EdgeError::Internal(s) => write!(f, ": {s}"),
        }
    }
}

impl std::error::Error for EdgeError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_text_is_cut_at_a_char_boundary() {
        assert_eq!(kept("abc"), "abc");
        let long = "é".repeat(100);
        let k = kept(&long);
        assert!(k.len() <= KEEP && !k.is_empty());
        assert!(long.starts_with(&k));
    }
}
