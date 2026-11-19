//! Typed settings: declarations, scopes, resolution and the settings commands (spec node
//! `CORE.STO.SET`).
//!
//! **Declarations** ([`decl`]). Each owning domain declares its settings with
//! [`fid_settings!`](crate::fid_settings): key, schema in the registry keyword dialect, default,
//! allowed scopes ([`Scope`]) and an admin flag. [`SettingsRegistry::build`] checks every
//! declaration (REQ-001): the key grammar, the owner (the key's first segment), the schema with
//! `fid_registry::check_schema` plus this module's closed keyword subset ([`check`]), a quantity
//! on every `number`, a default that validates, and a non-empty scope set. The registry
//! exports `settings-manifest.json` ([`SettingsRegistry::manifest`]).
//!
//! **Stores.** Project values live in the project database's `settings_current` table
//! ([`ProjectTable`], written only through [`stage_project_value`] in the journal's own
//! transaction). User and machine values live in canonical JSON files ([`FileStore`]) written
//! by temp file plus rename. Nothing outside the commands writes a setting (REQ-004): the file
//! writer is private to this module and the project row is staged only from a
//! [`ChangeEntry`], which only the commands build.
//!
//! **Resolution** ([`resolve()`]). A read goes through the key's allowed scopes in the order
//! project, user, machine, default and reports the scope that supplied the value (REQ-003). A
//! stored value that fails its schema is never used: the next scope or the default is, and a
//! [`SettingWarning`] naming the key and scope goes to the Activity Log (REQ-002).
//!
//! **Commands** ([`commands`]), declared with the `fid-registry` macros:
//!
//! | Command | Class | MCP | Notes |
//! |---|---|---|---|
//! | `settings.list_values@1` | R0 | exposed | sorted by key (REQ-007) |
//! | `settings.set_value@1` | W2 | exposed | refuses admin keys with `admin_setting` (REQ-005) |
//! | `settings.reset_value@1` | W2 | exposed | as set |
//! | `settings.set_admin_value@1` | A0 | never | `human_only`; human principal, CLI or GUI only (REQ-006) |
//!
//! Every change is one [`ChangeEntry`] handed to the [`SettingsJournal`] seam: project scope to
//! the project's Record Journal, user and machine scope to the per-user settings journal
//! (REQ-008). Agents cannot change user or machine values (`policy`) and change project values
//! only on their agent branch `agent/<client>/<n>` (REQ-009). File scopes are written only after
//! the journal commit; a refused command changes nothing (REQ-010).
//!
//! The checks of a change run in a pinned order, and the first failure is returned before the
//! journal is touched: unknown key, admin flag, surface and principal, allowed scope, value
//! schema, store writability.

pub mod check;
pub mod commands;
pub mod decl;
pub mod files;
pub mod resolve;
pub mod table;

pub use check::{
    check_setting_schema, validate_value, wire_form, ValueError, MAX_EXACT_INTEGER, MAX_SCHEMA_DEPTH,
    SUPPORTED_KEYWORDS,
};
pub use commands::{
    declarations, invoke, is_agent_branch, list_to_value, list_values, replay_outcome, reset_value, set_admin_value,
    set_value, Caller, ChangeEntry, Invocation, JournalTarget, ListValuesParams, ListedValue, PrincipalKind,
    ProjectScope, ReplayOutcome, ResetValueParams, ResetValueResult, SetValueParams, SetValueResult, SettingValue,
    SettingsCtx, SettingsJournal, LIST_VALUES, RESET_VALUE, SET_ADMIN_VALUE, SET_VALUE,
};
pub use decl::{DeclError, SettingDecl, SettingSource, SettingsRegistry, MANIFEST_FORMAT};
pub use files::{machine_config_dir, user_config_dir, FileStore, FILE_FORMAT, MAX_FILE_BYTES, SETTINGS_FILE};
pub use resolve::{resolve, Resolved, Sources};
pub use table::{decode_record, encode_record, stage_project_value, ProjectTable, ProjectValues, Staged};

use fid_registry::FidSchema;
use std::fmt;

/// Longest setting key, in bytes.
pub const MAX_KEY_BYTES: usize = 128;
/// Largest setting value or default, in bytes of its RFC 8785 canonical form.
pub const MAX_VALUE_BYTES: usize = 64 * 1024;
/// Most settings one registry declares, and most values one settings file holds.
pub const MAX_SETTINGS: usize = 4096;
/// The key grammar of REQ-001, for schemas.
pub const KEY_PATTERN: &str = "^[a-z][a-z0-9_]*(\\.[a-z][a-z0-9_]*)+$";

/// Where a setting value is stored.
#[derive(FidSchema, Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Scope {
    /// The project database; journaled in the project's Record Journal.
    Project,
    /// The per-user settings file; journaled in the per-user settings journal.
    User,
    /// The machine settings file; journaled in the per-user settings journal.
    Machine,
}

impl Scope {
    /// Every scope, in resolution order.
    pub const ALL: [Scope; 3] = [Scope::Project, Scope::User, Scope::Machine];

    /// The wire string.
    pub fn as_str(self) -> &'static str {
        match self {
            Scope::Project => "project",
            Scope::User => "user",
            Scope::Machine => "machine",
        }
    }

    /// The scope with this exact wire string.
    pub fn parse(s: &str) -> Option<Scope> {
        Scope::ALL.into_iter().find(|v| v.as_str() == s)
    }
}

impl fmt::Display for Scope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The scope that supplied an effective value: one of the [`Scope`]s, or the default.
#[derive(FidSchema, Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Source {
    Project,
    User,
    Machine,
    Default,
}

impl Source {
    /// The wire string.
    pub fn as_str(self) -> &'static str {
        match self {
            Source::Project => "project",
            Source::User => "user",
            Source::Machine => "machine",
            Source::Default => "default",
        }
    }
}

impl From<Scope> for Source {
    fn from(s: Scope) -> Source {
        match s {
            Scope::Project => Source::Project,
            Scope::User => Source::User,
            Scope::Machine => Source::Machine,
        }
    }
}

/// A refused or failed settings command. Codes are the leaf's error list plus two internal
/// faults; nothing was changed unless the code is `storage`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SettingsError {
    /// W2 change of an admin-flagged key (REQ-005).
    AdminSetting(String),
    /// Principal or surface not allowed (REQ-006, REQ-009).
    Policy(String),
    /// Scope not among the key's allowed scopes (REQ-010).
    ScopeNotAllowed(String),
    /// The scope's store cannot be written by this process (REQ-010).
    ScopeNotWritable(String),
    /// The key is not declared.
    UnknownKey(String),
    /// Parameters or the value failed their schema.
    Validation(String),
    /// The journal refused or failed the commit; nothing was changed.
    Journal(String),
    /// The journal committed but the file store could not be updated; the store is derived
    /// from the journal and must be rebuilt.
    Storage(String),
}

impl SettingsError {
    /// Stable machine-readable code.
    pub fn code(&self) -> &'static str {
        match self {
            SettingsError::AdminSetting(_) => "admin_setting",
            SettingsError::Policy(_) => "policy",
            SettingsError::ScopeNotAllowed(_) => "scope_not_allowed",
            SettingsError::ScopeNotWritable(_) => "scope_not_writable",
            SettingsError::UnknownKey(_) => "unknown_key",
            SettingsError::Validation(_) => "validation",
            SettingsError::Journal(_) => "journal_unavailable",
            SettingsError::Storage(_) => "storage",
        }
    }

    /// The detail text (display-safe: never echoes a setting value).
    pub fn detail(&self) -> &str {
        match self {
            SettingsError::AdminSetting(d)
            | SettingsError::Policy(d)
            | SettingsError::ScopeNotAllowed(d)
            | SettingsError::ScopeNotWritable(d)
            | SettingsError::UnknownKey(d)
            | SettingsError::Validation(d)
            | SettingsError::Journal(d)
            | SettingsError::Storage(d) => d,
        }
    }
}

impl fmt::Display for SettingsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code(), self.detail())
    }
}

impl std::error::Error for SettingsError {}

/// Why a stored value was not used.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum WarningReason {
    /// The value fails the setting's schema (REQ-002).
    InvalidValue,
    /// The value is stored in a scope the key does not allow.
    ScopeNotAllowed,
    /// The settings file or project row could not be read or decoded.
    Unreadable,
    /// The file holds a key no setting declares.
    UnknownKey,
}

impl WarningReason {
    /// Stable machine-readable code.
    pub fn code(self) -> &'static str {
        match self {
            WarningReason::InvalidValue => "invalid_value",
            WarningReason::ScopeNotAllowed => "scope_not_allowed",
            WarningReason::Unreadable => "unreadable",
            WarningReason::UnknownKey => "unknown_key",
        }
    }
}

/// An Activity Log warning about a stored value (REQ-002). `key` is `None` when a whole file is
/// unreadable, or when a stored key is not even a well-formed key (it is then not echoed).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct SettingWarning {
    pub key: Option<String>,
    pub scope: Scope,
    pub reason: WarningReason,
}

impl fmt::Display for SettingWarning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.key {
            Some(k) => write!(f, "setting {k} in scope {}: {}", self.scope, self.reason.code()),
            None => write!(f, "settings in scope {}: {}", self.scope, self.reason.code()),
        }
    }
}

/// The unchained Activity Log, where warnings go (REQ-002). The host wires it.
pub trait ActivityLog {
    /// Records one warning.
    fn warn(&self, warning: &SettingWarning);
}

/// Whether `key` matches the key grammar of REQ-001 and is at most [`MAX_KEY_BYTES`] bytes.
pub fn is_valid_key(key: &str) -> bool {
    if key.is_empty() || key.len() > MAX_KEY_BYTES {
        return false;
    }
    let mut segments = 0usize;
    for seg in key.split('.') {
        let mut chars = seg.chars();
        match chars.next() {
            Some(c) if c.is_ascii_lowercase() => {}
            _ => return false,
        }
        if !chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_') {
            return false;
        }
        segments += 1;
    }
    segments >= 2
}
