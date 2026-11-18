//! Jobs for Metrology Maven. Modules land one spec leaf at a time; see `spec/core/job/`.
//!
//! - [`progress`]: progress sink and cancellation token (`CORE.JOB.PROG`);
//! - [`sched`]: job model, queue, priorities, limits and timeouts (`CORE.JOB.SCHED`).

pub mod progress;
pub mod sched;

pub use progress::{
    CancelToken, Clock, ManualClock, Progress, ProgressSink, ProgressSnapshot, ProgressUpdate, ProgressWarning,
    Publisher, SystemClock, WarningKind, DEFAULT_POLL_INTERVAL, HEARTBEAT_INTERVAL, MAX_RECORDED_WARNINGS,
    MAX_STAGE_CHARS, MIN_PUBLISH_INTERVAL,
};
pub use sched::{
    default_max_running, Execution, Handler, HandlerError, JobClass, JobErrorCode, JobId, JobOpts, JobRecord,
    JobState, Jobs, JobsConfig, LifecycleEvent, LifecycleSink, NullLifecycleSink, PrincipalKind, DEFAULT_QUEUE_CAP,
    DEFAULT_TIMER_POLL, MAX_ERROR_DETAIL_CHARS,
};
