//! Template versions, immutability and the template commands. Spec node
//! `RPT.MODEL.TMPL.VER`.
//!
//! - **IDs.** A template ID is the template document's own `template_id` (a UUIDv7,
//!   `RPT.MODEL.TMPL.FORMAT`). A saved version is `TemplateRef { id, version, digest }`,
//!   `digest` the SHA-256 of the template's canonical bytes.
//! - **Save** (`report.save_template@1`, W2): from a stored report or from a template file
//!   named by an import-root handle. Locked IDs fail with `report.template_locked`; the
//!   template is validated (format, slots, kind registry, payloads, value-field rule and the
//!   label rule below); identical bytes return the latest version and journal nothing
//!   (REQ-005); otherwise version n+1 (n the highest built-in or project version of the ID,
//!   1 for a new ID) is stored in the content-addressed store and one event is journaled.
//! - **Immutability** (REQ-002): the backend can only add objects; nothing replaces or
//!   deletes one, and every read re-hashes the bytes against their digest.
//! - **Apply** (`report.apply_template@1`, W2): a new report whose pages and blocks are the
//!   template's (same IDs, so each block's slot is found in the recorded template version),
//!   with `template_ref = {digest, id, version}` (REQ-004).
//! - **Show / list** (R0): one version's canonical document; every version, built-in and
//!   project, sorted by ID then version, with locked flags (REQ-006).
//! - **Files** (REQ-007): read only through [`fid_io_reg::RootSet`]; an agent's or script's
//!   raw path is `raw_path_forbidden` before any file-system call.
//! - **Labels (D-045).** A template can never hide the provisional-interpretation label: it
//!   holds no value, value text or number (format rule), and this leaf also refuses any
//!   payload holding a label switch ([`LABEL_CONTROL_KEYS`]) or the provisional label's key
//!   or text (`report.template_invalid`, reason `label_control`).
//! - **Replay** ([`replay_exact`], AC-6): saves and applies replay from the store by digest;
//!   a changed template is caught by its digest, a changed built-in by its version's digest.

pub mod command;
mod convert;
pub mod defs;
pub mod error;
pub mod event;
pub mod replay;
pub mod store;

pub use command::{
    applied_value, apply_template, execute, list_template, list_value, parse_selector, save_template, saved_value,
    show_template, shown_value, ApplyTemplate, Env, SaveTemplate, Saved, ShowTemplate, Source, TemplateInfo,
    MAX_FILE_PARAM_BYTES,
};
pub use convert::LABEL_CONTROL_KEYS;
pub use defs::command_defs;
pub use error::VerError;
pub use event::{SaveSource, TemplateEvent, APPLY_TEMPLATE, LIST_TEMPLATE, SAVE_TEMPLATE, SHOW_TEMPLATE};
pub use replay::{replay_exact, TemplateReplayRun};
pub use store::{
    BuiltinRegistry, JournalEntry, MemoryTemplateBackend, ObjectRead, TemplateBackend, TemplateIndex, MAX_TEMPLATE_IDS,
    MAX_VERSIONS,
};
