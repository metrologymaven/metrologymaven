//! Acceptance criteria and named transitions of spec node AUTO.SURF.JOBS.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use common::{job_id, FakeBus, Send, HASH};
use fid_canon::CValue;
use fid_surface::jobs::{
    classify, uuid_v7, BusEvent, CancelState, CancelStatus, CountingKeySource, Evidence, InvocationId, KeySource,
    ManualClock, ProgressUpdate, Purpose, Reason, State, SystemKeySource, Tracker, TrackerError, Verdict, Violation,
    CANCEL_JOB, MAX_ATTEMPTS, MAX_INVOCATIONS, MAX_STAGE_CHARS, WAIT_JOB,
};
use fid_surface::{Envelope, Outcome};
use fid_types::CommandId;
use fid_uuid::Uuid;
use std::time::Duration;

type T = Tracker<FakeBus, ManualClock, CountingKeySource>;

const CMD: &str = "align.run_321@1";
const MS200: Duration = Duration::from_millis(200);

fn setup(bus: FakeBus) -> (T, ManualClock) {
    let clock = ManualClock::new();
    (Tracker::new(bus, clock.clone(), CountingKeySource::new(0x0190_1234_5678)), clock)
}

fn cmd() -> CommandId {
    CommandId::parse(CMD).unwrap()
}

fn submit(t: &mut T) -> InvocationId {
    t.submit(cmd(), CValue::Null, MS200).unwrap()
}

fn give_up(t: &mut T, clock: &ManualClock) {
    t.pump(&mut ());
    clock.advance(MS200);
    t.pump(&mut ());
}

fn outcome(t: &T, id: InvocationId) -> Outcome {
    t.report(id).unwrap().envelope.outcome.clone()
}

fn job_str(t: &T, id: InvocationId) -> Option<String> {
    t.report(id).unwrap().envelope.job_id.clone()
}

fn committed(command: &str) -> Envelope {
    Envelope { journal_entry_hash: Some(HASH.to_string()), ..Envelope::bare(command, Outcome::Committed) }
}

// ------------------------------------------------------------------ acceptance criteria

/// AC-1 (as amended in this PR): `not_run` exactly when the bus certainly never accepted.
#[test]
fn ac1_refused_before_accept_is_not_run() {
    let mut bus = FakeBus::new();
    bus.refuse = Some("queue_full".into());
    let (mut t, clock) = setup(bus);
    let id = submit(&mut t);
    give_up(&mut t, &clock);
    assert_eq!(outcome(&t, id), Outcome::NotRun);
    assert_eq!(t.report(id).unwrap().reason, Reason::Refused("queue_full".into()));
    assert!(t.transport().jobs.is_empty());
}

#[test]
fn ac1_request_never_left_is_not_run() {
    let mut bus = FakeBus::new();
    bus.script.push_back(Send::NotSent);
    let (mut t, clock) = setup(bus);
    let id = submit(&mut t);
    assert_eq!(outcome(&t, id), Outcome::NotRun, "known at once");
    give_up(&mut t, &clock);
    assert_eq!(outcome(&t, id), Outcome::NotRun);
    assert_eq!(t.report(id).unwrap().reason, Reason::NotSent);
}

/// AC-1, the case the original wording got wrong: the bus took the request but its
/// acceptance message was dropped. The command may run, so the outcome is `no_reply`.
#[test]
fn ac1_dropped_acceptance_is_no_reply_not_not_run() {
    let mut bus = FakeBus::new();
    bus.drop_accept = true;
    let (mut t, clock) = setup(bus);
    let id = submit(&mut t);
    give_up(&mut t, &clock);
    assert_eq!(outcome(&t, id), Outcome::NoReply);
    assert_eq!(job_str(&t, id), None);
    assert_eq!(t.report(id).unwrap().reason, Reason::Deadline);
    assert_eq!(t.transport().jobs.len(), 1, "the bus did take it");
}

#[test]
fn ac1_request_lost_in_network_is_still_no_reply() {
    // The tracker cannot tell a lost request from a lost acceptance.
    for how in [Send::LostOk, Send::UnknownLost, Send::UnknownDelivered] {
        let mut bus = FakeBus::new();
        bus.drop_accept = true;
        bus.script.push_back(how);
        let (mut t, clock) = setup(bus);
        let id = submit(&mut t);
        give_up(&mut t, &clock);
        assert_eq!(outcome(&t, id), Outcome::NoReply, "{how:?}");
    }
}

#[test]
fn ac2_accepted_never_finished_is_no_reply_with_job() {
    let (mut t, clock) = setup(FakeBus::new());
    let id = submit(&mut t);
    give_up(&mut t, &clock);
    let j1 = job_id(1);
    assert_eq!(outcome(&t, id), Outcome::NoReply);
    assert_eq!(job_str(&t, id), Some(j1.to_string()));
    assert!(t.report(id).unwrap().envelope.validate().is_ok());
}

#[test]
fn ac3_resend_after_no_reply_returns_original_commit() {
    let (mut t, clock) = setup(FakeBus::new());
    let id = submit(&mut t);
    give_up(&mut t, &clock);
    assert_eq!(outcome(&t, id), Outcome::NoReply);
    // Meanwhile the job commits, and that reply is lost too.
    let j1 = job_id(1);
    t.transport_mut().commit(j1);
    t.transport_mut().inbox.clear();
    t.resend(id, MS200).unwrap();
    t.pump(&mut ());
    let r = t.report(id).unwrap();
    assert_eq!(r.envelope.outcome, Outcome::Committed);
    assert_eq!(r.envelope.journal_entry_hash.as_deref(), Some(HASH));
    assert_eq!(r.reason, Reason::Final);
    let bus = t.transport();
    assert_eq!(bus.commits, 1, "exactly one commit");
    let submits = bus.sent_with(Purpose::Submit);
    assert_eq!(submits.len(), 2);
    assert_eq!(submits[0].key, submits[1].key, "same idempotency key on resend");
    assert_eq!((submits[0].attempt, submits[1].attempt), (0, 1));
}

#[test]
fn ac4_progress_delivered_in_order() {
    let (mut t, _clock) = setup(FakeBus::new());
    let id = submit(&mut t);
    t.pump(&mut ());
    let j1 = job_id(1);
    for f in [0.1, 0.5, 1.0] {
        t.transport_mut().progress(j1, f, "fitting");
    }
    let mut got: Vec<(InvocationId, ProgressUpdate)> = Vec::new();
    t.pump(&mut got);
    let fr: Vec<f64> = got.iter().map(|(_, u)| u.fraction).collect();
    assert_eq!(fr, vec![0.1, 0.5, 1.0]);
    assert!(got.iter().all(|(i, u)| *i == id && u.stage == "fitting"));
}

#[test]
fn ac5_cancel_sent_once_and_outcome_cancelled() {
    let mut bus = FakeBus::new();
    bus.cancel_finishes = true;
    let (mut t, _clock) = setup(bus);
    let id = submit(&mut t);
    t.pump(&mut ());
    assert_eq!(t.request_cancel(id), Ok(CancelStatus::Sent));
    assert_eq!(t.request_cancel(id), Ok(CancelStatus::AlreadySent));
    t.pump(&mut ());
    assert_eq!(outcome(&t, id), Outcome::Cancelled);
    let cancels = t.transport().sent_with(Purpose::Cancel);
    assert_eq!(cancels.len(), 1);
    assert_eq!(cancels[0].command.as_str(), CANCEL_JOB);
    let CValue::Obj(p) = &cancels[0].params else { panic!() };
    assert_eq!(p.get("job"), Some(&CValue::Str(job_id(1).to_string())));
    assert_eq!(t.request_cancel(id), Err(TrackerError::AlreadyFinal));
}

/// Tiny seeded generator (xorshift64*), so AC-6 needs no dependency.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

/// AC-6: over 10,000 seeded interleavings of accept, progress, finish, drop and timeout
/// (plus resend, cancel, disconnect and clock jumps), `not_run` is never reported once an
/// acceptance was received, and only when the bus really never took the request.
#[test]
fn ac6_never_not_run_after_acceptance() {
    let mut rng = Rng(0x5eed_a0c6_0000_0001);
    let mut seen = [0u32; 3];
    for _ in 0..10_000 {
        let mut bus = FakeBus::new();
        bus.hold = true;
        bus.cancel_finishes = rng.below(2) == 0;
        if rng.below(8) == 0 {
            bus.refuse = Some("busy".into());
        }
        for _ in 0..4 {
            bus.script.push_back(match rng.below(8) {
                0 => Send::NotSent,
                1 => Send::UnknownDelivered,
                2 => Send::UnknownLost,
                3 => Send::LostOk,
                _ => Send::Ok,
            });
        }
        let (mut t, clock) = setup(bus);
        let id = submit(&mut t);
        let key = t.key(id).unwrap();
        for _ in 0..20 {
            match rng.below(10) {
                0 | 1 => t.transport_mut().deliver_one(),
                2 => t.transport_mut().drop_one(),
                3 => {
                    if let Some(j) = t.transport().job_of(key) {
                        t.transport_mut().progress(j, 0.5, "run");
                    }
                }
                4 => {
                    if let Some(j) = t.transport().job_of(key) {
                        if t.transport().jobs[&j].result.is_none() {
                            t.transport_mut().commit(j);
                        }
                    }
                }
                5 => clock.advance(Duration::from_millis(rng.below(300))),
                6 => {
                    let _ = t.resend(id, MS200);
                }
                7 => {
                    let _ = t.request_cancel(id);
                }
                8 => t.transport_mut().inbox.push_back(BusEvent::Disconnected),
                _ => clock.set(Duration::from_millis(rng.below(100))),
            }
            t.pump(&mut ());
            let ev = t.evidence(id).unwrap();
            if let Some(r) = t.report(id) {
                assert!(r.envelope.validate().is_ok());
                if r.envelope.outcome == Outcome::NotRun {
                    assert!(ev.job.is_none(), "not_run after an acceptance");
                    assert!(t.transport().job_of(key).is_none(), "not_run but the bus took it");
                }
                if r.envelope.outcome == Outcome::Committed {
                    assert_eq!(r.envelope.journal_entry_hash.as_deref(), Some(HASH));
                }
                seen[match r.envelope.outcome {
                    Outcome::NotRun => 0,
                    Outcome::NoReply => 1,
                    _ => 2,
                }] += 1;
            }
            assert!(t.transport().commits <= 1);
        }
    }
    assert!(seen.iter().all(|n| *n > 0), "every outcome class is exercised: {seen:?}");
}

// ------------------------------------------------------------------ keys and classification

#[test]
fn uuid_v7_layout_hand_labelled() {
    // Expected strings are written by hand from the RFC 9562 §5.7 bit layout.
    let cases: [(u64, u128, &str); 5] = [
        (0x0123_4567_89ab, 0, "01234567-89ab-7000-8000-000000000000"),
        (0x0123_4567_89ab, (1u128 << 74) - 1, "01234567-89ab-7fff-bfff-ffffffffffff"),
        (u64::MAX, 0, "ffffffff-ffff-7000-8000-000000000000"),
        (0, 1u128 << 62, "00000000-0000-7001-8000-000000000000"),
        (0, 1 | (1u128 << 80), "00000000-0000-7000-8000-000000000001"),
    ];
    for (ms, r, want) in cases {
        assert_eq!(uuid_v7(ms, r).to_string(), want);
    }
}

#[test]
fn system_keys_are_v7_and_distinct() {
    let mut k = SystemKeySource::new();
    let keys: Vec<Uuid> = (0..256).map(|_| k.next_key()).collect();
    assert!(keys.iter().all(|u| u.version() == 7 && u.variant_bits() == 0b10));
    let mut sorted = keys.clone();
    sorted.sort();
    sorted.dedup();
    assert_eq!(sorted.len(), keys.len());
}

#[test]
fn classify_is_exhaustively_honest() {
    let j = job_id(9);
    for left in 0u64..16 {
        for refused in 0u64..16 {
            if refused & !left != 0 {
                continue; // refusals are only recorded for attempts that left
            }
            for job in [None, Some(j)] {
                let e = Evidence { left, refused, job };
                let want = match job {
                    Some(j) => Verdict::NoReply(Some(j)),
                    None if left == refused => Verdict::NotRun,
                    None => Verdict::NoReply(None),
                };
                assert_eq!(classify(&e), want, "{e:?}");
                assert_eq!(e.never_accepted(), want == Verdict::NotRun);
            }
        }
    }
}

// ------------------------------------------------------------------ named transitions

#[test]
fn created_to_sent_to_accepted_to_finished() {
    let mut bus = FakeBus::new();
    bus.hold = true;
    let (mut t, _clock) = setup(bus);
    let id = submit(&mut t);
    assert_eq!(t.state(id), Some(&State::Sent));
    t.transport_mut().deliver_one();
    t.pump(&mut ());
    let j1 = job_id(1);
    assert_eq!(t.state(id), Some(&State::Accepted { job: j1 }));
    t.transport_mut().commit(j1);
    t.transport_mut().deliver_one();
    t.pump(&mut ());
    assert_eq!(outcome(&t, id), Outcome::Committed);
    let r = t.take(id).unwrap();
    assert_eq!(r.reason, Reason::Final);
    assert!(t.is_empty());
}

#[test]
fn every_submit_gets_a_fresh_key() {
    let (mut t, _clock) = setup(FakeBus::new());
    let a = submit(&mut t);
    let b = submit(&mut t);
    assert_ne!(t.key(a), t.key(b));
    assert_eq!(t.key(a).unwrap().version(), 7);
}

#[test]
fn timeout_racing_with_accept_honours_the_delivered_accept() {
    let mut bus = FakeBus::new();
    bus.hold = true;
    let (mut t, _clock) = setup(bus);
    let id = t.submit(cmd(), CValue::Null, Duration::ZERO).unwrap();
    // Deadline is now; the acceptance is already readable when the deadline is checked.
    t.transport_mut().deliver_one();
    t.pump(&mut ());
    assert_eq!(outcome(&t, id), Outcome::NoReply);
    assert_eq!(job_str(&t, id), Some(job_id(1).to_string()));
}

#[test]
fn accept_after_deadline_is_kept_as_evidence() {
    let mut bus = FakeBus::new();
    bus.hold = true;
    let (mut t, clock) = setup(bus);
    let id = submit(&mut t);
    clock.advance(MS200);
    t.pump(&mut ());
    assert_eq!(job_str(&t, id), None);
    t.transport_mut().deliver_one();
    t.pump(&mut ());
    assert_eq!(job_str(&t, id), None, "a report is never rewritten");
    assert_eq!(t.counters().late, 1);
    assert_eq!(t.evidence(id).unwrap().job, Some(job_id(1)));
    // A resend now waits on the known job; the bus replays its acceptance.
    t.resend(id, MS200).unwrap();
    assert_eq!(t.state(id), Some(&State::Accepted { job: job_id(1) }));
    t.pump(&mut ());
    assert_eq!(t.counters().duplicates, 0, "replayed acceptance is still held");
}

#[test]
fn duplicate_replies() {
    let (mut t, _clock) = setup(FakeBus::new());
    let id = submit(&mut t);
    let key = t.key(id).unwrap();
    let j1 = job_id(1);
    t.transport_mut().inbox.push_back(BusEvent::Accepted { key, job: j1 });
    t.pump(&mut ());
    assert_eq!(t.counters().duplicates, 1);
    assert_eq!(t.state(id), Some(&State::Accepted { job: j1 }));
    t.transport_mut().commit(j1);
    let fin = t.transport().inbox.back().cloned().unwrap();
    t.transport_mut().inbox.push_back(fin);
    t.pump(&mut ());
    assert_eq!(outcome(&t, id), Outcome::Committed);
    assert_eq!(t.counters().late, 1);
}

#[test]
fn lost_final_reply_is_no_reply_with_job() {
    let (mut t, clock) = setup(FakeBus::new());
    let id = submit(&mut t);
    t.pump(&mut ());
    t.transport_mut().commit(job_id(1));
    t.transport_mut().inbox.clear();
    give_up(&mut t, &clock);
    assert_eq!(outcome(&t, id), Outcome::NoReply);
    assert_eq!(job_str(&t, id), Some(job_id(1).to_string()));
}

#[test]
fn send_unknown_is_never_not_run() {
    let mut bus = FakeBus::new();
    bus.script.push_back(Send::UnknownLost);
    let (mut t, clock) = setup(bus);
    let id = submit(&mut t);
    assert_eq!(t.state(id), Some(&State::Sent));
    give_up(&mut t, &clock);
    assert_eq!(outcome(&t, id), Outcome::NoReply);
}

#[test]
fn refusal_of_one_of_two_attempts_is_not_enough() {
    let mut bus = FakeBus::new();
    bus.passive = true;
    let (mut t, clock) = setup(bus);
    let id = submit(&mut t);
    t.resend(id, MS200).unwrap();
    let key = t.key(id).unwrap();
    t.transport_mut().inbox.push_back(BusEvent::Refused { key, attempt: 1, reason: "busy".into() });
    give_up(&mut t, &clock);
    assert_eq!(outcome(&t, id), Outcome::NoReply, "attempt 0 is unaccounted for");
    // Both refused: certain.
    t.resend(id, MS200).unwrap();
    for attempt in [0, 2] {
        t.transport_mut().inbox.push_back(BusEvent::Refused { key, attempt, reason: "busy".into() });
    }
    t.pump(&mut ());
    assert_eq!(outcome(&t, id), Outcome::NotRun);
}

#[test]
fn refusal_of_an_attempt_that_never_left_is_ignored() {
    let mut bus = FakeBus::new();
    bus.passive = true;
    let (mut t, clock) = setup(bus);
    let id = submit(&mut t);
    let key = t.key(id).unwrap();
    for attempt in [5, 64, u32::MAX] {
        t.transport_mut().inbox.push_back(BusEvent::Refused { key, attempt, reason: "x".into() });
    }
    give_up(&mut t, &clock);
    assert_eq!(outcome(&t, id), Outcome::NoReply);
    assert_eq!(t.counters().bad_refusals, 3);
}

#[test]
fn refused_then_accepted_is_a_counted_contradiction() {
    let mut bus = FakeBus::new();
    bus.passive = true;
    let (mut t, _clock) = setup(bus);
    let id = submit(&mut t);
    let key = t.key(id).unwrap();
    t.transport_mut().inbox.push_back(BusEvent::Refused { key, attempt: 0, reason: "busy".into() });
    t.transport_mut().inbox.push_back(BusEvent::Accepted { key, job: job_id(1) });
    t.pump(&mut ());
    assert_eq!(outcome(&t, id), Outcome::NotRun);
    assert_eq!(t.counters().violations, 1);
    // From now on the evidence rules out not_run.
    t.resend(id, MS200).unwrap();
    assert_eq!(t.state(id), Some(&State::Accepted { job: job_id(1) }));
}

#[test]
fn accept_with_other_job_is_a_violation() {
    let (mut t, _clock) = setup(FakeBus::new());
    let id = submit(&mut t);
    let key = t.key(id).unwrap();
    t.transport_mut().inbox.push_back(BusEvent::Accepted { key, job: job_id(7) });
    t.pump(&mut ());
    let r = t.report(id).unwrap();
    assert_eq!(r.reason, Reason::ProtocolViolation(Violation::JobMismatch));
    assert_eq!(r.envelope.outcome, Outcome::NoReply);
    assert_eq!(r.envelope.job_id, Some(job_id(1).to_string()));
}

#[test]
fn job_id_owned_by_another_invocation_is_a_violation() {
    let mut bus = FakeBus::new();
    bus.passive = true;
    let (mut t, _clock) = setup(bus);
    let a = submit(&mut t);
    let b = submit(&mut t);
    let (ka, kb) = (t.key(a).unwrap(), t.key(b).unwrap());
    t.transport_mut().inbox.push_back(BusEvent::Accepted { key: ka, job: job_id(1) });
    t.transport_mut().inbox.push_back(BusEvent::Accepted { key: kb, job: job_id(1) });
    t.pump(&mut ());
    assert_eq!(t.state(a), Some(&State::Accepted { job: job_id(1) }));
    assert_eq!(t.report(b).unwrap().reason, Reason::ProtocolViolation(Violation::JobIdReused));
    assert_eq!(t.report(b).unwrap().envelope.outcome, Outcome::NoReply);
}

#[test]
fn finish_before_accept_is_no_reply() {
    let mut bus = FakeBus::new();
    bus.passive = true;
    let (mut t, _clock) = setup(bus);
    let id = submit(&mut t);
    let key = t.key(id).unwrap();
    t.transport_mut().inbox.push_back(BusEvent::Finished { key, job: job_id(1), envelope: committed(CMD) });
    t.pump(&mut ());
    let r = t.report(id).unwrap();
    assert_eq!(r.reason, Reason::ProtocolViolation(Violation::FinishBeforeAccept));
    assert_eq!(r.envelope.outcome, Outcome::NoReply);
    assert_eq!(r.reason.code(), "protocol_violation");
}

#[test]
fn bad_final_envelopes_are_no_reply() {
    let bad = [
        (Envelope::bare(CMD, Outcome::NotRun), "not_final_outcome"),
        (Envelope::bare(CMD, Outcome::NoReply), "not_final_outcome"),
        (Envelope { job_id: Some("j".into()), ..Envelope::bare(CMD, Outcome::Accepted) }, "not_final_outcome"),
        (Envelope::bare(CMD, Outcome::Committed), "invalid_final_envelope"),
        (Envelope::bare(CMD, Outcome::Unknown("teleported".into())), "invalid_final_envelope"),
        (committed("other.cmd@1"), "final_command_mismatch"),
        (Envelope { job_id: Some(job_id(2).to_string()), ..committed(CMD) }, "final_job_mismatch"),
    ];
    for (env, why) in bad {
        let mut bus = FakeBus::new();
        bus.passive = true;
        let (mut t, _clock) = setup(bus);
        let id = submit(&mut t);
        let key = t.key(id).unwrap();
        t.transport_mut().inbox.push_back(BusEvent::Accepted { key, job: job_id(1) });
        t.transport_mut().inbox.push_back(BusEvent::Finished { key, job: job_id(1), envelope: env });
        t.pump(&mut ());
        let r = t.report(id).unwrap();
        assert_eq!(r.reason, Reason::ProtocolViolation(Violation::BadFinalEnvelope(why)));
        assert_eq!(r.envelope.outcome, Outcome::NoReply);
        assert_eq!(r.envelope.job_id, Some(job_id(1).to_string()));
    }
}

#[test]
fn cancel_after_commit_sends_nothing() {
    let (mut t, _clock) = setup(FakeBus::new());
    let id = submit(&mut t);
    t.pump(&mut ());
    t.transport_mut().commit(job_id(1));
    t.pump(&mut ());
    assert_eq!(t.request_cancel(id), Err(TrackerError::AlreadyFinal));
    assert!(t.transport().sent_with(Purpose::Cancel).is_empty());
    assert_eq!(outcome(&t, id), Outcome::Committed);
}

#[test]
fn commit_winning_the_race_with_cancel_reports_committed() {
    let (mut t, _clock) = setup(FakeBus::new());
    let id = submit(&mut t);
    t.pump(&mut ());
    t.transport_mut().commit(job_id(1));
    assert_eq!(t.request_cancel(id), Ok(CancelStatus::Sent));
    t.pump(&mut ());
    assert_eq!(outcome(&t, id), Outcome::Committed);
    assert_eq!(t.transport().commits, 1);
}

#[test]
fn cancel_before_accept_is_sent_on_accept() {
    let mut bus = FakeBus::new();
    bus.hold = true;
    bus.cancel_finishes = true;
    let (mut t, _clock) = setup(bus);
    let id = submit(&mut t);
    assert_eq!(t.request_cancel(id), Ok(CancelStatus::Deferred));
    assert!(t.transport().sent_with(Purpose::Cancel).is_empty());
    t.transport_mut().deliver_one();
    t.pump(&mut ());
    assert_eq!(t.cancel_state(id), Some(CancelState::Sent));
    t.transport_mut().deliver_one();
    t.pump(&mut ());
    assert_eq!(outcome(&t, id), Outcome::Cancelled);
    assert_eq!(t.transport().sent_with(Purpose::Cancel).len(), 1);
}

#[test]
fn failed_cancel_send_can_be_retried_with_the_same_key() {
    let (mut t, _clock) = setup(FakeBus::new());
    let id = submit(&mut t);
    t.pump(&mut ());
    t.transport_mut().script.push_back(Send::NotSent);
    assert_eq!(t.request_cancel(id), Err(TrackerError::Transport(fid_surface::jobs::SendError::NotSent)));
    assert_eq!(t.cancel_state(id), Some(CancelState::Failed));
    t.transport_mut().script.push_back(Send::UnknownLost);
    assert!(t.request_cancel(id).is_err());
    assert_eq!(t.cancel_state(id), Some(CancelState::Unsure));
    assert_eq!(t.request_cancel(id), Ok(CancelStatus::Sent));
    let c = t.transport().sent_with(Purpose::Cancel);
    assert_eq!(c.len(), 2, "NotSent never left");
    assert_eq!(c[0].key, c[1].key);
}

#[test]
fn disconnect_and_reconnect() {
    let (mut t, _clock) = setup(FakeBus::new());
    let a = submit(&mut t);
    t.pump(&mut ());
    t.transport_mut().drop_accept = true;
    let b = submit(&mut t);
    t.transport_mut().inbox.push_back(BusEvent::Disconnected);
    t.pump(&mut ());
    assert_eq!(outcome(&t, a), Outcome::NoReply);
    assert_eq!(job_str(&t, a), Some(job_id(1).to_string()));
    assert_eq!(outcome(&t, b), Outcome::NoReply);
    assert_eq!(job_str(&t, b), None);
    assert_eq!(t.report(a).unwrap().reason, Reason::TransportClosed);
    // Meanwhile both commit on the bus; the replies die with the connection.
    t.transport_mut().commit(job_id(1));
    t.transport_mut().commit(job_id(2));
    t.transport_mut().inbox.clear();
    t.transport_mut().inbox.push_back(BusEvent::Reconnected);
    t.pump(&mut ());
    assert_eq!(t.counters().reconnects, 1);
    // `a` re-attaches to its job; `b` (job unknown) resends with its key.
    assert_eq!(t.reattach(b, MS200), Err(TrackerError::NoJob));
    t.reattach(a, MS200).unwrap();
    t.transport_mut().drop_accept = false;
    t.resend(b, MS200).unwrap();
    t.pump(&mut ());
    assert_eq!(outcome(&t, a), Outcome::Committed);
    assert_eq!(outcome(&t, b), Outcome::Committed);
    assert_eq!(t.transport().commits, 2);
    let w = t.transport().sent_with(Purpose::Wait);
    assert_eq!(w.len(), 1);
    assert_eq!(w[0].command.as_str(), WAIT_JOB);
    assert_eq!(t.reattach(a, MS200), Err(TrackerError::AlreadyFinal));
}

#[test]
fn clock_jumping_back_neither_fires_nor_extends_deadlines() {
    let mut bus = FakeBus::new();
    bus.passive = true;
    let (mut t, clock) = setup(bus);
    clock.set(Duration::from_secs(10));
    let id = submit(&mut t);
    clock.set(Duration::ZERO);
    t.pump(&mut ());
    assert_eq!(t.state(id), Some(&State::Sent));
    assert_eq!(t.deadline(id), Some(Duration::from_secs(10) + MS200));
    // The tracker's time stays at 10 s until the clock passes it again.
    clock.set(Duration::from_secs(10) + MS200);
    t.pump(&mut ());
    assert_eq!(outcome(&t, id), Outcome::NoReply);
    // A new invocation after a backward jump counts from the highest time seen.
    clock.set(Duration::from_secs(1));
    let id2 = submit(&mut t);
    assert_eq!(t.deadline(id2), Some(Duration::from_secs(10) + MS200 + MS200));
}

#[test]
fn clock_jumping_forward_classifies_by_state() {
    let mut bus = FakeBus::new();
    bus.hold = true;
    let (mut t, clock) = setup(bus);
    let a = submit(&mut t);
    t.transport_mut().deliver_one();
    t.pump(&mut ());
    let b = submit(&mut t);
    clock.advance(Duration::from_secs(u64::MAX));
    clock.advance(Duration::from_secs(u64::MAX));
    t.pump(&mut ());
    assert_eq!(job_str(&t, a), Some(job_id(1).to_string()));
    assert_eq!(outcome(&t, b), Outcome::NoReply);
    // Deadlines saturate rather than overflow.
    assert!(t.submit(cmd(), CValue::Null, Duration::MAX).is_ok());
}

#[test]
fn progress_filtering() {
    let mut bus = FakeBus::new();
    bus.hold = true;
    let (mut t, _clock) = setup(bus);
    let id = submit(&mut t);
    let j1 = job_id(1);
    // Before acceptance the job is unknown to the tracker.
    t.transport_mut().inbox.push_back(BusEvent::Progress { job: j1, fraction: 0.2, stage: "a".into() });
    t.transport_mut().deliver_one();
    let long = "é".repeat(MAX_STAGE_CHARS + 10);
    for (f, s) in [(f64::NAN, "nan"), (-0.1, "neg"), (1.5, "big"), (f64::INFINITY, "inf"), (0.0, long.as_str())] {
        t.transport_mut().inbox.push_back(BusEvent::Progress { job: j1, fraction: f, stage: s.into() });
    }
    let mut got: Vec<(InvocationId, ProgressUpdate)> = Vec::new();
    t.pump(&mut got);
    assert_eq!(t.counters().unmatched, 1);
    assert_eq!(t.counters().ignored_progress, 4);
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].0, id);
    assert_eq!(got[0].1.stage.chars().count(), MAX_STAGE_CHARS);
}

#[test]
fn bounded_invocations_and_attempts() {
    let mut bus = FakeBus::new();
    bus.passive = true;
    let (mut t, _clock) = setup(bus);
    let first = submit(&mut t);
    for _ in 1..MAX_INVOCATIONS {
        submit(&mut t);
    }
    assert_eq!(t.submit(cmd(), CValue::Null, MS200), Err(TrackerError::Full));
    for _ in 1..MAX_ATTEMPTS {
        t.resend(first, MS200).unwrap();
    }
    assert_eq!(t.resend(first, MS200), Err(TrackerError::TooManyAttempts));
    assert_eq!(t.take(first), None, "still waiting");
}

#[test]
fn duplicate_key_from_key_source_is_refused() {
    struct Same;
    impl KeySource for Same {
        fn next_key(&mut self) -> Uuid {
            uuid_v7(1, 1)
        }
    }
    let mut t = Tracker::new(FakeBus::new(), ManualClock::new(), Same);
    assert!(t.submit(cmd(), CValue::Null, MS200).is_ok());
    assert_eq!(t.submit(cmd(), CValue::Null, MS200), Err(TrackerError::DuplicateKey));
}

#[test]
fn resend_rules() {
    let (mut t, _clock) = setup(FakeBus::new());
    let id = submit(&mut t);
    t.pump(&mut ());
    assert_eq!(t.resend(id, MS200), Err(TrackerError::AlreadyAccepted));
    t.transport_mut().commit(job_id(1));
    t.pump(&mut ());
    assert_eq!(t.resend(id, MS200), Err(TrackerError::AlreadyFinal));
    t.take(id).unwrap();
    assert_eq!(t.resend(id, MS200), Err(TrackerError::UnknownInvocation));
    assert_eq!(t.request_cancel(id), Err(TrackerError::UnknownInvocation));
}

#[test]
fn events_after_take_are_unmatched() {
    let (mut t, _clock) = setup(FakeBus::new());
    let id = submit(&mut t);
    t.pump(&mut ());
    t.transport_mut().commit(job_id(1));
    t.pump(&mut ());
    let key = t.key(id).unwrap();
    t.take(id).unwrap();
    t.transport_mut().inbox.push_back(BusEvent::Accepted { key, job: job_id(1) });
    t.transport_mut().progress(job_id(1), 0.5, "x");
    t.pump(&mut ());
    assert_eq!(t.counters().unmatched, 2);
}
