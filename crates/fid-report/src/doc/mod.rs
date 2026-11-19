//! Report document model and storage. Spec node `RPT.MODEL.DOC`.
//!
//! A [`Report`] is an ordered list of [`Page`]s; a page is an ordered list of typed
//! [`Block`]s. Every report, page and block has a UUIDv7 that survives save, reload and
//! replay (REQ-001, REQ-008, REQ-011), and a removed page or block ID is retired and never
//! issued again in that report (REQ-004).
//!
//! - [`model`]: the types, their bounds and their RFC 8785 canonical form.
//! - [`text`]: [`Untrusted<Text>`](Untrusted), the bounded, untrusted user string (REQ-009, REQ-010).
//! - [`kind`]: the [`BlockKind`] trait and the [`KindRegistry`] (REQ-007).
//! - [`command`]: the six registry commands and the `--json` dispatcher [`execute`].
//! - [`event`]: the journal events, each holding every ID its command created.
//! - [`backend`]: the store-and-journal seam ([`ReportBackend`], [`MemoryBackend`]).
//! - [`replay`]: exact replay ([`replay_exact`], AC-7).
//! - [`defs`]: the registry definitions ([`command_defs`]).
//!
//! - [`value`]: declared value fields hold `value_binding@1` references, never literal
//!   values; DOC never formats, computes or trusts a value or its labels.
//!
//! Binding resolution belongs to `RPT.MODEL.BIND`; value text, its per-value labels and
//! its side belong to `RPT.MODEL.FMT`.

pub mod backend;
pub mod command;
pub mod defs;
pub mod error;
pub mod event;
pub mod kind;
pub mod model;
pub mod replay;
#[cfg(feature = "test-kinds")]
pub mod stub;
pub mod text;
mod unicode16;
pub mod value;

pub use backend::{MemoryBackend, ReportBackend, StoredReport};
pub use command::{
    add_block, add_page, create_report, execute, remove_block, remove_page, show_report, summary_value, AddBlock,
    AddPage, CreateReport, RemoveBlock, RemovePage, ShowReport,
};
pub use defs::command_defs;
pub use error::{ReportError, TextFault};
pub use event::{Op, ReportEvent, ADD_BLOCK, ADD_PAGE, CREATE_REPORT, REMOVE_BLOCK, REMOVE_PAGE, SHOW_REPORT};
pub use kind::{BlockKind, KindRegistry, KindRegistryError, PayloadError};
pub use model::{
    is_v7, Block, BlockId, BlockKindId, CanonicalJson, Page, PageId, Report, ReportId, TemplateRef, MAX_BLOCKS,
    MAX_ISSUED_IDS, MAX_PAGES, MAX_PAYLOAD_BYTES, MAX_PAYLOAD_DEPTH, MAX_REPORT_BYTES, SCHEMA_VERSION,
};
pub use replay::{replay_exact, ReplayRun, MAX_REPLAY_EVENTS};
pub use text::{must_escape, Text, Untrusted, MAX_TEXT_BYTES};
pub use value::{
    check_value_binding, is_pointer, resolve_pointer, BindingFault, MAX_POINTER_BYTES, MAX_VALUE_FIELDS,
    VALUE_BINDING_SCHEMA,
};
