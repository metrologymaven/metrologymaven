//! Replay for Metrology Maven. Modules land one spec leaf at a time; see `spec/core/rpl/`.

pub mod bisect;
pub mod classify;
pub mod subst;

pub use bisect::{
    attribute, bisect, first_divergent, first_divergent_by_seq, report_fragment, report_value, Attribution,
    BisectError, BisectResult, Cause, EntryFacts, TaggedDigest, NO_INPUT_DIFFERENCE,
};
pub use classify::{
    aggregate, classify, Annotation, ClassifyError, Decision, Determinism, DeterminismExt, Limit, Output,
    ReplayOutcome, StepComparison, StepResult, Tolerance, ToleranceTable,
};
pub use subst::{
    preflight, BlobSource, CommandCatalog, KernelSource, MissingItem, MissingKind, Observation, ObservationKind,
    ObservationSource, RecordedObservation, RecordedSpan, RecordedStep, StepObservations, StepPayload, Storage,
    SubstError, MAX_INLINE_BYTES, MAX_KIND_LEN, MAX_OBSERVATIONS_PER_STEP, MAX_STEPS,
};
