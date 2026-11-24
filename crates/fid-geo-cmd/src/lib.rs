//! The GEO command adapter of Metrology Maven: the bridge between the pure geometry types
//! (`fid-geo-types`, kernel graph) and the project document. Spec node `GEO.TYPES.REG`
//! (`spec/geo/types/reg/NODE.md`).
//!
//! - [`register`]: [`register_element`] and [`replace_element`], called inside a producing
//!   command's transaction ([`GeoTx`]). IDs follow CONSTITUTION §5: an imported ID is kept,
//!   a new element gets a UUIDv7 from CORE, a derived element gets a UUIDv5 over the
//!   producing command ID and its sorted inputs with their digests (REQ-001 to REQ-003).
//! - [`describe`]: `geo.describe_element@1` (R0, D0), which reports an element's cached
//!   facts as canonical JSON and is recorded only in the Activity Log (REQ-004 to REQ-006).
//!
//! The CORE element tree (CORE.PRJ.TREE) has no API on main yet; the leaf's Open questions
//! assume a transaction handle with `insert`, `replace` and `get`, which is the
//! [`ElementTree`] trait here. Nothing in this crate panics or uses `unsafe`.

pub mod describe;
mod error;
pub mod frame_cmds;
pub mod index_cmds;
pub mod register;
pub mod sel_cmds;

use fid_registry::{CommandDef, DefError};

pub use describe::{
    describe_element, invoke_cli, invoke_mcp, ActivityEntry, DescribeParams, Surface, CLI_DESCRIBE, DESCRIBE_ELEMENT,
    MAX_PARAMS_BYTES, MAX_PARAMS_DEPTH, MCP_DESCRIBE,
};
pub use error::GeoCmdError;
pub use frame_cmds::{
    apply_transform, convert_units, create_frame, delete_frame, invoke_frame_cli, invoke_frame_mcp,
    replay_frame_journal, resolve_transform, set_transform, ApplyTransformResult, AuthoredLengthSpec, ConversionRecord,
    ConvertUnitsResult, CreateFrameParams, CreateFrameResult, DeleteFrameResult, FrameJournalEntry, FramePrincipal,
    FrameUsageIndex, ResolveTransformResult, RotationParam, SetTransformParams, SetTransformResult, APPLY_TRANSFORM,
    CLI_APPLY_TRANSFORM, CLI_CONVERT_UNITS, CLI_CREATE_FRAME, CLI_DELETE_FRAME, CLI_RESOLVE_TRANSFORM,
    CLI_SET_TRANSFORM, CONVERT_UNITS, CREATE_FRAME, DELETE_FRAME, MAX_FRAME_REFS_REPORTED, MCP_APPLY_TRANSFORM,
    MCP_CONVERT_UNITS, MCP_CREATE_FRAME, MCP_DELETE_FRAME, MCP_RESOLVE_TRANSFORM, MCP_SET_TRANSFORM, RESOLVE_TRANSFORM,
    SET_TRANSFORM,
};
pub use index_cmds::{
    build_index, invoke_index_cli, invoke_index_mcp, query_box, query_knn, query_radius, AuthoredLength,
    BuildIndexParams, BuildIndexResult, BuildKind, QueryBoxParams, QueryHit, QueryKnnParams, QueryPoints,
    QueryRadiusParams, QueryResult, SelectionPoints, SelectionStore, BUILD_INDEX, CLI_BUILD_INDEX, CLI_QUERY_BOX,
    CLI_QUERY_KNN, CLI_QUERY_RADIUS, MAX_INLINE_QUERIES, MAX_QUERY_PARAMS_BYTES, MAX_QUERY_PARAMS_DEPTH,
    MAX_RESULT_ENTRIES, MCP_BUILD_INDEX, MCP_QUERY_BOX, MCP_QUERY_KNN, MCP_QUERY_RADIUS, QUERY_BOX, QUERY_KNN,
    QUERY_RADIUS,
};
pub use register::{
    derive_id, imported_text_id, kind_name, register_element, replace_element, Derivation, ElementFacts, ElementTree,
    EntityId, GeoEntry, GeoTx, IdOrigin, OriginKind, Registered, Replaced, TreeEntry, DERIVED_TAG, IMPORTED_SOURCE_TAG,
    MAX_COUNT, MAX_OUTPUT_BYTES, MAX_SOURCE_ID_BYTES,
};
pub use sel_cmds::{
    combine_selection, create_selection, describe_selection, grow_or_shrink_selection, invoke_sel_cli, invoke_sel_mcp,
    parse_index_spec, replay_sel_journal, resolve_selection, CombineOp, CombineSelectionParams, CreateSelectionParams,
    DescribeSelectionResult, GrowParam, GrowSelectionParams, ResolveInput, ResolveSelectionParams, SelJournalEntry,
    SelectionRegistry, SelectionResult, StoredSelection, CLI_COMBINE_SELECTION, CLI_CREATE_SELECTION,
    CLI_DESCRIBE_SELECTION, CLI_GROW_SELECTION, CLI_RESOLVE_SELECTION, COMBINE_SELECTION, CREATE_SELECTION,
    DESCRIBE_SELECTION, GROW_SELECTION, MAX_INLINE_INDICES, MAX_SEL_PARAMS_BYTES, MAX_SEL_PARAMS_DEPTH,
    MCP_COMBINE_SELECTION, MCP_CREATE_SELECTION, MCP_DESCRIBE_SELECTION, MCP_GROW_SELECTION, MCP_RESOLVE_SELECTION,
    RESOLVE_SELECTION,
};

/// All registry command definitions owned by `fid-geo-cmd`: `geo.describe_element@1` (R0),
/// the four `GEO.INDEX.CMD` commands (R1, D0, MCP exposed), the six `GEO.FRAME.CMD`
/// commands (four W2 + two R1, all D0, MCP exposed), and the five `GEO.SEL.CMD` commands
/// (four W2 + one R0, all D0, MCP exposed).
pub fn command_defs() -> Result<Vec<CommandDef>, DefError> {
    let mut defs = describe::command_defs()?;
    defs.extend(index_cmds::command_defs()?);
    defs.extend(frame_cmds::command_defs()?);
    defs.extend(sel_cmds::command_defs()?);
    Ok(defs)
}
