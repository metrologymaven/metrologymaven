//! The parametric dependency graph of Metrology Maven. Modules land one spec leaf at a
//! time; see `spec/core/dep/`.
//!
//! - [`edge`]: dependency edges, cycle rejection and dependency queries (`CORE.DEP.EDGE`).

pub mod edge;

pub use edge::{
    command_def, show_dependencies, Dependencies, EdgeError, EdgeIndex, EdgesSet, EntityId, Op, Output,
    ShowDependencies, EDGES_SET_EVENT, MAX_COMMIT_ELEMENTS, MAX_INPUTS, MAX_SCHEMA_DEPTH, MAX_WALK_NODES,
    SHOW_DEPENDENCIES,
};
