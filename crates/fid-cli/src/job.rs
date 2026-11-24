//! Terminal progress rendering and interrupt-to-cancel loop (spec node `AUTO.CLI.JOB`).

use fid_surface::jobs::{BusTransport, KeySource, MonotonicClock, ProgressSubscriber, ProgressUpdate, State, Tracker};
use fid_surface::{Envelope, ErrorCode, Outcome};
use fid_types::CommandId;
use std::io::Write;
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

/// Poll interval between tracker pumps while awaiting completion.
const PUMP_SLEEP: Duration = Duration::from_millis(10);

/// Interrupt state machine (`REQ-AUTO.CLI.JOB-002`, `-003`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InterruptState {
    None,
    CancelRequested,
    Detached,
}

/// Source of user interrupt events (`SIGINT` / Ctrl-C / test hook).
pub trait InterruptSource {
    fn poll_interrupt(&mut self) -> bool;
}

/// No-op interrupt source when no OS signal hook is attached.
#[derive(Debug, Default, Clone, Copy)]
pub struct NoInterrupts;

impl InterruptSource for NoInterrupts {
    fn poll_interrupt(&mut self) -> bool {
        false
    }
}

/// Channel-backed interrupt source for tests and signal forwarders.
pub struct ChannelInterruptSource {
    rx: Receiver<()>,
}

impl ChannelInterruptSource {
    pub fn new(rx: Receiver<()>) -> Self {
        Self { rx }
    }
}

impl InterruptSource for ChannelInterruptSource {
    fn poll_interrupt(&mut self) -> bool {
        self.rx.try_recv().is_ok()
    }
}

/// Progress subscriber writing job updates to `stderr` rate-limited for non-TTY pipes
/// (REQ-AUTO.CLI.JOB-001, AC-4).
pub struct StderrProgress<'a> {
    stderr: &'a mut dyn Write,
    is_tty: bool,
    last_emit: Option<Instant>,
    pub updates_written: usize,
}

impl<'a> StderrProgress<'a> {
    pub fn new(stderr: &'a mut dyn Write, is_tty: bool) -> Self {
        Self { stderr, is_tty, last_emit: None, updates_written: 0 }
    }

    fn should_emit(&mut self, now: Instant) -> bool {
        match self.last_emit {
            None => {
                self.last_emit = Some(now);
                true
            }
            Some(prev) => {
                // Non-TTY emits plain lines at most once per second; TTY redraws freely.
                let min_gap = if self.is_tty { Duration::from_millis(100) } else { Duration::from_secs(1) };
                if now.saturating_duration_since(prev) >= min_gap {
                    self.last_emit = Some(now);
                    true
                } else {
                    false
                }
            }
        }
    }
}

impl ProgressSubscriber for StderrProgress<'_> {
    fn progress(&mut self, _id: fid_surface::jobs::InvocationId, update: &ProgressUpdate) {
        if !self.should_emit(Instant::now()) {
            return;
        }
        let pct = (update.fraction.clamp(0.0, 1.0) * 100.0).round() as u32;
        if self.is_tty {
            let _ = write!(self.stderr, "\rprogress: {pct:3}% {}", update.stage);
        } else {
            let _ = writeln!(self.stderr, "progress: {pct:3}% {}", update.stage);
        }
        let _ = self.stderr.flush();
        self.updates_written += 1;
    }
}

/// Runs a single command through `AUTO.SURF.JOBS` [`Tracker`], rendering progress on `stderr`
/// and mapping 1st/2nd interrupts to `job.cancel_job@1` / `no_reply` detach
/// (`REQ-AUTO.CLI.JOB-001..004`).
pub fn run_tracked_invocation<T, C, K>(
    tracker: &mut Tracker<T, C, K>,
    command: CommandId,
    params: fid_canon::CValue,
    timeout: Duration,
    interrupts: &mut dyn InterruptSource,
    is_tty: bool,
    stderr: &mut dyn Write,
) -> Envelope
where
    T: BusTransport,
    C: MonotonicClock,
    K: KeySource,
{
    let inv_id = match tracker.submit(command.clone(), params, timeout) {
        Ok(id) => id,
        Err(err) => {
            return Envelope::failed(
                command.as_str(),
                ErrorCode::PreconditionFailed,
                &format!("failed to submit job: {err}"),
                None,
            );
        }
    };

    let mut int_state = InterruptState::None;
    let mut progress = StderrProgress::new(stderr, is_tty);

    loop {
        let _pumped = tracker.pump(&mut progress);

        if let Some(State::Finished(report)) = tracker.state(inv_id) {
            let env = report.envelope.clone();
            // REQ-AUTO.CLI.JOB-004: never exit 0 after interrupt unless committed/computed.
            if int_state != InterruptState::None
                && !matches!(env.outcome, Outcome::Committed | Outcome::Computed | Outcome::DryRun)
                && matches!(env.outcome, Outcome::Accepted)
            {
                return Envelope { job_id: env.job_id, ..Envelope::bare(command.as_str(), Outcome::NoReply) };
            }
            return env;
        }

        if interrupts.poll_interrupt() {
            match int_state {
                InterruptState::None => {
                    int_state = InterruptState::CancelRequested;
                    let _ = tracker.request_cancel(inv_id);
                }
                InterruptState::CancelRequested => {
                    let job_id = tracker.evidence(inv_id).and_then(|e| e.job).map(|u| u.to_string());
                    if let Some(jid) = &job_id {
                        let _ = writeln!(progress.stderr, "detached from job {jid}");
                    } else {
                        let _ = writeln!(progress.stderr, "detached from unaccepted invocation");
                    }
                    let _ = progress.stderr.flush();
                    return Envelope { job_id, ..Envelope::bare(command.as_str(), Outcome::NoReply) };
                }
                InterruptState::Detached => {}
            }
        }

        std::thread::sleep(PUMP_SLEEP);
    }
}
