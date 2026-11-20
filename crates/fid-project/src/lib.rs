//! The project model of Metrology Maven. Modules land one spec leaf at a time; see `spec/core/prj/`.

pub mod container;
pub mod id;
pub mod tree;

pub use container::{
    create, Container, ContainerError, ContainerKind, ContainerView, CreatedBy, Hierarchy, Label, Part, Plan, Revision,
    Run, CONTAINER_CREATED, MAX_LABEL_CHARS,
};
pub use id::{
    live_source, uuid_v5, uuid_v7, Bits, IdError, IdIndex, IdService, Observation, ObservationSource, Recording,
    ReplaySource, SystemSource, Uuid, V7_RANDOM_BITS,
};
pub use tree::{
    DomainTranslate, Element, ElementName, ElementState, ElementTypeDef, ElementTypeRegistry, Group, Slot, Tree,
    TreeError, TreeEvent, ELEMENT_CREATED, ELEMENT_SLOT_SET, GROUP_CREATED, GROUP_KIND, MAX_NAME_CHARS,
};
