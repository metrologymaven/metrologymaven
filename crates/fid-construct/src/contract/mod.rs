//! The construction contract. Spec node `KRN.CONS.DECL`.
//!
//! A construction is a pure function of declared input elements and parameters
//! (REQ-KRN.CONS-003). Every construction implements [`Construction`]:
//!
//! - [`Construction::declare`] publishes, as plain data, what it reads: a
//!   [`DependencyDecl`] with its algorithm ID ([`ConstructAlgorithmId`],
//!   `construct.<op>.<variant>@<semver>`), its ordered input references ([`ElementRef`]:
//!   element ID and role) and the digest of its [`ConstructParams`] (REQ-001, REQ-002). The
//!   declaration has one canonical byte form; `CORE.DEP` consumes it without the kernel
//!   depending on `CORE`.
//! - [`Construction::evaluate`] computes a [`ConstructedElement`] (a feature, or a value
//!   with quantity kind and unit) from [`Inputs`] bound to that declaration, or returns a
//!   typed [`ConstructError`] and no element (REQ-003). Undefined and ill-conditioned
//!   constructions are told apart by pinned rules (see [`ConstructError`]).
//!
//! [`run`] is the driver: declare, bind the supplied geometry (count and validity
//! checked), evaluate, digest the output. An [`Evaluation`] yields the `construct.*`
//! journal event ([`ConstructEvent`], REQ-006). [`harness::check`] tests any construction for
//! repeat and replace-then-evaluate bit identity (REQ-005). [`CommandRow`] is the data row
//! that declares a `construct.<verb_object>@1` command (REQ-006).
//!
//! Every angular and distance threshold is a field of [`ConstructParams`] and part of the
//! parameter digest (REQ-004). Canonical bytes are RFC 8785 JSON with numbers as IEEE-754
//! hex strings, identical on x86-64, ARM64 and wasm32 (D0). Nothing here panics, holds a
//! hash map or uses FMA, `f64::min` or `f64::max` (KNP-1).

mod algorithm;
mod decl;
mod error;
mod event;
mod geom;
pub mod harness;
mod id;
mod inputs;
pub(crate) mod json;
mod params;
mod rows;

pub use algorithm::{ConstructAlgorithmId, MODULE};
pub use decl::{Declared, DependencyDecl, DECL_FORMAT, MAX_INPUTS};
pub use error::{Condition, ConstructError, Measure, UndefinedReason};
pub use event::ConstructEvent;
pub use geom::{ConstructedElement, ElementGeom, InputKind, Scalar, ELEMENT_FORMAT};
pub use id::{ElementId, ElementRef, Role, MAX_NAME_LEN};
pub use inputs::Inputs;
pub use params::{ConstructParams, QuantityKind, MAX_VALUES, PARAMS_FORMAT};
pub use rows::{
    is_construct_command, validate_fields, CommandRow, Field, RowError, ValueType, COMMON_PARAMS, JOURNAL_FIELDS,
    MAX_FIELD_NAME, ROW_FORMAT,
};

use fid_types::{CommandId, Digest};

/// A construction (REQ-KRN.CONS.DECL-001). The value holds only configuration fixed when it
/// was built (its [`Declared`] input references and options), never geometry or a cache.
/// It cannot read undeclared supplied inputs; geometry captured in the construction value at
/// `make()` is a contract violation the harness cannot detect. [`harness::check`] detects
/// state that changes between calls.
pub trait Construction {
    /// The dependency declaration under `params` (normally
    /// `DependencyDecl::new(&self.declared, params)`).
    fn declare(&self, params: &ConstructParams) -> DependencyDecl;

    /// Computes the element from the inputs bound to [`Construction::declare`]'s
    /// declaration (declared order). Reads inputs only through `inputs` and thresholds only
    /// through `params`. Returns an error and no element when the construction is undefined
    /// or ill-conditioned.
    fn evaluate(&self, inputs: &Inputs<'_>, params: &ConstructParams) -> Result<ConstructedElement, ConstructError>;
}

/// A successful evaluation: the declaration it ran under, the element and its digest.
/// Fields are private; only [`run`] builds one.
#[derive(Debug, Clone, PartialEq)]
pub struct Evaluation {
    decl: DependencyDecl,
    output: ConstructedElement,
    output_digest: Digest,
}

impl Evaluation {
    /// The declaration.
    pub fn decl(&self) -> &DependencyDecl {
        &self.decl
    }

    /// The constructed element.
    pub fn output(&self) -> &ConstructedElement {
        &self.output
    }

    /// The output digest.
    pub fn output_digest(&self) -> &Digest {
        &self.output_digest
    }

    /// The journal event for committing this evaluation as `command`.
    /// [`ConstructError::InvalidDecl`] (`command`) unless `command` is a construction
    /// command ([`is_construct_command`]).
    pub fn event(&self, command: &CommandId) -> Result<ConstructEvent, ConstructError> {
        if !is_construct_command(command) {
            return Err(ConstructError::InvalidDecl { field: "command" });
        }
        Ok(ConstructEvent::new(
            command.clone(),
            self.decl.algorithm().clone(),
            self.decl.inputs().to_vec(),
            *self.decl.params_digest(),
            self.output_digest,
        ))
    }
}

/// Runs `c` on `inputs` (declared order) under `params`:
///
/// 1. `decl = c.declare(params)`; [`ConstructError::InvalidDecl`] (`params_digest`) when its
///    parameter digest is not the digest of `params`;
/// 2. [`Inputs::bind`] (so [`ConstructError::InputCount`] when the number of inputs differs
///    from the declaration, [`ConstructError::InvalidFeature`] for an invalid input);
/// 3. `c.evaluate`; its error is returned unchanged;
/// 4. the output digest.
pub fn run<C: Construction + ?Sized>(
    c: &C,
    inputs: &[ElementGeom<'_>],
    params: &ConstructParams,
) -> Result<Evaluation, ConstructError> {
    let decl = c.declare(params);
    if decl.params_digest() != &params.digest() {
        return Err(ConstructError::InvalidDecl { field: "params_digest" });
    }
    let output = {
        let bound = Inputs::bind(&decl, inputs)?;
        c.evaluate(&bound, params)?
    };
    let output_digest = output.digest();
    Ok(Evaluation { decl, output, output_digest })
}
