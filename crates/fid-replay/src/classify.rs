//! Replay outcome classification and decision invariance. Spec node `CORE.RPL.CLASS`.
//!
//! Rules, applied in order for one step (REQ-001..006):
//! 1. a missing input gives `NotReplayable`;
//! 2. any changed conformance decision or characteristic status gives `Divergent`;
//! 3. every output digest equal gives `Identical`;
//! 4. a D0/D1/D2 step on the same kernel digest and platform class that is not
//!    bit-identical gives `Divergent`;
//! 5. a differing output whose quantity kind has no tolerance gives `Divergent`
//!    with `ToleranceMissing`;
//! 6. every differing output within tolerance gives `Equivalent`, otherwise `Divergent`.
//!
//! `Marginal` (REQ-008) is computed independently and never changes the outcome.

use std::collections::BTreeMap;
use std::fmt;

/// Determinism class of the replayed step (CONSTITUTION §4); shared from `fid-types`.
pub use fid_types::Determinism;

/// Replay rules attached to a [`Determinism`] class.
pub trait DeterminismExt {
    /// Classes that must replay bit-identically on the same kernel and platform class.
    fn requires_bit_identity(self) -> bool;
}

impl DeterminismExt for Determinism {
    fn requires_bit_identity(self) -> bool {
        matches!(self, Determinism::D0 | Determinism::D1 | Determinism::D2)
    }
}

/// Outcome of one step or a whole replay. Ordered by severity for aggregation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ReplayOutcome {
    Identical,
    Equivalent,
    NotReplayable,
    Divergent,
}

impl ReplayOutcome {
    /// Wire name, e.g. `IDENTICAL`.
    pub fn as_str(self) -> &'static str {
        match self {
            ReplayOutcome::Identical => "IDENTICAL",
            ReplayOutcome::Equivalent => "EQUIVALENT",
            ReplayOutcome::NotReplayable => "NOT_REPLAYABLE",
            ReplayOutcome::Divergent => "DIVERGENT",
        }
    }
}

impl fmt::Display for ReplayOutcome {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Notes attached to a step result.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Annotation {
    /// A replayed value lies within the equivalence tolerance of a decision limit.
    Marginal,
    /// A differing output's quantity kind has no tolerance entry.
    ToleranceMissing(String),
}

/// One output of a step, recorded and replayed.
#[derive(Debug, Clone, PartialEq)]
pub struct Output {
    pub kind: String,
    pub recorded: f64,
    pub replayed: f64,
    pub recorded_digest: String,
    pub replayed_digest: String,
}

/// A conformance decision or characteristic status, recorded and replayed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decision {
    pub recorded: String,
    pub replayed: String,
}

/// A decision limit for a quantity kind.
#[derive(Debug, Clone, PartialEq)]
pub struct Limit {
    pub kind: String,
    pub value: f64,
}

/// Everything the classifier needs about one step.
#[derive(Debug, Clone, PartialEq)]
pub struct StepComparison {
    pub class: Determinism,
    /// Recording and replay ran under the same attested kernel: compute it with
    /// `fid_journal::same_attested_kernel`, which is false when either side records the
    /// reserved "no kernel attested" digest.
    pub same_kernel: bool,
    pub same_platform_class: bool,
    pub outputs: Vec<Output>,
    pub decisions: Vec<Decision>,
    pub limits: Vec<Limit>,
    /// Set when a step input is missing (the missing item's name).
    pub missing: Option<String>,
}

/// Equivalence tolerance: |recorded - replayed| <= abs + rel * max(|recorded|, |replayed|).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tolerance {
    pub abs: f64,
    pub rel: f64,
}

impl Tolerance {
    fn bound(&self, a: f64, b: f64) -> f64 {
        let (a, b) = (a.abs(), b.abs());
        let larger = if a >= b { a } else { b };
        self.abs + self.rel * larger
    }

    /// Whether two values are within this tolerance of each other.
    pub fn within(&self, a: f64, b: f64) -> bool {
        (a - b).abs() <= self.bound(a, b)
    }
}

/// Tolerances by quantity kind.
pub trait ToleranceTable {
    fn tolerance(&self, kind: &str) -> Option<Tolerance>;
}

impl ToleranceTable for BTreeMap<String, Tolerance> {
    fn tolerance(&self, kind: &str) -> Option<Tolerance> {
        self.get(kind).copied()
    }
}

/// The result for one step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StepResult {
    pub outcome: ReplayOutcome,
    /// Sorted, without duplicates.
    pub annotations: Vec<Annotation>,
}

/// Invalid input to the classifier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClassifyError {
    /// NaN or infinity in an output value, limit or tolerance.
    ComparisonInvalid(String),
}

impl ClassifyError {
    /// Stable code.
    pub fn code(&self) -> &'static str {
        match self {
            ClassifyError::ComparisonInvalid(_) => "comparison_invalid",
        }
    }
}

impl fmt::Display for ClassifyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ClassifyError::ComparisonInvalid(why) => write!(f, "comparison_invalid: {why}"),
        }
    }
}

impl std::error::Error for ClassifyError {}

fn finite(what: &str, x: f64) -> Result<(), ClassifyError> {
    if x.is_finite() {
        Ok(())
    } else {
        Err(ClassifyError::ComparisonInvalid(format!("{what} is not finite")))
    }
}

/// Classifies one replayed step.
pub fn classify(step: &StepComparison, table: &dyn ToleranceTable) -> Result<StepResult, ClassifyError> {
    for o in &step.outputs {
        finite(&format!("output {} recorded", o.kind), o.recorded)?;
        finite(&format!("output {} replayed", o.kind), o.replayed)?;
    }
    for l in &step.limits {
        finite(&format!("limit {}", l.kind), l.value)?;
    }
    // Every tolerance the step can use must be finite and non-negative.
    for kind in step.outputs.iter().map(|o| &o.kind).chain(step.limits.iter().map(|l| &l.kind)) {
        if let Some(t) = table.tolerance(kind) {
            if !(t.abs.is_finite() && t.rel.is_finite() && t.abs >= 0.0 && t.rel >= 0.0) {
                return Err(ClassifyError::ComparisonInvalid(format!("tolerance {kind} is not finite and >= 0")));
            }
        }
    }

    let mut annotations = Vec::new();
    // REQ-008: Marginal, independent of the outcome.
    'limits: for l in &step.limits {
        let Some(tol) = table.tolerance(&l.kind) else { continue };
        for o in step.outputs.iter().filter(|o| o.kind == l.kind) {
            if tol.within(o.replayed, l.value) {
                annotations.push(Annotation::Marginal);
                break 'limits;
            }
        }
    }

    let outcome = outcome_of(step, table, &mut annotations);
    annotations.sort();
    annotations.dedup();
    Ok(StepResult { outcome, annotations })
}

fn outcome_of(step: &StepComparison, table: &dyn ToleranceTable, notes: &mut Vec<Annotation>) -> ReplayOutcome {
    if step.missing.is_some() {
        return ReplayOutcome::NotReplayable;
    }
    // A changed decision or status is DIVERGENT regardless of the numbers (REQ-002,
    // CONSTITUTION §4), so it is checked before the identical-digest rule.
    if step.decisions.iter().any(|d| d.recorded != d.replayed) {
        return ReplayOutcome::Divergent;
    }
    let differing: Vec<&Output> = step.outputs.iter().filter(|o| o.recorded_digest != o.replayed_digest).collect();
    if differing.is_empty() {
        return ReplayOutcome::Identical;
    }
    if step.class.requires_bit_identity() && step.same_kernel && step.same_platform_class {
        return ReplayOutcome::Divergent;
    }
    let mut all_within = true;
    let mut missing_tol = false;
    for o in &differing {
        match table.tolerance(&o.kind) {
            None => {
                missing_tol = true;
                notes.push(Annotation::ToleranceMissing(o.kind.clone()));
            }
            Some(t) => {
                if !t.within(o.recorded, o.replayed) {
                    all_within = false;
                }
            }
        }
    }
    if missing_tol || !all_within {
        ReplayOutcome::Divergent
    } else {
        ReplayOutcome::Equivalent
    }
}

/// Aggregates step outcomes (REQ-007): any Divergent, else any NotReplayable,
/// else any Equivalent, else Identical. An empty replay is Identical.
pub fn aggregate(outcomes: &[ReplayOutcome]) -> ReplayOutcome {
    outcomes.iter().copied().max().unwrap_or(ReplayOutcome::Identical)
}
