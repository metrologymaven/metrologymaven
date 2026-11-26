//! Storage and journaling seam of block frames.
//!
//! A [`FrameBackend`] is a [`LayoutBackend`] that also stores each report's
//! [`ReportFrames`] and commits them, with the report when the block moved page, and their
//! [`FrameEvent`] atomically (the command bus does this in production).
//! [`MemoryProject`] implements it in memory, with frame events in the same journal as
//! report and page-setup events.

use super::event::FrameEvent;
use super::model::ReportFrames;
use crate::doc::{ReportBackend, ReportId, StoredReport};
use crate::layout::page::{LayoutBackend, LayoutError, MemoryProject};

/// A project backend that also stores block frames.
pub trait FrameBackend: LayoutBackend {
    /// The stored frames of `report`; `None` when no block of it ever had a frame.
    fn load_frames(&self, report: &ReportId) -> Result<Option<ReportFrames>, LayoutError>;
    /// Stores `frames` for `report` (and `moved`, the report after a page move, when the
    /// block changed page) and appends `event` to the journal: all or nothing.
    fn commit_frames(
        &mut self,
        report: &ReportId,
        moved: Option<StoredReport>,
        frames: ReportFrames,
        event: FrameEvent,
    ) -> Result<(), LayoutError>;
}

impl MemoryProject {
    /// The stored frames of `report`, if any block of it ever had a frame.
    pub fn frames(&self, report: &ReportId) -> Option<&ReportFrames> {
        self.frames.get(report)
    }
}

impl FrameBackend for MemoryProject {
    fn load_frames(&self, report: &ReportId) -> Result<Option<ReportFrames>, LayoutError> {
        Ok(self.frames.get(report).cloned())
    }

    fn commit_frames(
        &mut self,
        report: &ReportId,
        moved: Option<StoredReport>,
        frames: ReportFrames,
        event: FrameEvent,
    ) -> Result<(), LayoutError> {
        let mismatch = || LayoutError::StoreInvalid("event does not match the stored frames");
        if event.report != *report || frames.digest()? != event.frames_digest {
            return Err(mismatch());
        }
        let report_digest = match &moved {
            Some(s) => s.digest,
            None => self.load(report)?.ok_or(LayoutError::NotFound(report.uuid()))?.digest,
        };
        if report_digest != event.report_digest || moved.is_some() == (event.page == event.from_page) {
            return Err(mismatch());
        }
        self.journal.push(event.to_value());
        if let Some(stored) = moved {
            self.docs.put_stored(*report, stored);
        }
        self.frames.insert(*report, frames);
        Ok(())
    }
}
