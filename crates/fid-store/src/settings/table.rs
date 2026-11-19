//! Project-scope values: the `settings_current` table of `project.db`.
//!
//! A row is `(key, value, entry_hash)`: `value` is the canonical JSON record `{"v": <value>}`
//! for a set and `{}` for a reset (the transaction API has no delete; a reset row means "no
//! value at project scope"), and `entry_hash` the Record Journal entry that wrote it. The table
//! is derived from journal entries and rebuildable. It is written only by
//! [`stage_project_value`], in the same transaction as the journal entry, from a
//! [`ChangeEntry`] the settings commands built.

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
    /// The stored record of `key` (see [`encode_record`]), `Ok(None)` when there is none.
    /// `Err` holds a display-safe reason.
    fn record(&self, key: &str) -> Result<Option<Vec<u8>>, String>;
}

/// [`ProjectValues`] over `settings_current` of an open project.
#[derive(Debug, Clone, Copy)]
pub struct ProjectTable<'s> {
    store: &'s Store,
}

impl<'s> ProjectTable<'s> {
    pub fn new(store: &'s Store) -> ProjectTable<'s> {
        ProjectTable { store }
    }
}

impl ProjectValues for ProjectTable<'_> {
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

/// Stages the `settings_current` row of a project-scope change in `txn`, the transaction that
/// appends `entry` as Record Journal entry `entry_hash`. Refuses an entry of another scope.
pub fn stage_project_value(txn: &mut Txn<'_>, entry: &ChangeEntry, entry_hash: &str) -> Result<(), SettingsError> {
    if entry.scope() != Scope::Project {
        return Err(SettingsError::Validation("only project-scope changes go to settings_current".into()));
    }
    let record = encode_record(entry.value())?;
    txn.upsert(
        TABLE,
        &[("key", Value::from(entry.key()))],
        &[("value", Value::Blob(record)), ("entry_hash", Value::from(entry_hash))],
    );
    Ok(())
}
