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

/// The committed ResultRecords, keyed by (branch, run, characteristic). The branch is
/// derived from the principal ([`Branch::for_principal`]), with the overlay rule of
/// `chars::dprof`:
///
/// - a person commits to and reads from the working branch only, so agent work never
///   reaches it; a person may commit for a (run, characteristic) an agent already proposed
///   a record for, and that agent record stays on the agent branch, shadowed;
/// - an agent or a script commits to the agent branch, refused with `duplicate_result` when
///   either branch already holds that (run, characteristic); it reads the agent branch
///   first, then the working branch.
#[derive(Debug, Clone, Default)]
pub struct ResultStore {
    records: BTreeMap<(Branch, RunId, CharId), ResultRecord>,
}

impl ResultStore {
    /// An empty store.
    pub fn new() -> ResultStore {
        ResultStore::default()
    }

    /// Commits `record` on `principal`'s branch and returns its digest. `duplicate_result`
    /// (REQ-GDT.CHAR.RECORD-006), whatever the content, when a record for the same
    /// (run, characteristic) is visible to the committer: on the working branch for a
    /// person; on the agent or the working branch for an agent or a script. A full store is
    /// `store_full`.
    pub fn commit(&mut self, principal: Principal, record: ResultRecord) -> Result<Digest, RecordError> {
        let branch = Branch::for_principal(principal);
        if self.lookup(branch, record.run(), record.characteristic()).is_some() {
            return Err(RecordError::DuplicateResult);
        }
        if self.records.len() >= MAX_RECORDS {
            return Err(RecordError::StoreFull);
        }
        let digest = record.digest();
        self.records.insert((branch, record.run().clone(), record.characteristic().clone()), record);
        Ok(digest)
    }

    /// The record of (run, characteristic) as `principal` sees it (`not_found`): a person
    /// reads the working branch only; an agent or a script reads the agent branch, then the
    /// working branch.
    pub fn get(&self, principal: Principal, run: &RunId, char: &CharId) -> Result<&ResultRecord, RecordError> {
        self.lookup(Branch::for_principal(principal), run, char).ok_or(RecordError::NotFound)
    }

    fn lookup(&self, branch: Branch, run: &RunId, char: &CharId) -> Option<&ResultRecord> {
        let found = |b: Branch| self.records.get(&(b, run.clone(), char.clone()));
        match branch {
            Branch::Working => found(Branch::Working),
            Branch::Agent => found(Branch::Agent).or_else(|| found(Branch::Working)),
        }
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

/// The record of (`run`, `char`) as `principal` sees it, with the ID grammar checks of
/// [`get_result`] (`invalid_field{run}`, `invalid_field{char}`, `not_found`).
pub fn get_record<'s>(
    store: &'s ResultStore,
    principal: Principal,
    run: &str,
    char: &str,
) -> Result<&'s ResultRecord, RecordError> {
    let run = RunId::parse(run)?;
    let char = CharId::parse(char).ok_or(RecordError::InvalidField { field: "char" })?;
    store.get(principal, &run, &char)
}

/// `gdt.get_result@1` (R0, REQ-GDT.CHAR.RECORD-007): the stored canonical JSON of the record
/// of (`run`, `char`) as `principal` sees it ([`ResultStore::get`]). Errors: `invalid_field{run}`,
/// `invalid_field{char}` for IDs outside their grammar, `not_found`. Read-only.
pub fn get_result<'s>(
    store: &'s ResultStore,
    principal: Principal,
    run: &str,
    char: &str,
) -> Result<&'s [u8], RecordError> {
    get_record(store, principal, run, char).map(ResultRecord::canonical_json)
}
