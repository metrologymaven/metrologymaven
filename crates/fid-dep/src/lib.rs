//! The parametric dependency graph of Metrology Maven. Modules land one spec leaf at a
//! time; see `spec/core/dep/`.
//!
//! - [`edge`]: dependency edges, cycle rejection and dependency queries (`CORE.DEP.EDGE`).
//! - [`prop`]: change propagation and recompute mode (`CORE.DEP.PROP`).

pub mod edge;
pub mod prop;

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
