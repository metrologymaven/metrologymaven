//! The project model of Metrology Maven. Modules land one spec leaf at a time; see `spec/core/prj/`.

pub mod container;
pub mod id;
pub mod lifecycle;
pub mod stage;
pub mod stage_switch;
pub mod tree;

pub use container::{
    create, Container, ContainerError, ContainerKind, ContainerView, CreatedBy, Hierarchy, Label, Part, Plan, Revision,
    Run, CONTAINER_CREATED, MAX_LABEL_CHARS,
};
pub use id::{
    live_source, uuid_v5, uuid_v7, Bits, IdError, IdIndex, IdService, Observation, ObservationSource, Recording,
    ReplaySource, SystemSource, Uuid, V7_RANDOM_BITS,
};
pub use stage::{
    add_stage, list_stages, remove_stage, reorder_stages, stage_list_id, Stage, StageListing, ADD_STAGE,
    DEFAULT_STAGE_LABEL, LIST_STAGES, REMOVE_STAGE, REORDER_STAGES, STAGES_REORDERED, STAGE_ADDED, STAGE_KIND,
    STAGE_LIST_KIND, STAGE_REMOVED,
};
pub use stage_switch::{
    set_active_stage, value_digest, MemoryValueCache, ReadStats, StageContext, ValueCache, ValueError, ValueStore,
    ACTIVE_STAGE_SET, SET_ACTIVE_STAGE,
};
pub use tree::{
    DomainTranslate, Element, ElementName, ElementState, ElementTypeDef, ElementTypeRegistry, Group, Slot, Tree,
    TreeError, TreeEvent, ELEMENT_CREATED, ELEMENT_SLOT_SET, GROUP_CREATED, GROUP_KIND, MAX_NAME_CHARS,
};
