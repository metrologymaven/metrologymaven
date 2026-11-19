//! Report storage and journaling seam (CORE.STO, CORE.JRN).
//!
//! The command handlers never touch SQLite or the journal file: they hand one
//! [`StoredReport`] and one [`ReportEvent`] to a [`ReportBackend`], which must commit both
//! atomically (the command bus does this in production; keeping the store driver out of
//! this crate is the CORE.BUS.LINT "no side doors" rule). [`MemoryBackend`] is the
//! in-memory backend used by tests and tools.

use super::error::ReportError;
use super::event::ReportEvent;
use super::kind::KindRegistry;
use super::model::{Report, ReportId};
use fid_canon::{digest_bytes, CValue};
use fid_project::Uuid;
use fid_types::Digest;
use std::collections::BTreeMap;

/// One stored report: its RFC 8785 canonical bytes and their SHA-256 digest, keyed by the
/// report ID in the project store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredReport {
    pub canonical: Vec<u8>,
    pub digest: Digest,
}

impl StoredReport {
    /// Serialises a report and digests the bytes.
    pub fn of(report: &Report) -> Result<StoredReport, ReportError> {
        let canonical = report.to_canonical()?;
        let digest = Digest::from_bytes(digest_bytes(&canonical).0);
        Ok(StoredReport { canonical, digest })
    }

    /// Reads a stored report back for `id`. The digest must match the bytes, the bytes must
    /// be the canonical form of a valid report, and its ID must be `id`; anything else is
    /// `report.store_invalid` (fail closed).
    pub fn open(&self, id: &ReportId, kinds: &KindRegistry) -> Result<Report, ReportError> {
        if Digest::from_bytes(digest_bytes(&self.canonical).0) != self.digest {
            return Err(ReportError::StoreInvalid("digest mismatch"));
        }
        let r = Report::from_canonical(&self.canonical, kinds)?;
        if r.id() != *id {
            return Err(ReportError::StoreInvalid("stored under another id"));
        }
        Ok(r)
    }
}

/// The project a report command runs against.
pub trait ReportBackend {
    /// The project's ID (the `project` parameter of `report.create_report@1` must match).
    fn project(&self) -> Uuid;
    /// The stored report, if any.
    fn load(&self, id: &ReportId) -> Result<Option<StoredReport>, ReportError>;
    /// Stores `stored` under `id` and appends `event` to the journal, both or neither.
    fn commit(&mut self, id: &ReportId, stored: StoredReport, event: ReportEvent) -> Result<(), ReportError>;
}

/// An in-memory project: stored reports plus the journal of report events.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryBackend {
    project: Uuid,
    docs: BTreeMap<ReportId, StoredReport>,
    journal: Vec<ReportEvent>,
}

impl MemoryBackend {
    pub fn new(project: Uuid) -> MemoryBackend {
        MemoryBackend { project, docs: BTreeMap::new(), journal: Vec::new() }
    }

    /// Reopens a project from its stored documents and journal (as closing and reopening a
    /// project file would).
    pub fn reopen(project: Uuid, docs: BTreeMap<ReportId, StoredReport>, journal: Vec<ReportEvent>) -> MemoryBackend {
        MemoryBackend { project, docs, journal }
    }

    /// Splits into stored documents and journal.
    pub fn into_parts(self) -> (BTreeMap<ReportId, StoredReport>, Vec<ReportEvent>) {
        (self.docs, self.journal)
    }

    pub fn journal(&self) -> &[ReportEvent] {
        &self.journal
    }

    /// The journal as event values, oldest first (the input of [`super::replay_exact`]).
    pub fn journal_values(&self) -> Vec<CValue> {
        self.journal.iter().map(ReportEvent::to_value).collect()
    }

    pub fn stored(&self, id: &ReportId) -> Option<&StoredReport> {
        self.docs.get(id)
    }
}

impl ReportBackend for MemoryBackend {
    fn project(&self) -> Uuid {
        self.project
    }

    fn load(&self, id: &ReportId) -> Result<Option<StoredReport>, ReportError> {
        Ok(self.docs.get(id).cloned())
    }

    fn commit(&mut self, id: &ReportId, stored: StoredReport, event: ReportEvent) -> Result<(), ReportError> {
        if event.report() != *id || event.report_digest() != stored.digest {
            return Err(ReportError::Backend("event does not match the stored report"));
        }
        self.journal.push(event);
        self.docs.insert(*id, stored);
        Ok(())
    }
}
