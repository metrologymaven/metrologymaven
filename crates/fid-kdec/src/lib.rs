//! Kernel decision-rule types of Metrology Maven (spec node `UNC.DEC`). Pure Rust, no I/O,
//! no `unsafe`; kernel graph, KNP-1.
//!
//! Implemented leaves: `UNC.DEC.TYPE` (the [`DecisionRule`] value type, the rule-ID
//! grammar, the four-valued [`Outcome`], the built-in catalog `dr-catalog@1`, see [`rule`],
//! and the [`Decision`] record that carries the evidence basis of its uncertainty
//! statement, see [`decision`]) and `UNC.DEC.EVAL` (rule evaluation and conformance
//! probability, algorithm `dec.rule.evaluate@1.0.0`, see [`eval`]).
//!
//! Specific and global risk belong to `UNC.DEC.RISK`. Evaluation uses binary64 only for the
//! normal-approximation probability (no FMA, bit-identical on every target); every
//! comparison against a limit is exact.

pub mod decision;
pub mod eval;
pub mod rule;

pub use decision::{Decision, EvidenceBasis};
pub use eval::{evaluate, DecisionRecord, EvalError, EvalInput, Flag, PSource, Samples, Tolerance};
pub use rule::{
    Catalog, DecError, DecisionRule, NotDecidableReason, Outcome, ParamDecimal, PdfSource, RiskReporting, RuleId,
    RuleKind, RuleParams, RuleParts, RuleRef, StandardsRef,
};
