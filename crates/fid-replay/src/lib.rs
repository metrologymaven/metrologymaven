//! Replay for Metrology Maven. Modules land one spec leaf at a time; see `spec/core/rpl/`.

pub mod bisect;
pub mod classify;
pub mod snapshot;
pub mod subst;

pub use bisect::{
    attribute, bisect, first_divergent, first_divergent_by_seq, report_fragment, report_value, Attribution,
    BisectError, BisectResult, Cause, EntryFacts, TaggedDigest, NO_INPUT_DIFFERENCE,
};
pub use classify::{
    aggregate, classify, Annotation, ClassifyError, Decision, Determinism, DeterminismExt, Limit, Output,
    ReplayOutcome, StepComparison, StepResult, Tolerance, ToleranceTable,
};
#[cfg(feature = "store")]
pub use snapshot::store_adapter::{ActivityWarningSink, StoreLogicalAncestors, StoreSnapshotCache, StoreSnapshotTable};
pub use snapshot::{
    content_digest, select_valid, take, CacheRead, DocState, EventBody, FoldError, LogicalAncestors,
    MemLogicalAncestors, MemSnapshotCache, MemSnapshotTable, SnapshotCache, SnapshotError, SnapshotPolicy,
    SnapshotRecord, SnapshotTable, SnapshotWarning, VecWarningSink, WarningSink, ACTIVITY_WARNING_COMMAND,
    MAX_ANCESTOR_WALK, SNAPSHOT_INTERVAL,
};
pub use subst::{
    preflight, BlobSource, CommandCatalog, KernelSource, MissingItem, MissingKind, Observation, ObservationKind,
    ObservationSource, RecordedObservation, RecordedSpan, RecordedStep, StepObservations, StepPayload, Storage,
    SubstError, MAX_INLINE_BYTES, MAX_KIND_LEN, MAX_OBSERVATIONS_PER_STEP, MAX_STEPS,
};
