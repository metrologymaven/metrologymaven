//! The project model of Metrology Maven. Modules land one spec leaf at a time; see `spec/core/prj/`.

pub mod container;
pub mod id;
pub mod lifecycle;
pub mod meta;
pub mod query;
pub mod stage;
pub mod stage_switch;
pub mod tree;
pub mod tree_cmds;

pub use meta::{
    plan_set_metadata, plan_set_tags, run_find_elements_by_tag, validate_meta_value, MetaError, MetaEvent, MetaKey,
    MetaStore, MetaTarget, Tag, Untrusted, FIND_ELEMENTS_BY_TAG, MAX_KEY_BYTES, MAX_TAGS_PER_TARGET, MAX_TAG_CHARS,
    MAX_VALUE_BYTES, METADATA_SET, META_KEY_PATTERN, PROJECT_TARGET, SET_METADATA, SET_TAGS, TAGS_SET,
};

pub use container::{
    create, Container, ContainerError, ContainerKind, ContainerView, CreatedBy, Hierarchy, Label, Part, Plan, Revision,
    Run, CONTAINER_CREATED, MAX_LABEL_CHARS,
};
pub use id::{
    live_source, uuid_v5, uuid_v7, Bits, IdError, IdIndex, IdService, Observation, ObservationSource, Recording,
    ReplaySource, SystemSource, Uuid, V7_RANDOM_BITS,
};
pub use query::{
    describe_element, describe_element_tree, list_elements, list_elements_tree, ListParams, PageToken,
    DESCRIBE_ELEMENT, LIST_ELEMENTS, MAX_PAGE_SIZE, MAX_TOKEN_BYTES,
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
    group_list_id, tree_index_id, DomainTranslate, Element, ElementName, ElementState, ElementTypeDef,
    ElementTypeRegistry, Group, Slot, Tree, TreeError, TreeEvent, ELEMENTS_MOVED, ELEMENT_CREATED, ELEMENT_SLOT_SET,
    GROUP_CREATED, GROUP_KIND, GROUP_LIST_KIND, MAX_BATCH_IDS, MAX_GROUP_DEPTH, MAX_NAME_CHARS, RENAMED,
    TREE_INDEX_KIND, VISIBILITY_SET,
};
pub use tree_cmds::{
    create_group, move_element, rename_element, set_visibility, tree_root_id, CREATE_GROUP, MOVE_ELEMENT,
    RENAME_ELEMENT, SET_VISIBILITY,
};
