//! `FakeBus`: a deterministic bus double for the AUTO.SURF.JOBS tests.
//!
//! It records every request, applies a scripted send result, and models the bus side:
//! acceptance (or a dropped acceptance), explicit refusal, progress, commit with the
//! idempotency rule (one commit per key, replayed on resend), and cancellation.
#![allow(dead_code, clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::CValue;
use fid_surface::jobs::{BusEvent, BusRequest, BusTransport, Purpose, SendError};
use fid_surface::{Envelope, Outcome};
use fid_uuid::Uuid;
use std::collections::{BTreeMap, VecDeque};

/// What one send does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Send {
    /// Delivered; `Ok`.
    Ok,
    /// Nothing left; `Err(NotSent)`.
    NotSent,
    /// Delivered, but the transport reports `Err(Unknown)`.
    UnknownDelivered,
    /// Lost, and the transport reports `Err(Unknown)`.
    UnknownLost,
    /// Lost in the network although the transport reported `Ok`.
    LostOk,
}

#[derive(Debug, Clone)]
pub struct Job {
    pub key: Uuid,
    pub command: String,
    pub result: Option<Envelope>,
}

#[derive(Debug, Default)]
pub struct FakeBus {
    pub sent: Vec<BusRequest>,
    pub inbox: VecDeque<BusEvent>,
    /// Scripted send results, used in order; `Send::Ok` when empty.
    pub script: VecDeque<Send>,
    /// Bus side: the request is processed but the acceptance message is dropped.
    pub drop_accept: bool,
    /// Bus side: refuse every submit with this reason.
    pub refuse: Option<String>,
    /// Bus side: a cancel request finishes the job as `cancelled`.
    pub cancel_finishes: bool,
    /// Bus side: do nothing at all with delivered requests (tests inject events by hand).
    pub passive: bool,
    /// Hold bus messages in `pending` instead of `inbox` (tests deliver or drop them).
    pub hold: bool,
    pub pending: VecDeque<BusEvent>,
    pub jobs: BTreeMap<Uuid, Job>,
    pub by_key: BTreeMap<Uuid, Uuid>,
    pub commits: u32,
    pub cancels_received: u32,
    next_job: u128,
}

pub const HASH: &str = "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

pub fn job_id(n: u128) -> Uuid {
    // Job IDs look like UUIDv7s with a fixed timestamp.
    fid_surface::jobs::uuid_v7(0x0190_0000_0000, n)
}

impl FakeBus {
    pub fn new() -> Self {
        Self::default()
    }

    /// Publishes progress for `job`.
    pub fn progress(&mut self, job: Uuid, fraction: f64, stage: &str) {
        self.emit(BusEvent::Progress { job, fraction, stage: stage.to_string() });
    }

    /// Commits the job (once) and publishes its final state.
    pub fn commit(&mut self, job: Uuid) {
        let j = self.jobs.get_mut(&job).unwrap();
        assert!(j.result.is_none(), "a job commits at most once");
        let env =
            Envelope { journal_entry_hash: Some(HASH.to_string()), ..Envelope::bare(&j.command, Outcome::Committed) };
        j.result = Some(env.clone());
        let key = j.key;
        self.commits += 1;
        self.emit(BusEvent::Finished { key, job, envelope: Box::new(env) });
    }

    pub fn job_of(&self, key: Uuid) -> Option<Uuid> {
        self.by_key.get(&key).copied()
    }

    pub fn sent_with(&self, purpose: Purpose) -> Vec<&BusRequest> {
        self.sent.iter().filter(|r| r.purpose == purpose).collect()
    }

    fn emit(&mut self, e: BusEvent) {
        if self.hold {
            self.pending.push_back(e);
        } else {
            self.inbox.push_back(e);
        }
    }

    /// Moves the oldest held message to the inbox.
    pub fn deliver_one(&mut self) {
        if let Some(e) = self.pending.pop_front() {
            self.inbox.push_back(e);
        }
    }

    /// Drops the oldest held message.
    pub fn drop_one(&mut self) {
        self.pending.pop_front();
    }

    fn deliver(&mut self, r: &BusRequest) {
        if self.passive {
            return;
        }
        match r.purpose {
            Purpose::Submit => {
                if let Some(reason) = self.refuse.clone() {
                    self.emit(BusEvent::Refused { key: r.key, attempt: r.attempt, reason });
                    return;
                }
                // Idempotency rule: a known key gets the original job (and result), no new commit.
                let job = match self.by_key.get(&r.key) {
                    Some(job) => *job,
                    None => {
                        self.next_job += 1;
                        let job = job_id(self.next_job);
                        self.by_key.insert(r.key, job);
                        self.jobs
                            .insert(job, Job { key: r.key, command: r.command.as_str().to_string(), result: None });
                        job
                    }
                };
                if !self.drop_accept {
                    self.emit(BusEvent::Accepted { key: r.key, job });
                }
                if let Some(env) = self.jobs.get(&job).and_then(|j| j.result.clone()) {
                    self.emit(BusEvent::Finished { key: r.key, job, envelope: Box::new(env) });
                }
            }
            Purpose::Cancel => {
                self.cancels_received += 1;
                let Some(CValue::Str(s)) = (match &r.params {
                    CValue::Obj(o) => o.get("job").cloned(),
                    _ => None,
                }) else {
                    panic!("cancel without job")
                };
                let job = Uuid::parse(&s).unwrap();
                if self.cancel_finishes {
                    let j = self.jobs.get_mut(&job).unwrap();
                    if j.result.is_none() {
                        let env =
                            Envelope { job_id: Some(s.clone()), ..Envelope::bare(&j.command, Outcome::Cancelled) };
                        j.result = Some(env.clone());
                        let key = j.key;
                        self.emit(BusEvent::Finished { key, job, envelope: Box::new(env) });
                    }
                }
            }
            Purpose::Wait => {
                let Some(CValue::Str(s)) = (match &r.params {
                    CValue::Obj(o) => o.get("job").cloned(),
                    _ => None,
                }) else {
                    panic!("wait without job")
                };
                let job = Uuid::parse(&s).unwrap();
                if let Some((key, Some(env))) = self.jobs.get(&job).map(|j| (j.key, j.result.clone())) {
                    self.emit(BusEvent::Finished { key, job, envelope: Box::new(env) });
                }
            }
        }
    }
}

impl BusTransport for FakeBus {
    fn send(&mut self, request: &BusRequest) -> Result<(), SendError> {
        let how = self.script.pop_front().unwrap_or(Send::Ok);
        if how == Send::NotSent {
            return Err(SendError::NotSent);
        }
        self.sent.push(request.clone());
        if matches!(how, Send::Ok | Send::UnknownDelivered) {
            self.deliver(request);
        }
        match how {
            Send::Ok | Send::LostOk => Ok(()),
            _ => Err(SendError::Unknown),
        }
    }

    fn recv(&mut self) -> Option<BusEvent> {
        self.inbox.pop_front()
    }
}
