//! Acceptance criteria of spec node CORE.JOB.PROG (AC-1 .. AC-5).
//!
//! The test handler `slow_sum` named by the spec (`fixtures/core/job/prog/slow_sum`) lives in
//! this file, because a handler is Rust code; its expected output and digest are in
//! `fixtures/core/job/prog/expected.json`, written by the independent reference script
//! `fixtures/core/job/prog/make_expected.py`.
//!
//! Timing: rate limits, heartbeats and cancellation latency are tested deterministically with a
//! `ManualClock` and hand-driven `Progress::tick` calls. Exactly one test (`real_time_smoke`)
//! runs the background publisher on real time, with generous bounds for slow CI machines.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::{digest, parse, CValue};
use fid_jobs::{
    CancelToken, Clock, ManualClock, Progress, ProgressSink, ProgressUpdate, WarningKind, DEFAULT_POLL_INTERVAL,
    MAX_RECORDED_WARNINGS, MAX_STAGE_CHARS,
};
use std::collections::BTreeMap;
use std::sync::mpsc::Receiver;
use std::sync::Arc;
use std::time::{Duration, Instant};

const EXPECTED: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/core/job/prog/expected.json");

const N: u64 = 10_000_000;
const REPORT_EVERY: u64 = 100_000;
const CHECKPOINT_EVERY: u64 = 10_000;
const SECOND: Duration = Duration::from_secs(1);

/// `slow_sum` was cancelled; the value is the checkpoint (1-based) at which it noticed.
#[derive(Debug, PartialEq, Eq)]
struct Cancelled(u64);

/// Test handler `slow_sum` (CORE.JOB.PROG AC-1): sums 0 .. n-1, reports progress every
/// 10^5 integers and checks for cancellation every 10^4. `between_checkpoints(k)` runs right
/// after checkpoint `k` has been checked; tests use it to move a manual clock, drive the
/// publisher or request cancellation "while the handler works".
fn slow_sum(
    n: u64,
    sink: &ProgressSink,
    cancel: &CancelToken,
    mut between_checkpoints: impl FnMut(u64),
) -> Result<u64, Cancelled> {
    let mut sum: u64 = 0;
    for i in 0..n {
        sum = sum.wrapping_add(i);
        let done = i + 1;
        if done % CHECKPOINT_EVERY == 0 {
            let k = done / CHECKPOINT_EVERY;
            if cancel.is_cancelled() {
                return Err(Cancelled(k));
            }
            between_checkpoints(k);
        }
        if done % REPORT_EVERY == 0 {
            sink.report(done as f64 / n as f64, "summing");
        }
    }
    Ok(sum)
}

/// Output digest of a `slow_sum` result: canonical JSON `{"n": n, "sum": sum}`, 64-bit integers.
fn output_digest(n: u64, sum: u64) -> String {
    let mut o = BTreeMap::new();
    o.insert("n".to_string(), CValue::U64(n));
    o.insert("sum".to_string(), CValue::U64(sum));
    digest(&CValue::Obj(o)).unwrap().to_string()
}

fn expected_digest() -> String {
    let bytes = std::fs::read(EXPECTED).unwrap();
    match parse(&bytes).unwrap() {
        CValue::Obj(o) => match o.get("digest") {
            Some(CValue::Str(s)) => s.clone(),
            other => panic!("digest: expected string, got {other:?}"),
        },
        other => panic!("expected object, got {other:?}"),
    }
}

fn drain(rx: &Receiver<ProgressUpdate>) -> Vec<ProgressUpdate> {
    rx.try_iter().collect()
}

/// Every update has a finite fraction in [0, 1].
fn assert_valid_fractions(updates: &[ProgressUpdate]) {
    for u in updates {
        assert!(u.fraction.is_finite() && (0.0..=1.0).contains(&u.fraction), "bad fraction {u:?}");
    }
}

/// At most 20 updates in any 1 s window [t, t + 1 s).
fn assert_at_most_20_per_second(updates: &[ProgressUpdate]) {
    for (i, first) in updates.iter().enumerate() {
        let in_window = updates[i..].iter().take_while(|u| u.at < first.at + SECOND).count();
        assert!(in_window <= 20, "{in_window} updates in the 1 s window from {:?}", first.at);
    }
}

/// At least one update per second between `start` and `end`.
fn assert_at_least_1_per_second(updates: &[ProgressUpdate], start: Duration, end: Duration) {
    let mut prev = start;
    for u in updates {
        assert!(u.at - prev <= SECOND, "gap {:?} before update at {:?}", u.at - prev, u.at);
        prev = u.at;
    }
    assert!(end - prev <= SECOND, "gap {:?} at the end", end - prev);
}

#[test]
fn ac1_slow_sum_one_listener_rate_bounds_and_valid_fractions() {
    // Simulated time: 2 ms per checkpoint, so 1000 checkpoints take 2 s and the handler
    // reports 50 times per second, more than the publisher may pass on.
    let clock = Arc::new(ManualClock::new());
    let progress = Progress::new(clock.clone());
    let rx = progress.subscribe();
    progress.tick();
    let sum = slow_sum(N, &progress.sink(), &progress.cancel_token(), |_| {
        clock.advance(Duration::from_millis(2));
        progress.tick();
    })
    .unwrap();
    assert_eq!(sum, N * (N - 1) / 2);
    let end = clock.now();
    let updates = drain(&rx);
    assert!(updates.len() >= 2, "{} updates", updates.len());
    assert_valid_fractions(&updates);
    assert_at_most_20_per_second(&updates);
    assert_at_least_1_per_second(&updates, Duration::ZERO, end);
    // Fractions only grow, and the handler's reports do get through.
    assert!(updates.windows(2).all(|w| w[0].fraction <= w[1].fraction));
    assert!(updates.last().unwrap().fraction >= 0.9);
}

#[test]
fn ac1_throttle_holds_when_reporting_every_millisecond() {
    let clock = Arc::new(ManualClock::new());
    let progress = Progress::new(clock.clone());
    let rx = progress.subscribe();
    let sink = progress.sink();
    for ms in 0..=3000u32 {
        sink.report(f64::from(ms) / 3000.0, "busy");
        progress.tick();
        clock.advance(Duration::from_millis(1));
    }
    let updates = drain(&rx);
    assert_at_most_20_per_second(&updates);
    assert_at_least_1_per_second(&updates, Duration::ZERO, Duration::from_millis(3000));
    // 50 ms spacing over 3 s: 61 publications, none of them heartbeats.
    assert_eq!(updates.len(), 61);
    assert!(updates.iter().all(|u| !u.heartbeat));
}

#[test]
fn ac2_digest_identical_with_three_listeners_and_none() {
    // With three listeners and the publisher running at every checkpoint.
    let clock = Arc::new(ManualClock::new());
    let watched = Progress::new(clock.clone());
    let listeners: Vec<_> = (0..3).map(|_| watched.subscribe()).collect();
    let with = slow_sum(N, &watched.sink(), &watched.cancel_token(), |_| {
        clock.advance(Duration::from_millis(3));
        watched.tick();
    })
    .unwrap();
    for rx in &listeners {
        assert!(!drain(rx).is_empty(), "each listener saw progress");
    }

    // With no listeners and no publisher at all.
    let unwatched = Progress::new(Arc::new(ManualClock::new()));
    let without = slow_sum(N, &unwatched.sink(), &unwatched.cancel_token(), |_| {}).unwrap();

    let d_with = output_digest(N, with);
    let d_without = output_digest(N, without);
    assert_eq!(d_with, d_without);
    assert_eq!(d_with, expected_digest());
}

#[test]
fn ac3_silent_handler_gets_heartbeats_with_last_value() {
    let clock = Arc::new(ManualClock::new());
    let progress = Progress::new(clock.clone());
    let rx = progress.subscribe();
    progress.sink().report(0.25, "loading");
    // The handler then reports nothing for 3 s; the publisher polls every 10 ms.
    for _ in 0..=300 {
        progress.tick();
        clock.advance(Duration::from_millis(10));
    }
    let updates = drain(&rx);
    let first = &updates[0];
    assert!(!first.heartbeat);
    assert_eq!((first.fraction, first.stage.as_str(), first.report_seq), (0.25, "loading", 1));
    let beats: Vec<_> = updates[1..].iter().filter(|u| u.at <= Duration::from_secs(3)).collect();
    assert!(beats.len() >= 3, "{} heartbeats", beats.len());
    for b in &beats {
        assert!(b.heartbeat);
        assert_eq!((b.fraction, b.stage.as_str(), b.report_seq), (0.25, "loading", 1));
    }
    assert_at_least_1_per_second(&updates, Duration::ZERO, Duration::from_secs(3));
}

#[test]
fn ac4_cancel_seen_at_next_checkpoint_and_returns_promptly() {
    let progress = Progress::new(Arc::new(ManualClock::new()));
    let mut requested_after = None;
    let mut observed_at = None;
    let result = slow_sum(N, &progress.sink(), &progress.cancel_token(), |k| {
        if k == 500 {
            // Cancellation arrives while the handler works between checkpoints 500 and 501.
            progress.cancel();
            requested_after = Some(k);
        }
        observed_at = Some(Instant::now());
    });
    let returned = Instant::now();
    assert_eq!(requested_after, Some(500));
    assert_eq!(result, Err(Cancelled(501)), "observed at the next checkpoint");
    // The last hook ran just before checkpoint 501 was checked; generous bound for slow CI.
    assert!(returned - observed_at.unwrap() < SECOND);
}

#[test]
fn ac4_cancel_from_another_thread() {
    let progress = Arc::new(Progress::new(Arc::new(ManualClock::new())));
    let (sink, token) = (progress.sink(), progress.cancel_token());
    let (started_tx, started_rx) = std::sync::mpsc::channel();
    let (done_tx, done_rx) = std::sync::mpsc::channel();
    let worker = std::thread::spawn(move || {
        // Far more work than the test will wait for: only cancellation ends it in time.
        let r = slow_sum(u64::MAX, &sink, &token, |k| {
            if k == 1 {
                let _ = started_tx.send(());
            }
        });
        let _ = done_tx.send(r);
    });
    started_rx.recv_timeout(Duration::from_secs(60)).unwrap();
    progress.cancel();
    let r = done_rx.recv_timeout(Duration::from_secs(60)).expect("handler stopped after cancel");
    assert!(matches!(r, Err(Cancelled(k)) if k >= 2));
    worker.join().unwrap();
}

#[test]
fn ac5_invalid_fractions_are_dropped_with_a_warning() {
    let clock = Arc::new(ManualClock::new());
    let progress = Progress::new(clock.clone());
    let rx = progress.subscribe();
    let sink = progress.sink();
    sink.report(0.4, "ok");
    progress.tick();
    let bad = [f64::NAN, 1.5, -0.1, f64::INFINITY, f64::NEG_INFINITY];
    for (i, f) in bad.iter().enumerate() {
        sink.report(*f, &format!("bad {i}"));
        clock.advance(Duration::from_millis(100));
        progress.tick();
    }
    clock.advance(SECOND);
    progress.tick();
    let updates = drain(&rx);
    assert!(!updates.is_empty());
    for u in &updates {
        assert_eq!((u.fraction, u.stage.as_str(), u.report_seq), (0.4, "ok", 1), "listener saw {u:?}");
    }
    assert_eq!(progress.warning_count(), 5);
    let kinds: Vec<_> = progress.warnings().iter().map(|w| w.kind).collect();
    assert_eq!(
        kinds,
        [
            WarningKind::NotANumber,
            WarningKind::AboveOne,
            WarningKind::Negative,
            WarningKind::AboveOne,
            WarningKind::Negative
        ]
    );
    assert!(progress.warnings()[0].fraction.is_nan());
    assert_eq!(progress.warnings()[1].fraction, 1.5);
}

#[test]
fn warnings_are_bounded_but_counted() {
    let progress = Progress::new(Arc::new(ManualClock::new()));
    let sink = progress.sink();
    for _ in 0..1000 {
        sink.report(f64::NAN, "x");
    }
    assert_eq!(progress.warning_count(), 1000);
    assert_eq!(progress.warnings().len(), MAX_RECORDED_WARNINGS);
}

#[test]
fn bounds_zero_and_one_are_accepted_and_negative_zero_is_normalised() {
    let clock = Arc::new(ManualClock::new());
    let progress = Progress::new(clock.clone());
    let sink = progress.sink();
    sink.report(-0.0, "start");
    let u = progress.tick().unwrap();
    assert_eq!(u.fraction.to_bits(), 0.0f64.to_bits());
    sink.report(1.0, "end");
    clock.advance(Duration::from_millis(50));
    assert_eq!(progress.tick().unwrap().fraction, 1.0);
    assert_eq!(progress.warning_count(), 0);
}

#[test]
fn stage_labels_are_cut_to_128_characters() {
    let progress = Progress::new(Arc::new(ManualClock::new()));
    let long: String = "\u{00e9}".repeat(300);
    progress.sink().report(0.5, &long);
    let u = progress.tick().unwrap();
    assert_eq!(u.stage.chars().count(), MAX_STAGE_CHARS);
    assert!(u.stage.chars().all(|c| c == '\u{00e9}'));
    let progress = Progress::new(Arc::new(ManualClock::new()));
    progress.sink().report(0.5, "short");
    assert_eq!(progress.tick().unwrap().stage, "short");
}

#[test]
fn dropped_listeners_are_removed() {
    let clock = Arc::new(ManualClock::new());
    let progress = Progress::new(clock.clone());
    let keep = progress.subscribe();
    drop(progress.subscribe());
    assert_eq!(progress.listener_count(), 2);
    progress.tick();
    assert_eq!(progress.listener_count(), 1);
    assert_eq!(drain(&keep).len(), 1);
}

/// The only real-time test: background publisher thread, real clock, generous bounds.
#[test]
fn real_time_smoke() {
    let progress = Arc::new(Progress::with_system_clock());
    let rx = progress.subscribe();
    let publisher = progress.spawn_publisher(DEFAULT_POLL_INTERVAL).unwrap();
    let sink = progress.sink();
    let start = Instant::now();
    let run = Duration::from_millis(1500);
    while start.elapsed() < run {
        let f = start.elapsed().as_secs_f64() / run.as_secs_f64();
        sink.report(f.min(1.0), "smoke");
        std::thread::sleep(Duration::from_millis(5));
    }
    publisher.stop();
    let updates = drain(&rx);
    assert!(updates.len() >= 2, "{} updates", updates.len());
    assert_valid_fractions(&updates);
    // Holds by construction of the publication times, however slow the machine is.
    assert_at_most_20_per_second(&updates);
    assert!(updates.iter().any(|u| u.report_seq > 0), "handler reports reached the listener");
    // Generous: a very slow runner may starve the thread, but not for 5 s.
    assert!(updates.windows(2).all(|w| w[1].at - w[0].at < Duration::from_secs(5)));
}
