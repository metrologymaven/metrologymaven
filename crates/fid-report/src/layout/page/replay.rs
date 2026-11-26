//! Exact replay of a journal of report and page-setup events (`RPT.DESIGN.PAGE` REQ-007,
//! AC-6).
//!
//! Events are replayed in journal order, so a page override is checked against the report
//! as it was when the override was set. Report events (`RPT.MODEL.DOC`) replay as in
//! [`crate::doc::replay_exact`]: the operation is applied with the IDs the event holds and
//! the stored digest compared. A `report.set_page_layout@1` event is re-checked with the
//! handler's rules (the report exists, the page is live, the layout passes
//! [`PageLayout::check`](super::PageLayout::check)), applied, and the digest of the
//! report's page setup compared with the event's `layout_digest`. Outcomes:
//!
//! - `NOT_REPLAYABLE`: an event cannot be read, names a block kind the replaying registry
//!   does not have, or lies beyond [`MAX_REPLAY_EVENTS`];
//! - `DIVERGENT`: a check fails on replay or a replayed digest differs;
//! - `IDENTICAL`: every event replays to its recorded digest.
//!
//! Replay stops at the first event that is not identical.

use super::command::{live_page, with_layout};
use super::event::{is_layout_event, LayoutEvent};
use super::model::ReportLayout;
use crate::doc::{KindRegistry, Op, Report, ReportEvent, ReportId, StoredReport, MAX_REPLAY_EVENTS};
use fid_canon::CValue;
use fid_project::Uuid;
use fid_replay::ReplayOutcome;
use fid_types::Digest;
use std::collections::BTreeMap;

/// The outcome of an exact replay, with the reports and page setups it rebuilt.
#[derive(Debug, Clone, PartialEq)]
pub struct LayoutReplayRun {
    pub outcome: ReplayOutcome,
    /// Index of the first event that was not identical, if any.
    pub first_mismatch: Option<usize>,
    /// Replayed reports after the last identical event, with their stored digests.
    pub reports: BTreeMap<ReportId, (Report, Digest)>,
    /// Replayed page setups after the last identical event.
    pub layouts: BTreeMap<ReportId, ReportLayout>,
}

/// The outcome of replaying one event (shared with [`crate::layout::frame::replay_exact`]).
pub(crate) enum Step {
    Ok,
    NotReplayable,
    Divergent,
}

/// Replays `events` (oldest first) of project `project` in exact mode.
pub fn replay_exact(events: &[CValue], kinds: &KindRegistry, project: Uuid) -> LayoutReplayRun {
    let mut reports: BTreeMap<ReportId, (Report, Digest)> = BTreeMap::new();
    let mut layouts: BTreeMap<ReportId, ReportLayout> = BTreeMap::new();
    for (i, v) in events.iter().enumerate() {
        let step = if i >= MAX_REPLAY_EVENTS {
            Step::NotReplayable
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
        return LayoutReplayRun { outcome, first_mismatch: Some(i), reports, layouts };
    }
    LayoutReplayRun { outcome: ReplayOutcome::Identical, first_mismatch: None, reports, layouts }
}

pub(crate) fn report_step(
    v: &CValue,
    kinds: &KindRegistry,
    project: Uuid,
    reports: &mut BTreeMap<ReportId, (Report, Digest)>,
) -> Step {
    let Ok(ev) = ReportEvent::from_value(v) else { return Step::NotReplayable };
    if let Op::AddBlock { kind, kind_version, .. } = ev.op() {
        if kinds.get(kind).map(|(v, _)| v) != Some(*kind_version) {
            return Step::NotReplayable;
        }
    }
    if let Op::CreateReport { project: p, .. } = ev.op() {
        if *p != project {
            return Step::Divergent;
        }
    }
    let id = ev.report();
    let state = reports.get(&id).map(|(r, _)| r.clone());
    match ev.op().apply(state, kinds).and_then(|r| StoredReport::of(&r).map(|s| (r, s.digest))) {
        Ok((r, d)) if d == ev.report_digest() => {
            reports.insert(id, (r, d));
            Step::Ok
        }
        _ => Step::Divergent,
    }
}

pub(crate) fn layout_step(
    v: &CValue,
    reports: &BTreeMap<ReportId, (Report, Digest)>,
    layouts: &mut BTreeMap<ReportId, ReportLayout>,
) -> Step {
    let Ok(ev) = LayoutEvent::from_value(v) else { return Step::NotReplayable };
    let Some((report, _)) = reports.get(&ev.report) else { return Step::Divergent };
    if let Some(page) = ev.page {
        if live_page(report, &page.uuid()) != Some(page) {
            return Step::Divergent;
        }
    }
    if ev.layout.check().is_err() {
        return Step::Divergent;
    }
    let current = layouts.get(&ev.report).cloned().unwrap_or_default();
    let next = with_layout(&current, ev.page, ev.layout);
    match next.digest() {
        Ok(d) if d == ev.layout_digest => {
            layouts.insert(ev.report, next);
            Step::Ok
        }
        _ => Step::Divergent,
    }
}
