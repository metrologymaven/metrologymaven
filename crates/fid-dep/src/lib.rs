//! The parametric dependency graph of Metrology Maven. Modules land one spec leaf at a
//! time; see `spec/core/dep/`.
//!
//! - [`edge`]: dependency edges, cycle rejection and dependency queries (`CORE.DEP.EDGE`).
//! - [`prop`]: change propagation and recompute mode (`CORE.DEP.PROP`).
//! - [`recompute`]: deterministic recompute with failure isolation and single-entry commit (`CORE.DEP.RECOMP`).
//! - [`replace`]: input replacement preserving target identity and metadata (`CORE.DEP.REPL`).

pub mod edge;
pub mod prop;
pub mod recompute;
pub mod replace;

pub use edge::{
    command_def, show_dependencies, Dependencies, EdgeError, EdgeIndex, EdgesSet, EntityId, Op, Output,
    ShowDependencies, EDGES_SET_EVENT, MAX_COMMIT_ELEMENTS, MAX_INPUTS, MAX_SCHEMA_DEPTH, MAX_WALK_NODES,
    SHOW_DEPENDENCIES,
};
pub use prop::{
    is_markable, plan, propagate, set_command_def, set_recompute_mode, Adjacency, DepGraph, ElementState,
    ElementStates, ElementsMarked, Marking, PropError, Propagation, RecomputeMode, RecomputeModeSet,
    RecomputeSubmission, SetRecomputeMode, ELEMENTS_MARKED_EVENT, MAX_CHANGED, MAX_PASS_EDGES, MAX_PASS_ELEMENTS,
    RECOMPUTE_DEPENDENTS, RECOMPUTE_MODE_SET_EVENT, SET_RECOMPUTE_MODE,
};
pub use recompute::{
    command_def as recompute_command_def, execute_recompute, order_induced_subgraph, plan_recompute, replay_exact,
    CancelCheck, ElementOutcome, ElementRunOutput, FailedEntry, NeverCancel, NullProgress, OverlayEntity, OverlayEvent,
    OverlayView, ProgressReport, RecompError, RecomputeCommitted, RecomputeExecution, RecomputePlan, RecomputeRunner,
    RecomputeScope, RecomputedEntry, RecordedCommand, ReplayOutcome, ELEMENT_STATE_CHANGED_EVENT,
    RECOMPUTE_COMMAND_NAME, RECOMPUTE_COMMITTED_EVENT,
};
pub use replace::{
    apply_recompute_to_elements, command_def as replace_command_def, overlay_from_elements, replace_input,
    ElementRemoved, InputReplaced, ReplaceError, ReplaceInputParams, ReplaceOutcome, TreeElement,
    ELEMENT_REMOVED_EVENT, INPUT_REPLACED_EVENT, REPLACE_INPUT, REPLACE_INPUT_NAME,
};
