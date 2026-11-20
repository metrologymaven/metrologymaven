//! Kernel decision-rule types of Metrology Maven (spec node `UNC.DEC`). Pure Rust, no I/O,
//! no `unsafe`, no floating-point arithmetic; kernel graph, KNP-1.
//!
//! Implemented leaves: `UNC.DEC.TYPE` (the [`DecisionRule`] value type, the rule-ID
//! grammar, the four-valued [`Outcome`], the built-in catalog `dr-catalog@1`, see [`rule`],
//! and the [`Decision`] record that carries the evidence basis of its uncertainty
//! statement, see [`decision`]).
//!
//! This crate evaluates nothing and compares nothing against a limit. Evaluation belongs to
//! `UNC.DEC.EVAL` and `UNC.DEC.RISK`; NODE.md Design pins the contract they inherit.

pub mod decision;
pub mod rule;

pub use decision::{Decision, EvidenceBasis};
pub use rule::{
    Catalog, DecError, DecisionRule, NotDecidableReason, Outcome, ParamDecimal, PdfSource, RiskReporting, RuleId,
    RuleKind, RuleParams, RuleParts, RuleRef, StandardsRef,
};
