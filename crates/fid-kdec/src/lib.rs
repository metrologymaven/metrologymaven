//! Kernel decision-rule types of Metrology Maven (spec node `UNC.DEC`). Pure Rust, no I/O,
//! no `unsafe`; kernel graph, KNP-1.
//!
//! Implemented leaves: `UNC.DEC.TYPE` (the [`DecisionRule`] value type, the rule-ID
//! grammar, the four-valued [`Outcome`], the built-in catalog `dr-catalog@1`, see [`rule`],
//! and the [`Decision`] record that carries the evidence basis of its uncertainty
//! statement, see [`decision`]) and `UNC.DEC.EVAL` (rule evaluation and conformance
//! probability, algorithm `dec.rule.evaluate@1.0.0`, see [`eval`]).
//!
//! Specific and global conformity risk are implemented in [`risk`] (`UNC.DEC.RISK`,
//! algorithm `dec.risk.compute@1.0.0`). Evaluation uses binary64 only for the
//! normal-approximation probability and risk integrals (no FMA, bit-identical on every
//! target); every comparison against a limit is exact.

pub mod decision;
pub mod eval;
pub mod risk;
pub mod rule;

pub use decision::{Decision, EvidenceBasis};
pub use eval::{
    evaluate, evaluate_with_samples, DecisionRecord, EvalError, EvalInput, Flag, PSource, SampleBytes, Tolerance,
};
pub use risk::{
    acceptance_zone_for_rule, compute as compute_risk, global_risk, specific_risk, AcceptanceZone, Prior, RiskError,
    RiskFigures, RiskFlag, ToleranceF64, UntrustedString, ALGORITHM_ID as RISK_ALGORITHM_ID,
};
pub use rule::{
    Catalog, DecError, DecisionRule, NotDecidableReason, Outcome, ParamDecimal, PdfSource, RiskReporting, RuleId,
    RuleKind, RuleParams, RuleParts, RuleRef, StandardsRef,
};
