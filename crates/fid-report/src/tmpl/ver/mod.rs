//! Template versions, immutability and the template commands. Spec node
//! `RPT.MODEL.TMPL.VER`.
//!
//! - **IDs and versions.** A template ID is the template document's own `template_id` (a
//!   UUIDv7, `RPT.MODEL.TMPL.FORMAT`). A version is `TemplateRef { id, version, digest,
//!   builtin }`, `digest` the SHA-256 of the canonical bytes. Built-in and project versions
//!   are separate number spaces: `builtin:<id>@<version>` (the version given by the shipping
//!   crate) and `<id>@<version>` (1, 2, 3, ... per ID, from the journal). A project save
//!   never uses or collides with a built-in version, so a new release cannot break a
//!   project's index.
//! - **Save** (`report.save_template@1`, W2): from a stored report or from a template file
//!   named by an import-root handle. An ID with a locked built-in fails with
//!   `report.template_locked` (REQ-003); the template is validated (format, slots, kind
//!   registry, payloads, value-field rule and the label rule below); identical bytes return
//!   the latest project version and journal nothing (REQ-005); otherwise project version
//!   n+1 is stored in the content-addressed store and one event is journaled, under a
//!   compare-and-set on the latest version (`conflict/head_moved` when a concurrent save
//!   won). Project-level locking is deferred: a file with `locked: true` is refused.
//! - **Immutability** (REQ-002): the backend can only add objects; nothing replaces or
//!   deletes one, and every read re-hashes the bytes against their digest.
//! - **Apply** (`report.apply_template@1`, W2): a new report whose pages and blocks are the
//!   template's, with `template_ref = {builtin, digest, id, version}`. REQ-004 is met by
//!   reference: blocks keep the template's block IDs and the report records the digest, so
//!   slots are resolved from the recorded template version (by `RPT.MODEL.BIND.GEN`), not
//!   copied into the report.
//! - **Show / list** (R0): one version's canonical document; every version sorted by ID,
//!   then built-in before project, then version, with locked flags (REQ-006).
//! - **Files** (REQ-007): read only through [`fid_io_reg::RootSet`]; an agent's or script's
//!   raw path is `raw_path_forbidden` before any file-system call.
//! - **Labels (D-045).** The guarantee is architectural: labels, the provisional one
//!   included, are produced by `RPT.MODEL.FMT` from the stored record a value binding names;
//!   a template holds no value, value text or number (format rule), so nothing in it reaches
//!   the label code and it cannot hide a label. As defence in depth this leaf also refuses
//!   any payload key that is a label switch ([`is_label_control_key`], keys
//!   normalised by [`normalise_key`]) and the provisional label's key or text
//!   (`report.template_invalid`, reason `label_control`).
//! - **Replay** ([`replay_exact`], AC-6): saves and applies replay from the store by digest;
//!   a changed template is caught by its digest, a built-in changed under the same version
//!   by its digest, and replay never depends on which other built-ins a release ships.

pub mod command;
mod convert;
pub mod defs;
pub mod error;
pub mod event;
pub mod replay;
pub mod store;

pub use command::{
    applied_value, apply_template, execute, list_template, list_value, parse_selector, save_template, saved_value,
    show_template, shown_value, ApplyTemplate, Env, SaveTemplate, Saved, Selector, ShowTemplate, Source, TemplateInfo,
    MAX_FILE_PARAM_BYTES,
};
pub use convert::{is_label_control_key, normalise_key, LABEL_CONTROL_WORDS};
pub use defs::command_defs;
pub use error::VerError;
pub use event::{SaveSource, TemplateEvent, APPLY_TEMPLATE, LIST_TEMPLATE, SAVE_TEMPLATE, SHOW_TEMPLATE};
pub use replay::{replay_exact, TemplateReplayRun};
pub use store::{
    BuiltinRegistry, Expected, JournalEntry, MemoryTemplateBackend, ObjectRead, TemplateBackend, TemplateIndex,
    MAX_TEMPLATE_IDS, MAX_VERSIONS,
};
