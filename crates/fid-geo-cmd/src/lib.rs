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
pub mod index_cmds;
pub mod register;

use fid_registry::{CommandDef, DefError};

pub use describe::{
    describe_element, invoke_cli, invoke_mcp, ActivityEntry, DescribeParams, Surface, CLI_DESCRIBE, DESCRIBE_ELEMENT,
    MAX_PARAMS_BYTES, MAX_PARAMS_DEPTH, MCP_DESCRIBE,
};
pub use error::GeoCmdError;
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

/// All registry command definitions owned by `fid-geo-cmd`: `geo.describe_element@1` (R0)
/// and the four `GEO.INDEX.CMD` commands (`geo.build_index@1`, `geo.query_knn@1`,
/// `geo.query_radius@1`, `geo.query_box@1`; all R1, D0, MCP exposed).
pub fn command_defs() -> Result<Vec<CommandDef>, DefError> {
    let mut defs = describe::command_defs()?;
    defs.extend(index_cmds::command_defs()?);
    Ok(defs)
}
