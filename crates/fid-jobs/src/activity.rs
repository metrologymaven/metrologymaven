//! The job [`LifecycleSink`] of the Activity Log (spec node `CORE.JRN.STREAM.ALOG`, REQ-004).
//!
//! Every lifecycle change goes to the Activity Log whether or not a recording session is
//! active: the sink does not consult the recording flag at all. A failed write cannot reach the
//! scheduler (`emit` returns nothing), so it is counted by the log
//! (`ActivityLog::warning_count`, `ActivityLog::take_last_warning`) and never fails the job
//! (REQ-008).
//!
//! The implementation sits here rather than in `fid-journal` because `fid-journal` (layer
//! model) may not depend on `fid-jobs` (layer engine).

use crate::sched::{JobErrorCode, JobState, LifecycleEvent, LifecycleSink};
use fid_journal::stream::{ActivityLog, JobChange};

/// The [`JobChange`] the Activity Log records for `event`.
pub fn job_change<'a>(event: &'a LifecycleEvent, job: &'a str) -> JobChange<'a> {
    JobChange {
        job,
        command: event.command.as_str(),
        principal_kind: event.principal_kind.as_str(),
        from: event.from.map(JobState::as_str),
        to: event.to.as_str(),
        error: event.error.map(JobErrorCode::code),
        at_ns: u64::try_from(event.at.as_nanos()).unwrap_or(u64::MAX),
    }
}

impl LifecycleSink for ActivityLog {
    fn emit(&self, event: &LifecycleEvent) {
        let job = event.job.to_string();
        // A failure is recorded by the log itself; the job is unaffected (REQ-008).
        let _ = self.record_lifecycle(&job_change(event, &job));
    }
}
