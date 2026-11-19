//! Defaults profiles as immutable, content-addressed, versioned objects (spec node
//! `GDT.CHAR.DPROF`, REQ-GDT.CHAR-004).
//!
//! - [`ident`]: the reference grammar `dp:<dialect>-<edition>@<x.y.z>#sha256:<hex>`
//!   (REQ-001), editions, versions and characteristic IDs.
//! - [`profile`]: the `fid.defaults-profile/1` document, its strict bounded parser, the
//!   key vocabulary it is checked against (`unknown_key`), and the digest: SHA-256 over
//!   the RFC 8785 canonical JSON of the whole document.
//! - [`store`]: the project-local profile store and per-characteristic profile bindings,
//!   with the two W2 commands `gdt.publish_defaults_profile@1` (REQ-002, 003, 004, 006)
//!   and `gdt.repoint_defaults_profile@1` (REQ-005), human adoption of agent-branch
//!   profiles, and a canonical snapshot for persistence.
//! - [`event`]: the journal events of both commands (canonical JSON, strict reader).
//! - [`replay`]: exact replay of those events (`IDENTICAL` when every re-derived event
//!   matches byte for byte).
//! - [`publish_defaults_profile`]: the command entry point that reads the profile through
//!   an approved import handle (`fid-io-reg`), so agents never pass raw paths.
//!
//! Nothing here panics; every failure is a [`DprofError`] with a stable code.

pub mod event;
pub mod ident;
pub mod profile;
pub mod replay;
pub mod store;

pub use event::{Change, DprofEvent, PublishEvent, RepointEvent};
pub use ident::{CharId, Edition, ProfileId, ProfileRef, SemVer};
pub use profile::{parse_profile, DefaultsProfile, KeyVocabulary, ProfileDoc, ProfileValue};
pub use replay::{replay, ProfileContent, Replayed};
pub use store::{Branch, DprofState, PublishStatus, Published, Repointed, StoredProfile};

use fid_io_reg::{IoError, Principal, RootSet};
use std::fmt;
use std::io::Read;

/// Command ID of the publish command.
pub const PUBLISH_COMMAND: &str = "gdt.publish_defaults_profile@1";
/// Command ID of the re-point command.
pub const REPOINT_COMMAND: &str = "gdt.repoint_defaults_profile@1";

/// Longest text echoed back inside an error, in bytes.
const MAX_ECHO: usize = 160;

fn echo(s: &str) -> String {
    let mut end = s.len().min(MAX_ECHO);
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    s[..end].to_owned()
}

/// Errors of defaults-profile handling. [`DprofError::code`] is the stable code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DprofError {
    /// `malformed_profile{reason}`: the document breaks the format or a limit.
    Malformed {
        /// Static reason.
        reason: &'static str,
    },
    /// `unknown_key{key}`: a key outside the DMT schema's vocabulary.
    UnknownKey {
        /// The key.
        key: String,
    },
    /// `profile_immutable{id}`: this dialect, edition and version exist with another digest.
    ProfileImmutable {
        /// `dp:<dialect>-<edition>@<version>`.
        id: String,
    },
    /// `dialect_mismatch{char}`: a named characteristic's dialect differs from the target's.
    DialectMismatch {
        /// The characteristic.
        char: String,
    },
    /// `defaults_profile_unknown{ref}`: no such profile is selectable on this branch.
    DefaultsProfileUnknown {
        /// The reference as given (truncated).
        reference: String,
    },
    /// `bad_profile_ref`: the text is not a profile reference.
    BadProfileRef,
    /// `bad_request{reason}`: a malformed command parameter.
    BadRequest {
        /// Static reason.
        reason: &'static str,
    },
    /// `char_unknown{char}`: no profile binding exists for this characteristic.
    CharUnknown {
        /// The characteristic.
        char: String,
    },
    /// `char_exists{char}`: the characteristic is already bound.
    CharExists {
        /// The characteristic.
        char: String,
    },
    /// `human_only`: only a human principal may do this.
    HumanOnly,
    /// `bad_vocabulary{reason}`: the key vocabulary itself is invalid.
    BadVocabulary {
        /// Static reason.
        reason: &'static str,
    },
    /// `store_invalid{reason}`: a store snapshot breaks the format, or the store is full.
    Store {
        /// Static reason.
        reason: &'static str,
    },
    /// `event_invalid{reason}`: a journal event could not be read.
    Event {
        /// Static reason.
        reason: &'static str,
    },
    /// An import-handle or file-system error from `fid-io-reg`.
    Io(IoError),
}

impl DprofError {
    /// The stable code.
    pub fn code(&self) -> &'static str {
        match self {
            DprofError::Malformed { .. } => "malformed_profile",
            DprofError::UnknownKey { .. } => "unknown_key",
            DprofError::ProfileImmutable { .. } => "profile_immutable",
            DprofError::DialectMismatch { .. } => "dialect_mismatch",
            DprofError::DefaultsProfileUnknown { .. } => "defaults_profile_unknown",
            DprofError::BadProfileRef => "bad_profile_ref",
            DprofError::BadRequest { .. } => "bad_request",
            DprofError::CharUnknown { .. } => "char_unknown",
            DprofError::CharExists { .. } => "char_exists",
            DprofError::HumanOnly => "human_only",
            DprofError::BadVocabulary { .. } => "bad_vocabulary",
            DprofError::Store { .. } => "store_invalid",
            DprofError::Event { .. } => "event_invalid",
            DprofError::Io(e) => e.code(),
        }
    }

    /// The static reason, where there is one.
    pub fn reason(&self) -> Option<&'static str> {
        match self {
            DprofError::Malformed { reason }
            | DprofError::BadRequest { reason }
            | DprofError::BadVocabulary { reason }
            | DprofError::Store { reason }
            | DprofError::Event { reason } => Some(reason),
            _ => None,
        }
    }

    pub(crate) fn unknown_ref(text: &str) -> DprofError {
        DprofError::DefaultsProfileUnknown { reference: echo(text) }
    }
}

impl fmt::Display for DprofError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DprofError::UnknownKey { key: s }
            | DprofError::ProfileImmutable { id: s }
            | DprofError::DialectMismatch { char: s }
            | DprofError::DefaultsProfileUnknown { reference: s }
            | DprofError::CharUnknown { char: s }
            | DprofError::CharExists { char: s } => write!(f, "{}{{{}}}", self.code(), s.escape_debug()),
            DprofError::Io(e) => write!(f, "{e}"),
            other => match other.reason() {
                Some(r) => write!(f, "{}{{{r}}}", other.code()),
                None => f.write_str(other.code()),
            },
        }
    }
}

impl std::error::Error for DprofError {}

impl From<IoError> for DprofError {
    fn from(e: IoError) -> Self {
        DprofError::Io(e)
    }
}

/// Longest `file` parameter accepted, in bytes.
pub const MAX_FILE_PARAM_LEN: usize = 1024;

/// Runs `gdt.publish_defaults_profile@1` (W2; CLI `fid gdt publish-defaults-profile`, MCP
/// `gdt_publish_defaults_profile`): resolves `file` through the approved import roots
/// (agents and scripts must pass a `root://` handle), reads at most
/// [`profile::MAX_PROFILE_BYTES`], parses and checks the profile against `vocab`, and
/// publishes it into `state` on the principal's branch. Returns the reference, whether it
/// was created or already existed, and the journal event (none for an identical
/// re-publish or a dry run).
pub fn publish_defaults_profile(
    state: &mut DprofState,
    roots: &RootSet,
    principal: Principal,
    file: &str,
    vocab: &KeyVocabulary,
    dry_run: bool,
) -> Result<Published, DprofError> {
    if file.is_empty() || file.len() > MAX_FILE_PARAM_LEN {
        return Err(DprofError::BadRequest { reason: "bad_file" });
    }
    let opened = roots.resolve_import(principal, file)?;
    let limit = profile::MAX_PROFILE_BYTES as u64 + 1;
    let mut bytes = Vec::new();
    opened.file.take(limit).read_to_end(&mut bytes).map_err(|_| DprofError::Malformed { reason: "unreadable" })?;
    let doc = parse_profile(&bytes, vocab)?;
    state.publish(doc, principal, file, dry_run)
}
