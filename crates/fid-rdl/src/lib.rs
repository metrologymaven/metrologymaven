//! The Reference Data Library record format. Spec node `UNC.RDL.FORMAT`
//! (`spec/unc/rdl/format/NODE.md`); every format choice below is pinned in that node's
//! Design.
//!
//! - [`record`]: the `rdl-dataset/v1` schema and [`RdlDataset`]: bounded parsing (4 MiB,
//!   depth 32), validation in field order with typed errors naming the field, the RFC 8785
//!   canonical form (through `fid-canon`) and the dataset digest (REQ-001, -002, -005).
//! - [`view`]: [`public_view`], fail-closed redaction for `reference_only` records and tier B
//!   (REQ-003, -004).
//! - [`command`]: `rdl.list_dataset@1` and `rdl.show_dataset@1` (R0, D0), with one handler
//!   behind the CLI and MCP forms, an Activity Log record per call and exact replay (REQ-006).
//!
//! Nothing here panics; every input is bounded and every refusal is a typed [`RdlError`]
//! whose text never holds record content.

pub mod command;
mod error;
pub mod ext;
pub mod record;
pub mod tracim;
pub mod view;

pub use command::{
    command_defs, execute, failed_envelope, invoke_cli, invoke_mcp, list_dataset, replay, run, show_dataset,
    ActivityEntry, Call, CliOutput, ListParams, RdlStore, ShowParams, Surface, CLI_LIST, CLI_SHOW, LIST_DATASET,
    MAX_ENTRY_BYTES, MAX_PARAMS_BYTES, MAX_PARAMS_DEPTH, MAX_RECORDS, MCP_LIST, MCP_SHOW, SHOW_DATASET,
};
pub use error::RdlError;
pub use record::{
    aim_ok, is_control_or_format, licence_ok, licence_redistributable, parse_id, BlobRef, CrossCheck, CrossOutcome,
    GeneratorRecord, HexF64, Kind, Provenance, RdlDataset, RefOutput, Reference, Tier, Unit, Visibility, FORMAT_CHARS,
    MAX_DEPTH, MAX_RECORD_BYTES, REDISTRIBUTABLE, SCHEMA,
};
pub use view::{public_view, view_form, ViewForm};
