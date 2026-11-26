//! Storage and journaling seam of page setup.
//!
//! A [`LayoutBackend`] is a [`ReportBackend`] (the reports of `RPT.MODEL.DOC`) that also
//! stores each report's [`ReportLayout`] and commits it with its [`LayoutEvent`] atomically
//! (the command bus does this in production). [`MemoryProject`] is the in-memory backend of
//! tests and tools: reports, page setups and one journal holding both kinds of events in
//! the order they were committed, which [`super::replay_exact`] reads.
//!
//! The page setup is kept beside the report document, not inside it, so a layout command
//! cannot change a block, a binding, a value or a report digest (REQ-RPT.DESIGN-007).

use super::error::LayoutError;
use super::event::LayoutEvent;
use super::model::ReportLayout;
use crate::doc::{MemoryBackend, ReportBackend, ReportError, ReportEvent, ReportId, StoredReport};
use fid_canon::CValue;
use fid_project::Uuid;
use std::collections::BTreeMap;

/// A project backend that also stores page setups.
pub trait LayoutBackend: ReportBackend {
    /// The stored page setup of `report`; `None` when none was ever set.
    fn load_layout(&self, report: &ReportId) -> Result<Option<ReportLayout>, LayoutError>;
    /// Stores `layout` for `report` and appends `event` to the journal, both or neither.
    fn commit_layout(&mut self, report: &ReportId, layout: ReportLayout, event: LayoutEvent)
        -> Result<(), LayoutError>;
}

/// An in-memory project: the reports of a [`MemoryBackend`], their page setups and one
/// journal of report and page-setup events, oldest first.
#[derive(Debug, Clone, PartialEq)]
pub struct MemoryProject {
    docs: MemoryBackend,
    layouts: BTreeMap<ReportId, ReportLayout>,
    journal: Vec<CValue>,
}

impl MemoryProject {
    pub fn new(project: Uuid) -> MemoryProject {
        MemoryProject { docs: MemoryBackend::new(project), layouts: BTreeMap::new(), journal: Vec::new() }
    }

    /// The journal as event values, oldest first (the input of [`super::replay_exact`]).
    pub fn journal(&self) -> &[CValue] {
        &self.journal
    }

    /// The report store.
    pub fn docs(&self) -> &MemoryBackend {
        &self.docs
    }

    /// The stored page setup of `report`, if one was ever set.
    pub fn layout(&self, report: &ReportId) -> Option<&ReportLayout> {
        self.layouts.get(report)
    }
}

impl ReportBackend for MemoryProject {
    fn project(&self) -> Uuid {
        self.docs.project()
    }

    fn load(&self, id: &ReportId) -> Result<Option<StoredReport>, ReportError> {
        self.docs.load(id)
    }

    fn commit(&mut self, id: &ReportId, stored: StoredReport, event: ReportEvent) -> Result<(), ReportError> {
        let value = event.to_value();
        self.docs.commit(id, stored, event)?;
        self.journal.push(value);
        Ok(())
    }
}

impl LayoutBackend for MemoryProject {
    fn load_layout(&self, report: &ReportId) -> Result<Option<ReportLayout>, LayoutError> {
        Ok(self.layouts.get(report).cloned())
    }

    fn commit_layout(
        &mut self,
        report: &ReportId,
        layout: ReportLayout,
        event: LayoutEvent,
    ) -> Result<(), LayoutError> {
        if event.report != *report || layout.digest()? != event.layout_digest {
            return Err(LayoutError::StoreInvalid("event does not match the stored page setup"));
        }
        self.journal.push(event.to_value());
        self.layouts.insert(*report, layout);
        Ok(())
    }
}
