//! Jobs for Metrology Maven. Modules land one spec leaf at a time; see `spec/core/job/`.
//!
//! - [`progress`]: progress sink and cancellation token (`CORE.JOB.PROG`);
//! - [`sched`]: job model, queue, priorities, limits and timeouts (`CORE.JOB.SCHED`);
//! - [`commands`]: job commands and the job-status contract (`CORE.JOB.CMD`).

pub mod commands;
pub mod progress;
pub mod sched;

pub use commands::{
    command_defs, context_types, job_status_schema, parse_job_id, parse_job_state, Caller, CancelOutcome, Invocation,
    JobList, JobParams, JobService, JobStatus, JournalEffect, ListJobsParams, PrincipalId, WaitParams, WaitResult,
    CANCEL_JOB, CANCEL_PRECONDITION, CANCEL_PRECONDITION_BOUND, COMMANDS, DEFAULT_WAIT_TIMEOUT, JOB_ID_PATTERN,
    LIST_JOBS, MAX_PARAMS_BYTES, MAX_PARAMS_DEPTH, MAX_PRINCIPAL_ID_BYTES, MAX_WAIT_TIMEOUT, NOT_JOB_OWNER_KEY,
    SHOW_STATUS, WAIT_JOB,
};
pub use progress::{
    CancelToken, Clock, ManualClock, Progress, ProgressSink, ProgressSnapshot, ProgressUpdate, ProgressWarning,
    Publisher, SystemClock, WarningKind, DEFAULT_POLL_INTERVAL, HEARTBEAT_INTERVAL, MAX_RECORDED_WARNINGS,
    MAX_STAGE_CHARS, MIN_PUBLISH_INTERVAL,
};
pub use sched::{
    default_max_running, Execution, Handler, HandlerError, JobClass, JobErrorCode, JobId, JobOpts, JobRecord, JobState,
    Jobs, JobsConfig, LifecycleEvent, LifecycleSink, NullLifecycleSink, PrincipalKind, DEFAULT_QUEUE_CAP,
    DEFAULT_RETAIN_ENDED, DEFAULT_TIMER_POLL, MAX_ERROR_DETAIL_CHARS,
};
