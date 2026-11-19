//! Committed records, one per (run, characteristic) per branch, and `gdt.get_result@1`.

use super::builder::ResultRecord;
use super::fields::RunId;
use super::RecordError;
use crate::chars::dprof::{Branch, CharId};
use fid_io_reg::Principal;
use fid_types::Digest;
use std::collections::BTreeMap;

/// Command ID of the read command (R0; CLI `fid gdt get-result --run <R> --char <C> --json`,
/// MCP `gdt_get_result`).
pub const GET_RESULT_COMMAND: &str = "gdt.get_result@1";
/// Most records one store holds.
pub const MAX_RECORDS: usize = 1_000_000;

/// The committed ResultRecords. A record lives on the branch derived from the committing
/// principal ([`Branch::for_principal`]: a person's on the working branch, an agent's or a
/// script's on the agent branch), so agent work never reaches the working branch, and each
/// branch holds at most one record per (run, characteristic).
#[derive(Debug, Clone, Default)]
pub struct ResultStore {
    records: BTreeMap<(Branch, RunId, CharId), ResultRecord>,
}

impl ResultStore {
    /// An empty store.
    pub fn new() -> ResultStore {
        ResultStore::default()
    }

    /// Commits `record` on `principal`'s branch and returns its digest. A second record for
    /// the same (run, characteristic) on that branch is `duplicate_result`
    /// (REQ-GDT.CHAR.RECORD-006), whatever its content; a full store is `store_full`.
    pub fn commit(&mut self, principal: Principal, record: ResultRecord) -> Result<Digest, RecordError> {
        let key = (Branch::for_principal(principal), record.run().clone(), record.characteristic().clone());
        if self.records.contains_key(&key) {
            return Err(RecordError::DuplicateResult);
        }
        if self.records.len() >= MAX_RECORDS {
            return Err(RecordError::StoreFull);
        }
        let digest = record.digest();
        self.records.insert(key, record);
        Ok(digest)
    }

    /// The record of (run, characteristic) on `principal`'s branch (`not_found`).
    pub fn get(&self, principal: Principal, run: &RunId, char: &CharId) -> Result<&ResultRecord, RecordError> {
        self.records.get(&(Branch::for_principal(principal), run.clone(), char.clone())).ok_or(RecordError::NotFound)
    }

    /// Number of records on all branches.
    pub fn len(&self) -> usize {
        self.records.len()
    }

    /// Whether the store holds no record.
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }
}

/// `gdt.get_result@1` (R0, REQ-GDT.CHAR.RECORD-007): the stored canonical JSON of the record
/// of (`run`, `char`) on `principal`'s branch. Errors: `invalid_field{run}`,
/// `invalid_field{char}` for IDs outside their grammar, `not_found`. Read-only.
pub fn get_result<'s>(
    store: &'s ResultStore,
    principal: Principal,
    run: &str,
    char: &str,
) -> Result<&'s [u8], RecordError> {
    let run = RunId::parse(run)?;
    let char = CharId::parse(char).ok_or(RecordError::InvalidField { field: "char" })?;
    store.get(principal, &run, &char).map(ResultRecord::canonical_json)
}
