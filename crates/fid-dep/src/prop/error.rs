//! The error type of the propagation module: a closed set with stable codes and reasons.

use super::{ElementState, EntityId};
use std::fmt;

/// Every way propagation, marking or the recompute-mode command refuses an input. When one
/// is returned nothing was changed: no state, no event, no submission.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PropError {
    /// The affected part of the graph holds a cycle. The element IDs of one cycle, in edge
    /// direction, starting from the smallest ID.
    Cycle(Vec<EntityId>),
    /// A changed or recomputed element is not in the dependency graph.
    UnknownElement(EntityId),
    /// An element the pass must read has no state in the state table.
    MissingState(EntityId),
    /// The same element is listed twice in `changed` or in `recomputed`.
    DuplicateElement(EntityId),
    /// A recomputed element is neither changed nor downstream of a changed element, so it
    /// cannot have been recomputed in this pass.
    NotInPass(EntityId),
    /// Applying a mark would move `element` out of a state other than `current` or `failed`
    /// (CORE.PRJ.TREE.MODEL allows only current -> outdated and failed -> outdated).
    InvalidTransition { element: EntityId, from: ElementState },
    /// `project.set_recompute_mode@1` parameters are invalid. Holds a reason.
    Validation(&'static str),
    /// An event does not have the pinned shape. Holds a reason.
    BadEvent(&'static str),
    /// A size budget was exceeded. Holds the name of the budget.
    InputLimit(&'static str),
    /// A fault that cannot be the caller's mistake (an internal invariant did not hold).
    Internal(&'static str),
}

impl PropError {
    /// Stable code.
    pub fn code(&self) -> &'static str {
        match self {
            PropError::Cycle(_) => "cycle",
            PropError::UnknownElement(_) | PropError::MissingState(_) => "not_found",
            PropError::DuplicateElement(_) => "duplicate_element",
            PropError::NotInPass(_) => "not_in_pass",
            PropError::InvalidTransition { .. } => "invalid_transition",
            PropError::Validation(_) => "validation",
            PropError::BadEvent(_) => "bad_event",
            PropError::InputLimit(_) => "input_limit",
            PropError::Internal(_) => "internal",
        }
    }

    /// Stable reason: distinguishes the two `not_found` cases, equals the code otherwise.
    pub fn reason(&self) -> &'static str {
        match self {
            PropError::UnknownElement(_) => "unknown_element",
            PropError::MissingState(_) => "missing_state",
            other => other.code(),
        }
    }

    /// The element IDs the error names, in the order shown (empty for the text-only cases).
    pub fn elements(&self) -> Vec<EntityId> {
        match self {
            PropError::Cycle(ids) => ids.clone(),
            PropError::UnknownElement(id)
            | PropError::MissingState(id)
            | PropError::DuplicateElement(id)
            | PropError::NotInPass(id)
            | PropError::InvalidTransition { element: id, .. } => vec![*id],
            PropError::Validation(_) | PropError::BadEvent(_) | PropError::InputLimit(_) | PropError::Internal(_) => {
                Vec::new()
            }
        }
    }
}

impl fmt::Display for PropError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.code(), self.reason())?;
        match self {
            PropError::Cycle(ids) => {
                f.write_str(": [")?;
                for (i, id) in ids.iter().enumerate() {
                    if i > 0 {
                        f.write_str(", ")?;
                    }
                    write!(f, "{id}")?;
                }
                f.write_str("]")
            }
            PropError::UnknownElement(id)
            | PropError::MissingState(id)
            | PropError::DuplicateElement(id)
            | PropError::NotInPass(id) => write!(f, ": {id}"),
            PropError::InvalidTransition { element, from } => write!(f, ": {element} is {}", from.as_str()),
            PropError::Validation(s) | PropError::BadEvent(s) | PropError::InputLimit(s) | PropError::Internal(s) => {
                write!(f, ": {s}")
            }
        }
    }
}

impl std::error::Error for PropError {}
