//! ResultRecord assembly and decision binding (spec node `GDT.CHAR.RECORD`), and the read
//! command `gdt.get_result@1`.
//!
//! Every GD&T evaluation ends in exactly one [`ResultRecord`], built by the one
//! [`ResultRecordBuilder`] every evaluating child calls. The builder enforces, once:
//!
//! - the CONSTITUTION §5 minimum fields plus the DMT row ID, the operator-chain digest and the
//!   DMT digest (`incomplete_result{field}`, REQ-GDT.CHAR.RECORD-001);
//! - that GDT never decides conformance: the decision enters only as a typed
//!   [`fid_kdec::Decision`] returned by a UNC decision rule, and there is no API that sets an
//!   outcome (REQ-002, AC-7 below);
//! - `NOT_DECIDABLE` with reason `no_rule_bound` or `unc_no_decision` when no rule is bound
//!   or UNC returned no decision (REQ-003);
//! - UEC-0 carries no uncertainty value (`uec0_with_value`, REQ-004);
//! - `evaluation_basis` is stored exactly as declared and never changed (`basis_changed`,
//!   REQ-005).
//!
//! The record carries the evidence facts of its uncertainty statement (claimed, declared
//! input, uncharacterized input) in both its `uncertainty` and its `decision.basis`, so a
//! claimed value never looks measured. [`ResultStore`] holds one record per
//! (run, characteristic) on the branch derived from the principal (`duplicate_result`,
//! REQ-006), and [`get_result`] returns its canonical JSON (REQ-007).
//!
//! # Wire form (`fid.gdt-result/1`, RFC 8785)
//!
//! ```json
//! { "schema": "fid.gdt-result/1", "run": "R", "characteristic": "C", "measurand": "...",
//!   "evaluation_basis": "POINTS", "value": { "value": "0x<bits>", "unit": "si:m" },
//!   "uncertainty": { "class": "UEC-2", "uec0_reason": null, "method": "LPU",
//!                    "statement": "sha256:...", "unit": "si:m", "u_std": null,
//!                    "expanded": { "U": "0x<bits>", "k": "0x<bits>" }, "interval": null,
//!                    "claimed": false, "declared_input": false, "uncharacterized_input": false },
//!   "decision": { "source": "unc", "rule": "dr:simple-acceptance@1", "outcome": "CONFORMS",
//!                 "reason": null, "basis": { "class": "UEC-2", "method": "LPU", "claimed": false,
//!                 "declared_input": false, "uncharacterized_input": false,
//!                 "statement": "sha256:..." } },
//!   "spec": { "dialect": "ASME", "standards": [...], "defaults_profile": "dp:...",
//!             "inherited_from": "explicit" },
//!   "algorithm": "gdt.form.flatness.minzone@1.0.0", "kernel_version": "0.1.0",
//!   "input_digests": ["sha256:..."], "journal_entry": "sha256:...",
//!   "dmt_row": "dmt:asme/flatness/none@1", "operator_chain_digest": "sha256:...",
//!   "dmt_digest": "sha256:..." }
//! ```
//!
//! `decision.source` is `unc` for a decision UNC returned and `gdt_not_decidable` for the two
//! reasons GDT records itself. Numbers are IEEE-754 bit patterns (`"0x"` + 16 hex digits).
//!
//! # AC-7: no outcome without a UNC decision
//!
//! The builder has no method that takes an [`fid_kdec::Outcome`]; this does not build
//! (`tests/compile_fail/record_outcome.rs`):
//!
#![doc = "```compile_fail"]
#![doc = include_str!("../../../tests/compile_fail/record_outcome.rs")]
#![doc = "```"]
//!
//! Stable rustdoc does not check the error code of a `compile_fail` block, so the control
//! below makes the test specific: `tests/compile_fail/record_outcome_control.rs` is the same
//! file binding the decision through [`ResultRecordBuilder::bind_decision`], and it builds.
//! A test asserts the two files differ only in that line.
//!
#![doc = "```"]
#![doc = include_str!("../../../tests/compile_fail/record_outcome_control.rs")]
#![doc = "```"]
//!
//! Nor can a record decision be written as a literal:
//!
//! ```compile_fail
//! let d = fid_gdt::chars::record::RecordDecision { outcome: fid_kdec::Outcome::Conforms };
//! ```
//!
//! Nothing here panics on any input, and every input is bounded.

mod builder;
pub mod fields;
mod store;

pub use builder::{
    NotDecidableReason, RecordDecision, ResultRecord, ResultRecordBuilder, Uncertainty, DECISION_SOURCE_GDT,
    DECISION_SOURCE_UNC, RECORD_SCHEMA,
};
pub use fields::{AlgorithmRef, EvaluationBasis, InputDigests, KernelVersion, Measurand, Quantity, RunId};
pub use store::{get_result, ResultStore, GET_RESULT_COMMAND, MAX_RECORDS};

use std::fmt;

/// Errors of the builder, the store and `gdt.get_result@1`. [`RecordError::code`] is the
/// stable code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordError {
    /// `incomplete_result{field}`: a required field was never set.
    IncompleteResult {
        /// The first missing field, in the pinned order of [`ResultRecordBuilder::finish`].
        field: &'static str,
    },
    /// `uec0_with_value`: UEC-0 paired with an uncertainty value.
    Uec0WithValue,
    /// `basis_changed`: a second, different `evaluation_basis` in the same build.
    BasisChanged,
    /// `uncertainty_changed`: a second, different uncertainty in the same build.
    UncertaintyChanged,
    /// `field_changed{field}`: a second, different value for another field.
    FieldChanged {
        /// The field.
        field: &'static str,
    },
    /// `invalid_field{field}`: a value outside its grammar or range.
    InvalidField {
        /// The field.
        field: &'static str,
    },
    /// `duplicate_input_digest`: the input digests repeat a digest.
    DuplicateInputDigest,
    /// `unit_mismatch`: the value's unit differs from the uncertainty statement's unit.
    UnitMismatch,
    /// `decision_rule_mismatch`: the UNC decision names another rule than the bound one, or
    /// a decision came without a bound rule, or a bound rule with a rule-less decision.
    DecisionRuleMismatch,
    /// `decision_basis_mismatch`: the decision rests on another uncertainty statement than
    /// the record's, or states a side without one.
    DecisionBasisMismatch,
    /// `invalid_decision{reason}`: a decision no UNC rule could have returned (defence in
    /// depth over `fid-kdec`'s own checks).
    InvalidDecision {
        /// Static reason.
        reason: &'static str,
    },
    /// `duplicate_result`: a record for this (run, characteristic) is already committed.
    DuplicateResult,
    /// `not_found`: no record for this (run, characteristic) on the principal's branch.
    NotFound,
    /// `store_full`: the store holds [`MAX_RECORDS`] records.
    StoreFull,
    /// `canonical_encoding`: the record could not be written as canonical JSON.
    CanonicalEncoding,
}

impl RecordError {
    /// The stable code.
    pub const fn code(&self) -> &'static str {
        match self {
            RecordError::IncompleteResult { .. } => "incomplete_result",
            RecordError::Uec0WithValue => "uec0_with_value",
            RecordError::BasisChanged => "basis_changed",
            RecordError::UncertaintyChanged => "uncertainty_changed",
            RecordError::FieldChanged { .. } => "field_changed",
            RecordError::InvalidField { .. } => "invalid_field",
            RecordError::DuplicateInputDigest => "duplicate_input_digest",
            RecordError::UnitMismatch => "unit_mismatch",
            RecordError::DecisionRuleMismatch => "decision_rule_mismatch",
            RecordError::DecisionBasisMismatch => "decision_basis_mismatch",
            RecordError::InvalidDecision { .. } => "invalid_decision",
            RecordError::DuplicateResult => "duplicate_result",
            RecordError::NotFound => "not_found",
            RecordError::StoreFull => "store_full",
            RecordError::CanonicalEncoding => "canonical_encoding",
        }
    }

    /// The field or reason, where there is one.
    pub const fn detail(&self) -> Option<&'static str> {
        match self {
            RecordError::IncompleteResult { field }
            | RecordError::FieldChanged { field }
            | RecordError::InvalidField { field } => Some(field),
            RecordError::InvalidDecision { reason } => Some(reason),
            _ => None,
        }
    }
}

impl fmt::Display for RecordError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.detail() {
            Some(d) => write!(f, "{}{{{d}}}", self.code()),
            None => f.write_str(self.code()),
        }
    }
}

impl std::error::Error for RecordError {}
