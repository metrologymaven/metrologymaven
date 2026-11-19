//! Exact replay of report events (REQ-011, AC-7).
//!
//! Each event is read back, its operation applied with the IDs it holds (nothing is
//! drawn), and the digest of the replayed stored report compared with the event's
//! `report_digest`. Outcomes (CONSTITUTION §4):
//!
//! - `NOT_REPLAYABLE`: an event cannot be read, or names a block kind (or kind version)
//!   that the replaying registry does not have;
//! - `DIVERGENT`: a `create_report` event names another project than the one replayed,
//!   an operation fails on replay (including the value-binding rule), or a replayed digest
//!   differs;
//! - `IDENTICAL`: every event replays to its recorded digest.
//!
//! Replay stops at the first event that is not identical.

use super::backend::StoredReport;
use super::event::{Op, ReportEvent};
use super::kind::KindRegistry;
use super::model::{Report, ReportId};
use fid_canon::CValue;
use fid_project::Uuid;
use fid_replay::ReplayOutcome;
use fid_types::Digest;
use std::collections::BTreeMap;

/// Most events one replay call accepts; more is `NOT_REPLAYABLE` at the first extra event.
pub const MAX_REPLAY_EVENTS: usize = 1_000_000;

/// The outcome of an exact replay and the replayed reports with their digests.
#[derive(Debug, Clone, PartialEq)]
pub struct ReplayRun {
    pub outcome: ReplayOutcome,
    /// Index of the first event that was not identical, if any.
    pub first_mismatch: Option<usize>,
    /// Replayed reports after the last identical event, with their stored digests.
    pub reports: BTreeMap<ReportId, (Report, Digest)>,
}

/// Replays `events` (oldest first) of project `project` in exact mode.
pub fn replay_exact(events: &[CValue], kinds: &KindRegistry, project: Uuid) -> ReplayRun {
    let mut reports: BTreeMap<ReportId, (Report, Digest)> = BTreeMap::new();
    for (i, v) in events.iter().enumerate() {
        let stop = |outcome| ReplayRun { outcome, first_mismatch: Some(i), reports: BTreeMap::new() };
        if i >= MAX_REPLAY_EVENTS {
            return ReplayRun { reports, ..stop(ReplayOutcome::NotReplayable) };
        }
        let Ok(ev) = ReportEvent::from_value(v) else {
            return ReplayRun { reports, ..stop(ReplayOutcome::NotReplayable) };
        };
        if let Op::AddBlock { kind, kind_version, .. } = ev.op() {
            if kinds.get(kind).map(|(v, _)| v) != Some(*kind_version) {
                return ReplayRun { reports, ..stop(ReplayOutcome::NotReplayable) };
            }
        }
        if let Op::CreateReport { project: p, .. } = ev.op() {
            if *p != project {
                return ReplayRun { reports, ..stop(ReplayOutcome::Divergent) };
            }
        }
        let id = ev.report();
        let state = reports.get(&id).map(|(r, _)| r.clone());
        let replayed = ev.op().apply(state, kinds).and_then(|r| StoredReport::of(&r).map(|s| (r, s.digest)));
        match replayed {
            Ok((r, d)) if d == ev.report_digest() => {
                reports.insert(id, (r, d));
            }
            _ => return ReplayRun { reports, ..stop(ReplayOutcome::Divergent) },
        }
    }
    ReplayRun { outcome: ReplayOutcome::Identical, first_mismatch: None, reports }
}
