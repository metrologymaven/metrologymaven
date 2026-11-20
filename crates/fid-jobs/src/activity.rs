//! The job [`LifecycleSink`] of the Activity Log (spec node `CORE.JRN.STREAM.ALOG`, REQ-004).
//!
//! Every lifecycle change goes to the Activity Log whether or not a recording session is
//! active: the sink does not consult the recording flag at all. A failed write cannot reach the
//! scheduler (`emit` returns nothing), so it is counted by the log
//! (`ActivityLog::warning_count`, `ActivityLog::take_last_warning`) and never fails the job
//! (REQ-008).
//!
//! **No file I/O under the scheduler's lock.** [`Jobs`](crate::Jobs) calls `emit` while it
//! holds its state lock, so that events arrive in the order the changes happened. The
//! [`ActivityLogSink`] therefore only queues a copy of the event on a bounded channel
//! ([`QUEUE_DEPTH`]) and one writer thread writes the queue to the log, in order. `emit` blocks
//! only when the writer is [`QUEUE_DEPTH`] events behind. [`ActivityLogSink::flush`] waits
//! until everything queued so far is written; dropping the sink drains the queue and stops the
//! thread.
//!
//! The implementation sits here rather than in `fid-journal` because `fid-journal` (layer
//! model) may not depend on `fid-jobs` (layer engine).

use crate::sched::{JobErrorCode, JobState, LifecycleEvent, LifecycleSink};
use fid_journal::stream::{ActivityLog, JobChange};
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::sync::Arc;
use std::thread::JoinHandle;

/// Most events waiting for the writer thread before `emit` blocks.
pub const QUEUE_DEPTH: usize = 4096;

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
        wall: Some(event.wall),
    }
}

fn write(log: &ActivityLog, event: &LifecycleEvent) {
    let job = event.job.to_string();
    // A failure is recorded by the log itself; the job is unaffected (REQ-008).
    let _ = log.record_lifecycle(&job_change(event, &job));
}

enum Msg {
    Event(Box<LifecycleEvent>),
    Flush(mpsc::Sender<()>),
}

/// A [`LifecycleSink`] that writes to an [`ActivityLog`] from its own thread.
pub struct ActivityLogSink {
    log: Arc<ActivityLog>,
    tx: Option<SyncSender<Msg>>,
    worker: Option<JoinHandle<()>>,
}

impl std::fmt::Debug for ActivityLogSink {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ActivityLogSink").field("log", &self.log).finish_non_exhaustive()
    }
}

impl ActivityLogSink {
    /// Starts the writer thread. Fails only if the thread cannot be started.
    pub fn spawn(log: Arc<ActivityLog>) -> std::io::Result<ActivityLogSink> {
        let (tx, rx) = mpsc::sync_channel::<Msg>(QUEUE_DEPTH);
        let writer_log = Arc::clone(&log);
        let worker = std::thread::Builder::new()
            .name("fid-jobs-activity-log".to_string())
            .spawn(move || run(&writer_log, &rx))?;
        Ok(ActivityLogSink { log, tx: Some(tx), worker: Some(worker) })
    }

    /// The log written to.
    pub fn log(&self) -> &Arc<ActivityLog> {
        &self.log
    }

    /// Waits until every event queued before this call is written.
    pub fn flush(&self) {
        let (done_tx, done_rx) = mpsc::channel();
        let sent = self.tx.as_ref().is_some_and(|tx| tx.send(Msg::Flush(done_tx)).is_ok());
        if sent {
            let _ = done_rx.recv();
        }
    }
}

fn run(log: &ActivityLog, rx: &Receiver<Msg>) {
    while let Ok(msg) = rx.recv() {
        match msg {
            Msg::Event(event) => write(log, &event),
            Msg::Flush(done) => {
                let _ = done.send(());
            }
        }
    }
}

impl LifecycleSink for ActivityLogSink {
    fn emit(&self, event: &LifecycleEvent) {
        let queued = self.tx.as_ref().is_some_and(|tx| tx.send(Msg::Event(Box::new(event.clone()))).is_ok());
        if !queued {
            // The writer thread is gone: write here rather than lose the change.
            write(&self.log, event);
        }
    }
}

impl Drop for ActivityLogSink {
    fn drop(&mut self) {
        // Closing the channel lets the writer drain the queue and stop.
        drop(self.tx.take());
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
