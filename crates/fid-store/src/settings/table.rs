//! Project-scope values: the `settings_current` table of `project.db`.
//!
//! A row is `(key, value, entry_hash)`: `value` is the canonical JSON record `{"v": <value>}`
//! for a set and `{}` for a reset (the transaction API has no delete; a reset row means "no
//! value at project scope"), and `entry_hash` the Record Journal entry that wrote it. The table
//! is derived from journal entries and rebuildable. It is written only by
//! [`stage_project_value`], in the same transaction as the journal entry, from a
//! [`ChangeEntry`] the settings commands built.
//!
//! **Working branch only.** The v1 table has no branch column: it holds the values of one
//! branch, the working branch it serves ([`ProjectValues::working_branch`]). A project change
//! committed on any other branch (an agent's `agent/<client>/<n>`) stays in the journal only:
//! [`stage_project_value`] stages no row for it ([`Staged::JournalOnly`]), so it never changes
//! what a person on the working branch reads (maintainer decision on #275). Until per-branch
//! reads exist, an agent cannot read back its own project overrides.

use super::commands::ChangeEntry;
use super::{Scope, SettingsError, MAX_VALUE_BYTES};
use crate::txn::{Store, Txn, Value};
use fid_canon::{parse_with_limits, to_canonical_bytes, CValue};
use rusqlite::OptionalExtension;
use std::collections::BTreeMap;

/// The table.
pub const TABLE: &str = "settings_current";
/// Largest record read: a value plus the `{"v":}` wrapper.
const MAX_RECORD_BYTES: usize = MAX_VALUE_BYTES + 16;
const MAX_RECORD_DEPTH: usize = 64;

/// Read access to the project-scope values of the branch a call reads. The project store
/// implements it ([`ProjectTable`]); tests and other hosts may too.
pub trait ProjectValues {
    /// The branch whose values this source holds (the working branch).
    fn working_branch(&self) -> &str;

    /// The stored record of `key` (see [`encode_record`]), `Ok(None)` when there is none.
    /// `Err` holds a display-safe reason.
    fn record(&self, key: &str) -> Result<Option<Vec<u8>>, String>;
}

/// [`ProjectValues`] over `settings_current` of an open project.
#[derive(Debug, Clone, Copy)]
pub struct ProjectTable<'s> {
    store: &'s Store,
    working_branch: &'s str,
}

impl<'s> ProjectTable<'s> {
    /// The table of `store`, serving `working_branch`.
    pub fn new(store: &'s Store, working_branch: &'s str) -> ProjectTable<'s> {
        ProjectTable { store, working_branch }
    }
}

impl ProjectValues for ProjectTable<'_> {
    fn working_branch(&self) -> &str {
        self.working_branch
    }

    fn record(&self, key: &str) -> Result<Option<Vec<u8>>, String> {
        self.store
            .read(|c| {
                c.query_row("SELECT value FROM settings_current WHERE key = ?1", [key], |r| r.get::<_, Vec<u8>>(0))
                    .optional()
            })
            .map_err(|e| format!("settings_current: {}", e.code()))
    }
}

/// The record of a set (`Some`) or a reset (`None`).
pub fn encode_record(value: Option<&CValue>) -> Result<Vec<u8>, SettingsError> {
    let mut o = BTreeMap::new();
    if let Some(v) = value {
        o.insert("v".to_string(), v.clone());
    }
    to_canonical_bytes(&CValue::Obj(o)).map_err(|e| SettingsError::Validation(format!("record: {}", e.code())))
}

/// Decodes a record: `Ok(Some(value))` for a set, `Ok(None)` for a reset, `Err(())` for
/// anything else (the read then warns `unreadable` and goes on to the next scope).
#[allow(clippy::result_unit_err)]
pub fn decode_record(bytes: &[u8]) -> Result<Option<CValue>, ()> {
    let v = parse_with_limits(bytes, MAX_RECORD_BYTES, MAX_RECORD_DEPTH).map_err(|_| ())?;
    let CValue::Obj(mut o) = v else { return Err(()) };
    match (o.len(), o.remove("v")) {
        (0, None) => Ok(None),
        (1, Some(v)) => Ok(Some(v)),
        _ => Err(()),
    }
}

/// What [`stage_project_value`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Staged {
    /// The row is staged in the transaction.
    Row,
    /// The entry is on another branch than the working branch: journal only, no row.
    JournalOnly,
}

/// Stages the `settings_current` row of a project-scope change in `txn`, the transaction that
/// appends `entry` as Record Journal entry `entry_hash`, when the entry is on
/// `working_branch`, the branch the table serves; otherwise stages nothing and returns
/// [`Staged::JournalOnly`]. Refuses an entry of another scope.
pub fn stage_project_value(
    txn: &mut Txn<'_>,
    entry: &ChangeEntry,
    entry_hash: &str,
    working_branch: &str,
) -> Result<Staged, SettingsError> {
    if entry.scope() != Scope::Project {
        return Err(SettingsError::Validation("only project-scope changes go to settings_current".into()));
    }
    if entry.branch() != Some(working_branch) {
        return Ok(Staged::JournalOnly);
    }
    let record = encode_record(entry.value())?;
    txn.upsert(
        TABLE,
        &[("key", Value::from(entry.key()))],
        &[("value", Value::Blob(record)), ("entry_hash", Value::from(entry_hash))],
    );
    Ok(Staged::Row)
}
