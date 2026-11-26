//! Exact replay of a journal of report, page-setup and block-frame events
//! (`RPT.DESIGN.PLACE.FRAME` REQ-003, AC-5).
//!
//! Events are replayed in journal order. Report events (`RPT.MODEL.DOC`) and page-setup
//! events (`RPT.DESIGN.PAGE`) replay exactly as in [`crate::layout::page::replay_exact`]. A
//! `report.move_block_layout@1` event is re-checked with the handler's rules: the report
//! exists, the block is live on `from_page`, `page` is a live page, and the frame passes
//! [`BlockFrame::check`](super::BlockFrame::check). For a page move the block is moved and the
//! report digest compared with `report_digest`; otherwise the replayed report's digest must
//! already equal it. Then the frame is applied and the digest of the report's frames compared
//! with `frames_digest`. Outcomes:
//!
//! - `NOT_REPLAYABLE`: an event cannot be read, names a block kind the replaying registry
//!   does not have, or lies beyond [`MAX_REPLAY_EVENTS`];
//! - `DIVERGENT`: a check fails on replay or a replayed digest differs;
//! - `IDENTICAL`: every event replays to its recorded digests.
//!
//! Replay stops at the first event that is not identical. The frame is replayed as recorded:
//! snapping happened before the event, so the grid pitch setting is not an input of replay.

use super::event::{is_frame_event, FrameEvent};
use super::model::ReportFrames;
use crate::doc::{KindRegistry, Report, ReportId, StoredReport, MAX_REPLAY_EVENTS};
use crate::layout::page::replay::{layout_step, report_step, Step};
use crate::layout::page::{is_layout_event, ReportLayout};
use fid_canon::CValue;
use fid_project::Uuid;
use fid_replay::ReplayOutcome;
use fid_types::Digest;
use std::collections::BTreeMap;

/// The outcome of an exact replay, with the reports, page setups and frames it rebuilt.
#[derive(Debug, Clone, PartialEq)]
pub struct FrameReplayRun {
    pub outcome: ReplayOutcome,
    /// Index of the first event that was not identical, if any.
    pub first_mismatch: Option<usize>,
    /// Replayed reports after the last identical event, with their stored digests.
    pub reports: BTreeMap<ReportId, (Report, Digest)>,
    /// Replayed page setups after the last identical event.
    pub layouts: BTreeMap<ReportId, ReportLayout>,
    /// Replayed block frames after the last identical event.
    pub frames: BTreeMap<ReportId, ReportFrames>,
}

/// Replays `events` (oldest first) of project `project` in exact mode.
pub fn replay_exact(events: &[CValue], kinds: &KindRegistry, project: Uuid) -> FrameReplayRun {
    let mut reports: BTreeMap<ReportId, (Report, Digest)> = BTreeMap::new();
    let mut layouts: BTreeMap<ReportId, ReportLayout> = BTreeMap::new();
    let mut frames: BTreeMap<ReportId, ReportFrames> = BTreeMap::new();
    for (i, v) in events.iter().enumerate() {
        let step = if i >= MAX_REPLAY_EVENTS {
            Step::NotReplayable
        } else if is_frame_event(v) {
            frame_step(v, &mut reports, &mut frames)
        } else if is_layout_event(v) {
            layout_step(v, &reports, &mut layouts)
        } else {
            report_step(v, kinds, project, &mut reports)
        };
        let outcome = match step {
            Step::Ok => continue,
            Step::NotReplayable => ReplayOutcome::NotReplayable,
            Step::Divergent => ReplayOutcome::Divergent,
        };
        return FrameReplayRun { outcome, first_mismatch: Some(i), reports, layouts, frames };
    }
    FrameReplayRun { outcome: ReplayOutcome::Identical, first_mismatch: None, reports, layouts, frames }
}

fn frame_step(
    v: &CValue,
    reports: &mut BTreeMap<ReportId, (Report, Digest)>,
    frames: &mut BTreeMap<ReportId, ReportFrames>,
) -> Step {
    let Ok(ev) = FrameEvent::from_value(v) else { return Step::NotReplayable };
    let Some((report, digest)) = reports.get(&ev.report) else { return Step::Divergent };
    let block = ev.block.uuid();
    if report.block(&block).is_none() || report.page_of_block(&block) != Some(ev.from_page) {
        return Step::Divergent;
    }
    if !report.pages().iter().any(|p| p.id() == ev.page) || ev.frame.check().is_err() {
        return Step::Divergent;
    }
    let (report, digest) = if ev.page == ev.from_page {
        (report.clone(), *digest)
    } else {
        let mut r = report.clone();
        if r.move_block_to_page(&block, &ev.page.uuid()).is_err() {
            return Step::Divergent;
        }
        match StoredReport::of(&r) {
            Ok(s) => (r, s.digest),
            Err(_) => return Step::Divergent,
        }
    };
    if digest != ev.report_digest {
        return Step::Divergent;
    }
    let mut next = frames.get(&ev.report).cloned().unwrap_or_default();
    next.frames.insert(ev.block, ev.frame);
    match next.digest() {
        Ok(d) if d == ev.frames_digest => {
            frames.insert(ev.report, next);
            reports.insert(ev.report, (report, digest));
            Step::Ok
        }
        _ => Step::Divergent,
    }
}
