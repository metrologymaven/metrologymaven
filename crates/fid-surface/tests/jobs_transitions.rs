//! AUTO.SURF.JOBS: every transition, driven by hand-injected (possibly adversarial) bus
//! messages. All sequences up to length 3 over the whole alphabet, from each first-send
//! result, then 20,000 seeded sequences of length 16, each step checked against
//! invariants computed from what the test itself injected.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use common::{job_id, FakeBus, Send, HASH};
use fid_canon::CValue;
use fid_surface::jobs::{BusEvent, CountingKeySource, ManualClock, Purpose, Report, State, Tracker};
use fid_surface::{Envelope, Outcome};
use fid_types::CommandId;
use std::collections::BTreeSet;
use std::time::Duration;

const CMD: &str = "probe.measure_point@1";
const TIMEOUT: Duration = Duration::from_millis(500);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Step {
    AcceptJ1,
    AcceptJ2,
    Refuse0,
    Refuse1,
    Progress,
    ProgressNaN,
    FinishCommitted,
    FinishNotRun,
    FinishCancelled,
    FinishOtherJob,
    Disconnect,
    Reconnect,
    Timeout,
    ClockBack,
    ResendOk,
    ResendNotSent,
    ResendUnknown,
    Cancel,
    Reattach,
    UnknownKey,
}

const ALPHABET: [Step; 20] = [
    Step::AcceptJ1,
    Step::AcceptJ2,
    Step::Refuse0,
    Step::Refuse1,
    Step::Progress,
    Step::ProgressNaN,
    Step::FinishCommitted,
    Step::FinishNotRun,
    Step::FinishCancelled,
    Step::FinishOtherJob,
    Step::Disconnect,
    Step::Reconnect,
    Step::Timeout,
    Step::ClockBack,
    Step::ResendOk,
    Step::ResendNotSent,
    Step::ResendUnknown,
    Step::Cancel,
    Step::Reattach,
    Step::UnknownKey,
];

const FIRST: [Send; 3] = [Send::Ok, Send::NotSent, Send::UnknownLost];

/// Ground truth kept by the test, independent of the tracker.
#[derive(Default)]
struct Truth {
    /// Attempts whose send was not `NotSent` (they may have reached the bus).
    left: BTreeSet<u32>,
    refused: BTreeSet<u32>,
    accepted_jobs: BTreeSet<String>,
    /// Some `Accepted` or `Finished` message named the key: the bus took it.
    taken: bool,
    attempts: u32,
}

fn run(first: Send, steps: &[Step]) {
    let j1 = job_id(1);
    let j2 = job_id(2);
    let mut bus = FakeBus::new();
    bus.passive = true;
    bus.script.push_back(first);
    let clock = ManualClock::new();
    let mut t = Tracker::new(bus, clock.clone(), CountingKeySource::new(0x0190_0000_0042));
    let id = t.submit(CommandId::parse(CMD).unwrap(), CValue::Null, TIMEOUT).unwrap();
    let key = t.key(id).unwrap();
    let mut truth = Truth { attempts: 1, ..Truth::default() };
    if first != Send::NotSent {
        truth.left.insert(0);
    }
    let committed = Envelope { journal_entry_hash: Some(HASH.to_string()), ..Envelope::bare(CMD, Outcome::Committed) };
    check(&t.report(id).cloned(), None, &truth, Step::ResendOk, steps);

    for (i, &step) in steps.iter().enumerate() {
        let before = t.report(id).cloned();
        let before_state = t.state(id).cloned();
        let mut reopening = false;
        let inbox = &mut t.transport_mut().inbox;
        match step {
            Step::AcceptJ1 => inbox.push_back(BusEvent::Accepted { key, job: j1 }),
            Step::AcceptJ2 => inbox.push_back(BusEvent::Accepted { key, job: j2 }),
            Step::Refuse0 => inbox.push_back(BusEvent::Refused { key, attempt: 0, reason: "busy".into() }),
            Step::Refuse1 => inbox.push_back(BusEvent::Refused { key, attempt: 1, reason: "busy".into() }),
            Step::Progress => inbox.push_back(BusEvent::Progress { job: j1, fraction: 0.5, stage: "s".into() }),
            Step::ProgressNaN => inbox.push_back(BusEvent::Progress { job: j1, fraction: f64::NAN, stage: "s".into() }),
            Step::FinishCommitted => {
                inbox.push_back(BusEvent::Finished { key, job: j1, envelope: Box::new(committed.clone()) })
            }
            Step::FinishNotRun => inbox.push_back(BusEvent::Finished {
                key,
                job: j1,
                envelope: Box::new(Envelope::bare(CMD, Outcome::NotRun)),
            }),
            Step::FinishCancelled => inbox.push_back(BusEvent::Finished {
                key,
                job: j1,
                envelope: Box::new(Envelope::bare(CMD, Outcome::Cancelled)),
            }),
            Step::FinishOtherJob => {
                inbox.push_back(BusEvent::Finished { key, job: j2, envelope: Box::new(committed.clone()) })
            }
            Step::Disconnect => inbox.push_back(BusEvent::Disconnected),
            Step::Reconnect => inbox.push_back(BusEvent::Reconnected),
            Step::UnknownKey => inbox.push_back(BusEvent::Accepted { key: j2, job: j1 }),
            Step::Timeout => clock.advance(Duration::from_secs(1)),
            Step::ClockBack => clock.set(Duration::ZERO),
            Step::ResendOk | Step::ResendNotSent | Step::ResendUnknown => {
                let how = match step {
                    Step::ResendOk => Send::Ok,
                    Step::ResendNotSent => Send::NotSent,
                    _ => Send::UnknownLost,
                };
                t.transport_mut().script.push_back(how);
                if t.resend(id, TIMEOUT).is_ok() {
                    reopening = true;
                    if how != Send::NotSent {
                        truth.left.insert(truth.attempts);
                    }
                    truth.attempts += 1;
                } else {
                    t.transport_mut().script.clear();
                }
            }
            Step::Cancel => {
                let _ = t.request_cancel(id);
            }
            Step::Reattach => {
                reopening = t.reattach(id, TIMEOUT).is_ok();
            }
        }
        // Record what this step tells the bus side, before the tracker sees it.
        let accepted_before = truth.taken;
        if matches!(
            step,
            Step::AcceptJ1
                | Step::AcceptJ2
                | Step::FinishCommitted
                | Step::FinishNotRun
                | Step::FinishCancelled
                | Step::FinishOtherJob
        ) {
            truth.taken = true;
        }
        match step {
            Step::AcceptJ1 => {
                truth.accepted_jobs.insert(j1.to_string());
            }
            Step::AcceptJ2 => {
                truth.accepted_jobs.insert(j2.to_string());
            }
            Step::Refuse0 if truth.left.contains(&0) => {
                truth.refused.insert(0);
            }
            Step::Refuse1 if truth.left.contains(&1) => {
                truth.refused.insert(1);
            }
            _ => {}
        }
        t.pump(&mut ());
        let after = t.report(id).cloned();
        let ctx = &steps[..=i];

        // Reports are never rewritten, except by an accepted resend or reattach.
        if before.is_some() && !reopening {
            assert_eq!(after, before, "report rewritten: {first:?} {ctx:?}");
        }
        // `not_run` is produced only with certainty: no acceptance before this step, and
        // every attempt that may have left was explicitly refused.
        let produced = after != before || reopening;
        if produced {
            if let Some(r) = &after {
                if r.envelope.outcome == Outcome::NotRun {
                    assert!(!accepted_before, "not_run after acceptance: {first:?} {ctx:?}");
                    assert!(
                        truth.left.is_subset(&truth.refused),
                        "not_run with an unrefused attempt: {first:?} {ctx:?}"
                    );
                }
                // A final bus result is taken only while accepted as the same job.
                if r.reason == fid_surface::jobs::Reason::Final {
                    let Some(State::Accepted { job }) = before_state else {
                        panic!("final result while not accepted: {first:?} {ctx:?}")
                    };
                    assert!(truth.accepted_jobs.contains(&job.to_string()), "{first:?} {ctx:?}");
                }
            }
        }
        // Once any acceptance was delivered, the tracker's evidence holds a job; once any
        // acceptance or final message was delivered, the evidence is sticky and no later
        // step (including a resend that never leaves) can produce `not_run`. A `not_run`
        // reported before that evidence arrived is not rewritten.
        if !truth.accepted_jobs.is_empty() {
            assert!(t.evidence(id).unwrap().job.is_some(), "{first:?} {ctx:?}");
        }
        if truth.taken {
            assert!(t.evidence(id).unwrap().taken, "{first:?} {ctx:?}");
            if produced {
                let out = after.as_ref().map(|r| r.envelope.outcome.clone());
                assert_ne!(out, Some(Outcome::NotRun), "not_run after the bus took the key: {first:?} {ctx:?}");
            }
        }
        check(&after, before.as_ref(), &truth, step, ctx);
        assert!(t.transport().sent_with(Purpose::Cancel).len() <= 1, "cancel sent twice: {first:?} {ctx:?}");
        assert_eq!(t.len(), 1);
    }
}

fn check(report: &Option<Report>, _before: Option<&Report>, truth: &Truth, _step: Step, ctx: &[Step]) {
    let Some(r) = report else { return };
    assert!(r.envelope.validate().is_ok(), "invalid envelope {r:?} after {ctx:?}");
    assert_eq!(r.envelope.command_id, CMD);
    if r.envelope.outcome == Outcome::NoReply {
        if let Some(j) = &r.envelope.job_id {
            assert!(truth.accepted_jobs.contains(j), "no_reply names a job never accepted: {ctx:?}");
        }
    }
}

#[test]
fn all_sequences_up_to_length_three() {
    let mut n = 0u32;
    for first in FIRST {
        run(first, &[]);
        for a in ALPHABET {
            run(first, &[a]);
            for b in ALPHABET {
                run(first, &[a, b]);
                for c in ALPHABET {
                    run(first, &[a, b, c]);
                    n += 1;
                }
            }
        }
    }
    assert_eq!(n, 3 * 20 * 20 * 20);
}

#[test]
fn seeded_long_sequences() {
    let mut s: u64 = 0x7a11_5eed_0000_0003;
    let mut next = || {
        s ^= s >> 12;
        s ^= s << 25;
        s ^= s >> 27;
        s.wrapping_mul(0x2545_f491_4f6c_dd1d)
    };
    for _ in 0..20_000 {
        let first = FIRST[(next() % 3) as usize];
        let steps: Vec<Step> = (0..16).map(|_| ALPHABET[(next() % 20) as usize]).collect();
        run(first, &steps);
    }
}
