//! The project model of Metrology Maven. Modules land one spec leaf at a time; see `spec/core/prj/`.

pub mod container;
pub mod id;

pub use container::{
    command_defs, create, created_event, register, Container, ContainerError, ContainerKind, CreateHandler, Hierarchy,
    Label, Part, Plan, Revision, Run, SharedSource, CONTAINER_CREATED, MAX_LABEL_CHARS,
};
pub use id::{
    live_source, uuid_v5, uuid_v7, Bits, IdError, IdIndex, IdService, Observation, ObservationSource, Recording,
    ReplaySource, SystemSource, Uuid, V7_RANDOM_BITS,
};
