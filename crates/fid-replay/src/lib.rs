//! Replay for Metrology Maven. Modules land one spec leaf at a time; see `spec/core/rpl/`.

pub mod bisect;
pub mod classify;

pub use bisect::{
    attribute, bisect, first_divergent, first_divergent_by_seq, report_fragment, report_value, Attribution,
    BisectError, BisectResult, Cause, EntryFacts, NO_INPUT_DIFFERENCE,
};
pub use classify::{
    aggregate, classify, Annotation, ClassifyError, Decision, Determinism, DeterminismExt, Limit, Output,
    ReplayOutcome, StepComparison, StepResult, Tolerance, ToleranceTable,
};
