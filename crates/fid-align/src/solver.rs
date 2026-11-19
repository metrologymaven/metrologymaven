//! The solver-facing contract and the sorted solver registry (REQ-005, REQ-007).

use crate::element::{InputRef, ParamSet, ReportRef};
use crate::error::{kept, AlignError, MAX_CODE_BYTES};
use fid_geo_frame::Transform;
use fid_kci::AlgorithmId;
use std::collections::BTreeMap;
use std::fmt;

/// Most solvers one registry holds.
pub const MAX_SOLVERS: usize = 4096;

/// A solver's typed refusal, for example `degenerate_input`. The code matches
/// `^[a-z][a-z0-9_]*$` and is at most [`MAX_CODE_BYTES`] bytes, so it can be stored as an
/// alignment's `failure` and passed through unchanged (REQ-006).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SolverError {
    code: String,
}

impl SolverError {
    /// Builds an error; a code outside the grammar is refused with `bad_params`.
    pub fn new(code: &str) -> Result<SolverError, AlignError> {
        if code_ok(code) {
            Ok(SolverError { code: code.to_string() })
        } else {
            Err(AlignError::BadParams("solver error code must match ^[a-z][a-z0-9_]*$, at most 64 bytes"))
        }
    }

    /// The code.
    pub fn code(&self) -> &str {
        &self.code
    }
}

impl fmt::Display for SolverError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.code)
    }
}

/// `^[a-z][a-z0-9_]*$`, 1..=[`MAX_CODE_BYTES`] bytes.
pub(crate) fn code_ok(code: &str) -> bool {
    let mut b = code.bytes();
    code.len() <= MAX_CODE_BYTES
        && matches!(b.next(), Some(c) if c.is_ascii_lowercase())
        && b.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_')
}

/// The status a solver may report. `failed` and `outdated` are set by the system only: a
/// solver that fails returns a [`SolverError`] instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum SolveStatus {
    Ok,
    NotConverged,
    Ambiguous,
}

/// What a solver returns: the transform (already validated by its GEO.FRAME type), the
/// residual-report reference and the status.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SolverOutput {
    pub transform: Transform,
    pub report: ReportRef,
    pub status: SolveStatus,
}

/// The contract every alignment solver implements.
pub trait AlignmentSolver {
    /// The algorithm ID and version this solver implements.
    fn algorithm_id(&self) -> AlgorithmId;

    /// Solves from the inputs (in command order, each with its current content digest), the
    /// parameters, and the parent's transform if the alignment has a parent.
    fn solve(
        &self,
        inputs: &[InputRef],
        params: &ParamSet,
        parent_pose: Option<&Transform>,
    ) -> Result<SolverOutput, SolverError>;
}

/// The recompute table: solvers keyed by exact algorithm ID and version, in ascending
/// [`AlgorithmId`] order. A lookup never falls back to another version (REQ-007).
#[derive(Default)]
pub struct SolverRegistry {
    solvers: BTreeMap<AlgorithmId, Box<dyn AlignmentSolver>>,
}

impl SolverRegistry {
    pub fn new() -> SolverRegistry {
        SolverRegistry::default()
    }

    /// Registers a solver under its own [`AlignmentSolver::algorithm_id`]. Refuses a second
    /// solver for the same ID and version (`duplicate_algorithm`) and more than
    /// [`MAX_SOLVERS`] solvers.
    pub fn register(&mut self, solver: Box<dyn AlignmentSolver>) -> Result<(), AlignError> {
        let id = solver.algorithm_id();
        if self.solvers.contains_key(&id) {
            return Err(AlignError::DuplicateAlgorithm(kept(&id.to_string())));
        }
        if self.solvers.len() >= MAX_SOLVERS {
            return Err(AlignError::InputLimit("solvers"));
        }
        self.solvers.insert(id, solver);
        Ok(())
    }

    /// Removes the solver for exactly this ID and version; true if one was registered.
    pub fn remove(&mut self, id: &AlgorithmId) -> bool {
        self.solvers.remove(id).is_some()
    }

    /// The solver for exactly this ID and version.
    pub fn get(&self, id: &AlgorithmId) -> Option<&dyn AlignmentSolver> {
        self.solvers.get(id).map(|s| s.as_ref())
    }

    /// Registered IDs, ascending.
    pub fn ids(&self) -> impl Iterator<Item = &AlgorithmId> {
        self.solvers.keys()
    }

    pub fn len(&self) -> usize {
        self.solvers.len()
    }

    pub fn is_empty(&self) -> bool {
        self.solvers.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn solver_codes_follow_the_grammar() {
        assert!(code_ok("degenerate_input"));
        assert!(code_ok("a1"));
        for bad in ["", "Degenerate", "1abc", "a-b", "a b", &"a".repeat(65)] {
            assert!(!code_ok(bad), "{bad:?}");
            assert!(SolverError::new(bad).is_err());
        }
        assert_eq!(SolverError::new("x").map(|e| e.code().to_string()), Ok("x".to_string()));
    }
}
