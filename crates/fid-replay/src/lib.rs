//! Replay for Metrology Maven. Modules land one spec leaf at a time; see `spec/core/rpl/`.

pub mod classify;

pub use classify::{
    aggregate, classify, Annotation, ClassifyError, Decision, Determinism, DeterminismExt, Limit, Output,
    ReplayOutcome, StepComparison, StepResult, Tolerance, ToleranceTable,
};
