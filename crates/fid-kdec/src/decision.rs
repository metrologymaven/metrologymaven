//! A decision record: the outcome, the rule that produced it and the evidence basis of the
//! uncertainty statement it rests on. Spec node `UNC.DEC.TYPE` (Design, "Decision").
//!
//! The evidence basis is taken from a validated [`UncertaintyStatement`] (`fid-kunc`,
//! `UNC.UEC.STMT`) and nowhere else, so the facts surfaces must show (the value is
//! *claimed* (`DECLARED`, `SPEC_DERIVED`), rests on a *declared input*, or rests on an
//! *uncharacterized input*) travel with every decision and cannot be dropped or set by
//! hand. A claimed value never looks measured: [`Decision::is_claimed`] stays true on a
//! `CONFORMS` built on it.
//!
//! # Checks of [`Decision::new`] (pinned order, error `outcome_mismatch{field}`)
//!
//! 1. `rule_missing`: no rule exactly when the outcome is `NOT_DECIDABLE(rule_missing)`.
//! 2. `label_only`: a `label_only` rule exactly when the outcome is
//!    `NOT_DECIDABLE(label_only)` (a transcribed result is never a kernel decision).
//! 3. Unbound template (error `invalid_rule{r}`): the guard-band template
//!    `dr:guard-band@v` (`r` unbound) is refused with every outcome except
//!    `NOT_DECIDABLE(rule_missing | label_only)`, which checks 1 and 2 already refuse for
//!    it; only a resolved instance `dr:guard-band(r=x)@v` can carry an outcome.
//! 4. `u_missing`: `NOT_DECIDABLE(u_missing)` only without a basis or on a UEC-0 basis.
//! 5. `basis`: `CONFORMS`, `NONCONFORMS` and `INDETERMINATE` need a basis of class UEC-1..3.
//! 6. `pdf_unavailable`: `NOT_DECIDABLE(pdf_unavailable)` only under a
//!    `conformance_probability` rule.
//!
//! [`EvidenceBasis::of`] refuses a statement whose decision-relevant numbers (`u_std`, `U`,
//! `k`, interval ends and `p`) are subnormal, with `invalid_number{field}`; `fid-kunc`
//! already refuses NaN and infinities.

use fid_kunc::{EvidenceClass, Method, UncertaintyStatement};
use fid_types::Digest;

use crate::rule::{DecError, DecisionRule, NotDecidableReason, Outcome, PdfSource, RuleKind, RuleParams, RuleRef};

/// The facts of an uncertainty statement that every decision on it carries.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct EvidenceBasis {
    class: EvidenceClass,
    method: Option<Method>,
    claimed: bool,
    declared_input: bool,
    uncharacterized_input: bool,
    statement: Digest,
}

fn usable(x: f64) -> bool {
    x.is_finite() && !x.is_subnormal()
}

impl EvidenceBasis {
    /// Takes the basis from a validated statement. Fails with `invalid_number{field}` if a
    /// decision-relevant number is NaN, infinite or subnormal (checked in the order
    /// `u_std`, `expanded.U`, `expanded.k`, `interval.low`, `interval.high`, `interval.p`).
    pub fn of(stmt: &UncertaintyStatement) -> Result<EvidenceBasis, DecError> {
        let e = stmt.expanded();
        let i = stmt.interval();
        let numbers = [
            ("u_std", stmt.u_std()),
            ("expanded.U", e.map(|e| e.value)),
            ("expanded.k", e.map(|e| e.k)),
            ("interval.low", i.map(|i| i.low)),
            ("interval.high", i.map(|i| i.high)),
            ("interval.p", i.map(|i| i.p)),
        ];
        for (field, x) in numbers {
            if x.is_some_and(|x| !usable(x)) {
                return Err(DecError::InvalidNumber { field });
            }
        }
        Ok(EvidenceBasis {
            class: stmt.class(),
            method: stmt.method(),
            claimed: stmt.is_claimed(),
            declared_input: stmt.rests_on_declared_input(),
            uncharacterized_input: stmt.rests_on_uncharacterized_input(),
            statement: stmt.digest(),
        })
    }

    /// The evidence class.
    pub fn class(&self) -> EvidenceClass {
        self.class
    }
    /// The evaluation method tag.
    pub fn method(&self) -> Option<Method> {
        self.method
    }
    /// The value was declared or derived from a specification, not evaluated.
    pub fn is_claimed(&self) -> bool {
        self.claimed
    }
    /// The value rests on at least one declared input.
    pub fn rests_on_declared_input(&self) -> bool {
        self.declared_input
    }
    /// The value rests on at least one uncharacterized input.
    pub fn rests_on_uncharacterized_input(&self) -> bool {
        self.uncharacterized_input
    }
    /// Digest of the statement.
    pub fn statement(&self) -> Digest {
        self.statement
    }
}

/// A checked decision: outcome, rule reference and evidence basis.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Decision {
    rule: Option<RuleRef>,
    outcome: Outcome,
    basis: Option<EvidenceBasis>,
}

fn mismatch(ok: bool, field: &'static str) -> Result<(), DecError> {
    if ok {
        Ok(())
    } else {
        Err(DecError::OutcomeMismatch { field })
    }
}

impl Decision {
    /// Checks and builds a decision (see the module documentation for the order).
    pub fn new(
        rule: Option<&DecisionRule>,
        outcome: Outcome,
        basis: Option<EvidenceBasis>,
    ) -> Result<Decision, DecError> {
        let kind = rule.map(DecisionRule::kind);
        let has_u = basis.as_ref().is_some_and(|b| b.class != EvidenceClass::Uec0);
        mismatch(
            rule.is_none() == (outcome == Outcome::NotDecidable(NotDecidableReason::RuleMissing)),
            "rule_missing",
        )?;
        mismatch(
            (kind == Some(RuleKind::LabelOnly)) == (outcome == Outcome::NotDecidable(NotDecidableReason::LabelOnly)),
            "label_only",
        )?;
        // The unbound guard-band template names no guard band, so no outcome under it is
        // reproducible (checks 1 and 2 already refuse the two `NOT_DECIDABLE` reasons that
        // would be exempt).
        let unbound = rule.is_some_and(|r| matches!(r.params(), RuleParams::GuardBand { r: None }));
        let exempt =
            matches!(outcome, Outcome::NotDecidable(NotDecidableReason::RuleMissing | NotDecidableReason::LabelOnly));
        if unbound && !exempt {
            return Err(DecError::InvalidRule { param: "r" });
        }
        match outcome {
            Outcome::NotDecidable(NotDecidableReason::UMissing) => mismatch(!has_u, "u_missing")?,
            Outcome::Conforms | Outcome::Nonconforms | Outcome::Indeterminate => mismatch(has_u, "basis")?,
            Outcome::NotDecidable(NotDecidableReason::PdfUnavailable) => mismatch(
                kind == Some(RuleKind::ConformanceProbability)
                    && rule.is_some_and(|r| r.pdf_source() != PdfSource::NotUsed),
                "pdf_unavailable",
            )?,
            Outcome::NotDecidable(NotDecidableReason::RuleMissing | NotDecidableReason::LabelOnly) => {}
        }
        Ok(Decision { rule: rule.map(DecisionRule::rule_ref), outcome, basis })
    }

    /// The outcome.
    pub fn outcome(&self) -> Outcome {
        self.outcome
    }
    /// The rule reference (`None` exactly for `NOT_DECIDABLE(rule_missing)`).
    pub fn rule(&self) -> Option<&RuleRef> {
        self.rule.as_ref()
    }
    /// The evidence basis.
    pub fn basis(&self) -> Option<&EvidenceBasis> {
        self.basis.as_ref()
    }
    /// The decision rests on a claimed (declared or specification-derived) uncertainty.
    /// Surfaces must label it; it is never presented as measured.
    pub fn is_claimed(&self) -> bool {
        self.basis.as_ref().is_some_and(EvidenceBasis::is_claimed)
    }
    /// The decision rests on an uncertainty with at least one declared input.
    pub fn rests_on_declared_input(&self) -> bool {
        self.basis.as_ref().is_some_and(EvidenceBasis::rests_on_declared_input)
    }
    /// The decision rests on an uncertainty with at least one uncharacterized input.
    pub fn rests_on_uncharacterized_input(&self) -> bool {
        self.basis.as_ref().is_some_and(EvidenceBasis::rests_on_uncharacterized_input)
    }
}
