//! Dependency declarations (REQ-KRN.CONS.DECL-002, REQ-KRN.CONS-003).
//!
//! A [`DependencyDecl`] is plain data: the algorithm ID, the ordered input references
//! (element ID and role) and the parameter digest. `CORE.DEP` consumes it without any crate
//! dependency from the kernel to `CORE`: each input ID in the canonical bytes is the
//! canonical 36-character lowercase UUID text that `fid_dep` parses for entity IDs, and
//! [`DependencyDecl::edge_inputs`] gives the strictly ascending, duplicate-free input list
//! that `fid_dep::EdgesSet::new` requires.
//!
//! # Pinned canonical form
//!
//! RFC 8785 canonical JSON, keys sorted:
//!
//! ```text
//! {"algorithm":"construct.<op>.<variant>@<semver>","format":"construct.dependency_decl@1",
//!  "inputs":[{"id":"<uuid>","role":"<role>"},...],"params_digest":"sha256:<hex>"}
//! ```
//!
//! (shown wrapped). `inputs` keeps the declared order: it is the order of the `inputs`
//! slice the construction is evaluated on.

use fid_types::Digest;

use super::algorithm::ConstructAlgorithmId;
use super::error::ConstructError;
use super::id::{ElementId, ElementRef};
use super::json::{self, J};
use super::params::ConstructParams;

/// The format tag inside the canonical declaration bytes.
pub const DECL_FORMAT: &str = "construct.dependency_decl@1";

/// Most inputs one construction may declare (the per-element input limit of `CORE.DEP`,
/// `fid_dep::MAX_INPUTS`).
pub const MAX_INPUTS: usize = 4096;

/// What a construction reads, before parameters: its algorithm and its ordered input
/// references. Built once by the construction (fallibly) and kept, so that
/// [`super::Construction::declare`] cannot fail. Fields are private: at most
/// [`MAX_INPUTS`] inputs, roles unique.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Declared {
    algorithm: ConstructAlgorithmId,
    inputs: Vec<ElementRef>,
}

impl Declared {
    /// Checks and wraps. [`ConstructError::InvalidDecl`] with field `inputs` (more than
    /// [`MAX_INPUTS`]) or `inputs.role` (two inputs with one role). The same element may
    /// appear under two roles.
    pub fn new(algorithm: ConstructAlgorithmId, inputs: &[ElementRef]) -> Result<Declared, ConstructError> {
        if inputs.len() > MAX_INPUTS {
            return Err(ConstructError::InvalidDecl { field: "inputs" });
        }
        let mut roles = Vec::new();
        roles.try_reserve_exact(inputs.len()).map_err(|_| ConstructError::InvalidDecl { field: "inputs" })?;
        roles.extend(inputs.iter().map(|r| r.role()));
        roles.sort_unstable();
        if roles.windows(2).any(|w| w[0] == w[1]) {
            return Err(ConstructError::InvalidDecl { field: "inputs.role" });
        }
        let mut v = Vec::new();
        v.try_reserve_exact(inputs.len()).map_err(|_| ConstructError::InvalidDecl { field: "inputs" })?;
        v.extend_from_slice(inputs);
        Ok(Declared { algorithm, inputs: v })
    }

    /// The algorithm.
    pub fn algorithm(&self) -> &ConstructAlgorithmId {
        &self.algorithm
    }

    /// The input references, in declared order.
    pub fn inputs(&self) -> &[ElementRef] {
        &self.inputs
    }
}

/// A dependency declaration: a [`Declared`] plus the digest of the parameters it was
/// declared with. Fields are private; the digest is computed here, never supplied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DependencyDecl {
    declared: Declared,
    params_digest: Digest,
}

impl DependencyDecl {
    /// Declares `declared` under `params`.
    pub fn new(declared: &Declared, params: &ConstructParams) -> DependencyDecl {
        DependencyDecl { declared: declared.clone(), params_digest: params.digest() }
    }

    /// The algorithm.
    pub fn algorithm(&self) -> &ConstructAlgorithmId {
        &self.declared.algorithm
    }

    /// The input references, in declared order.
    pub fn inputs(&self) -> &[ElementRef] {
        &self.declared.inputs
    }

    /// The parameter digest.
    pub fn params_digest(&self) -> &Digest {
        &self.params_digest
    }

    /// The input element IDs, strictly ascending and without duplicates: the incoming edges
    /// of the constructed element in `CORE.DEP`.
    pub fn edge_inputs(&self) -> Vec<ElementId> {
        let mut ids: Vec<ElementId> = self.declared.inputs.iter().map(|r| r.id()).collect();
        ids.sort_unstable();
        ids.dedup();
        ids
    }

    /// The canonical bytes (module documentation).
    pub fn canonical_bytes(&self) -> Vec<u8> {
        json::write(&J::O(vec![
            ("algorithm", J::S(self.declared.algorithm.to_string())),
            ("format", json::s(DECL_FORMAT)),
            ("inputs", refs_json(&self.declared.inputs)),
            ("params_digest", json::s(self.params_digest.as_str())),
        ]))
    }
}

/// `[{"id":"<uuid>","role":"<role>"},...]` in the given order.
pub(crate) fn refs_json(refs: &[ElementRef]) -> J {
    J::A(
        refs.iter().map(|r| J::O(vec![("id", J::S(r.id().to_text())), ("role", json::s(r.role().as_str()))])).collect(),
    )
}
