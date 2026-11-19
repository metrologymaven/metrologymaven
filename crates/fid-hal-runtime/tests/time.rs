//! Acceptance criteria of spec node HAL.ACQ.TIME (AC-1 .. AC-6) plus hostile inputs.
//! Expected values come from the independent rational-arithmetic reference
//! `fixtures/hal/acq/time/make_fixtures.py`; error cases are hand-labelled from the leaf's rules.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_hal_idl::{TimeSource, Timestamp};
use fid_hal_runtime::time::{
    estimate, fuse, ClockMapping, FusionVerdict, MappedTime, SessionClock, SessionTimebase, StreamSync, TimeError,
    MAX_SPAN_NS, MIN_PAIRS,
};
use std::cell::Cell;
use std::collections::BTreeMap;

const FIX: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/hal/acq/time/");

fn read(name: &str) -> String {
    std::fs::read_to_string(format!("{FIX}{name}")).unwrap()
}

/// Reads the `[t_dev, t_host]` lines of a pairs fixture.
fn pairs(name: &str) -> Vec<(i64, i64)> {
    read(name)
        .lines()
        .map(str::trim)
        .filter(|l| l.starts_with('[') && !l.starts_with("[]"))
        .map(|l| {
            let l = l.trim_end_matches(',').trim_start_matches('[').trim_end_matches(']');
            let (a, b) = l.split_once(',').unwrap();
            (a.trim().parse().unwrap(), b.trim().parse().unwrap())
        })
        .collect()
}

/// `name -> rest of line` from expected-v1.txt (the `map` lines are returned separately).
fn expected() -> (BTreeMap<String, String>, Vec<(i64, i64)>) {
    let mut kv = BTreeMap::new();
    let mut maps = Vec::new();
    for l in read("expected-v1.txt").lines() {
        if l.starts_with('#') || l.trim().is_empty() {
            continue;
        }
        let (k, v) = l.split_once(' ').unwrap();
        if k == "map" {
            let (a, b) = v.split_once(' ').unwrap();
            maps.push((a.parse().unwrap(), b.parse().unwrap()));
        } else {
            kv.insert(k.to_string(), v.to_string());
        }
    }
    (kv, maps)
}

fn golden_bits(kv: &BTreeMap<String, String>, k: &str) -> u64 {
    u64::from_str_radix(kv[k].split(' ').next().unwrap(), 16).unwrap()
}

fn exact(kv: &BTreeMap<String, String>, k: &str) -> f64 {
    kv[k].parse().unwrap()
}

/// AC-1: offset 3.000 ± 0.001 ms, drift 50 ± 0.5 ppm, sigma 200 ± 30 ns; and the f64 result
/// is within a few ulps of the exact OLS values.
#[test]
fn ac1_drift_50ppm_within_tolerance() {
    let p = pairs("drift_50ppm.json");
    assert_eq!(p.len(), 1000);
    let m = estimate(&p, TimeSource::DeviceClock).unwrap();
    assert_eq!(m.n, 1000);
    assert_eq!(m.source, TimeSource::DeviceClock);
    assert!((m.offset_ns - 3_000_000.0).abs() <= 1_000.0, "offset {}", m.offset_ns);
    assert!((m.drift - 50e-6).abs() <= 0.5e-6, "drift {}", m.drift);
    assert!((m.sigma_ns - 200.0).abs() <= 30.0, "sigma {}", m.sigma_ns);

    let (kv, _) = expected();
    assert!((m.offset_ns - exact(&kv, "exact_offset_ns")).abs() <= 1e-6);
    assert!((m.drift - exact(&kv, "exact_drift")).abs() <= 1e-15);
    assert!((m.sigma_ns - exact(&kv, "exact_sigma_ns")).abs() <= 1e-9);
}

/// AC-2: the f64 outputs are bit-identical to the reference's per-operation rounding; this
/// test runs on the Linux, Windows and macOS CI runners.
#[test]
fn ac2_bit_identical_to_reference() {
    let p = pairs("drift_50ppm.json");
    let m = estimate(&p, TimeSource::DeviceClock).unwrap();
    let (kv, maps) = expected();
    assert_eq!(m.offset_ns.to_bits(), golden_bits(&kv, "offset_ns"), "offset {:?}", m.offset_ns);
    assert_eq!(m.drift.to_bits(), golden_bits(&kv, "drift"), "drift {:?}", m.drift);
    assert_eq!(m.sigma_ns.to_bits(), golden_bits(&kv, "sigma_ns"), "sigma {:?}", m.sigma_ns);
    assert_eq!(m.intercept_ns.to_bits(), golden_bits(&kv, "intercept_ns"), "intercept {:?}", m.intercept_ns);
    assert_eq!(m.ref_dev_ns.to_string(), kv["ref_dev_ns"]);
    assert_eq!(m.ref_host_ns.to_string(), kv["ref_host_ns"]);
    assert!(maps.len() >= 5);
    for (t_dev, t_host) in maps {
        assert_eq!(m.map_to_host(t_dev), Ok(t_host), "t_dev {t_dev}");
    }
    // Same input, same bits, every time.
    assert_eq!(estimate(&p, TimeSource::DeviceClock), Ok(m));
}

/// AC-3: ten pairs are `time_estimate_insufficient`, and the mapping falls back to
/// host-receive time recorded as HOST_RECEIVE (REQ-003).
#[test]
fn ac3_ten_pairs_insufficient_and_fallback() {
    let p = pairs("ten_pairs.json");
    assert_eq!(p.len(), 10);
    let r = estimate(&p, TimeSource::DeviceClock);
    assert_eq!(r, Err(TimeError::EstimateInsufficient { n: 10 }));
    assert_eq!(r.unwrap_err().code(), "time_estimate_insufficient");
    let mapping = ClockMapping::from_estimate(r).unwrap();
    assert_eq!(mapping, ClockMapping::HostReceive);
    assert_eq!(mapping.source(), TimeSource::HostReceive);
    let ts = Timestamp { t_dev: Some(p[3].0), t_host: p[3].1 };
    assert_eq!(mapping.map(ts), Ok(MappedTime { t_session_ns: p[3].1, source: TimeSource::HostReceive }));
    // Exactly 15 is still insufficient, 16 is enough.
    let big = pairs("drift_50ppm.json");
    assert_eq!(estimate(&big[..MIN_PAIRS - 1], TimeSource::Ptp), Err(TimeError::EstimateInsufficient { n: 15 }));
    assert!(estimate(&big[..MIN_PAIRS], TimeSource::Ptp).is_ok());
}

fn sync(source: TimeSource, sigma_ns: f64) -> StreamSync {
    StreamSync { source, sigma_ns }
}

/// AC-4: two PTP streams (800 ns, 900 ns) pass, whatever the threshold.
#[test]
fn ac4_ptp_pair_passes() {
    let a = sync(TimeSource::Ptp, 800.0);
    let b = sync(TimeSource::Ptp, 900.0);
    assert_eq!(fuse(a, b, 100_000.0), Ok(FusionVerdict::Ok));
    assert_eq!(fuse(a, b, 0.0), Ok(FusionVerdict::Ok));
    let h = sync(TimeSource::HwTrigger, 5e6);
    assert_eq!(fuse(h, h, 0.0), Ok(FusionVerdict::Ok));
}

/// AC-5: DEVICE_CLOCK 40 µs and HOST_RECEIVE 2 ms against a 100 µs threshold is
/// `time_sync_insufficient`.
#[test]
fn ac5_mixed_sources_insufficient() {
    let a = sync(TimeSource::DeviceClock, 40_000.0);
    let b = sync(TimeSource::HostReceive, 2_000_000.0);
    let v = fuse(a, b, 100_000.0).unwrap();
    // sqrt(40000^2 + 2000000^2) = sqrt(4001600000000), hand-computed: 2000399.96...
    match v {
        FusionVerdict::TimeSyncInsufficient { rss_ns } => {
            assert_eq!(rss_ns, 4_001_600_000_000.0_f64.sqrt());
            assert!(rss_ns > 2_000_399.0 && rss_ns < 2_000_400.0);
        }
        FusionVerdict::Ok => panic!("expected time_sync_insufficient"),
    }
    assert_eq!(v.flag(), Some("time_sync_insufficient"));
}

/// The fusion rule's other branches, hand-labelled from REQ-004.
#[test]
fn fusion_rule_edges() {
    // Mixed PTP and HW_TRIGGER do not "share" a source: the RSS rule applies.
    let a = sync(TimeSource::Ptp, 30.0);
    let b = sync(TimeSource::HwTrigger, 40.0);
    assert_eq!(fuse(a, b, 50.0), Ok(FusionVerdict::Ok)); // rss exactly 50: at or below passes
    assert_eq!(fuse(a, b, 49.999), Ok(FusionVerdict::TimeSyncInsufficient { rss_ns: 50.0 }));
    // Two DEVICE_CLOCK streams share a source but not a privileged one.
    let d = sync(TimeSource::DeviceClock, 3.0);
    let e = sync(TimeSource::DeviceClock, 4.0);
    assert_eq!(fuse(d, e, 5.0), Ok(FusionVerdict::Ok));
    assert_eq!(fuse(d, e, 4.0), Ok(FusionVerdict::TimeSyncInsufficient { rss_ns: 5.0 }));
    // Overflowing RSS is infinite and insufficient, not a panic.
    let big = sync(TimeSource::Ntp, f64::MAX);
    assert_eq!(fuse(big, big, f64::MAX), Ok(FusionVerdict::TimeSyncInsufficient { rss_ns: f64::INFINITY }));
    // Hostile sigmas and thresholds are typed errors.
    for bad in [f64::NAN, f64::INFINITY, -1.0] {
        assert_eq!(fuse(sync(TimeSource::Ptp, bad), a, 1.0).unwrap_err().code(), "time_invalid_input");
        assert_eq!(fuse(a, sync(TimeSource::Ptp, bad), 1.0).unwrap_err().code(), "time_invalid_input");
        assert_eq!(fuse(a, a, bad).unwrap_err().code(), "time_invalid_input");
    }
}

/// A fake host: monotonic and wall clocks set by the test.
struct FakeClock {
    mono: Cell<i64>,
    utc: Cell<i64>,
}

impl SessionClock for FakeClock {
    fn mono_ns(&self) -> i64 {
        self.mono.get()
    }
    fn utc_ns(&self) -> i64 {
        self.utc.get()
    }
}

/// AC-6: a +1 s host wall-clock step after opening does not change session time.
#[test]
fn ac6_wall_clock_step_does_not_move_session_time() {
    let u: i64 = 1_763_424_000_000_000_000;
    let clock = FakeClock { mono: Cell::new(5_000_000_000), utc: Cell::new(u) };
    let tb = SessionTimebase::open(&clock);
    assert_eq!(tb.anchor_utc_ns(), u);
    assert_eq!(tb.anchor_mono_ns(), 5_000_000_000);

    clock.mono.set(5_250_000_000);
    let before = (tb.elapsed_ns(clock.mono_ns()).unwrap(), tb.now_utc_ns(&clock).unwrap());
    clock.utc.set(clock.utc.get() + 1_000_000_000); // host wall clock steps +1 s
    let after = (tb.elapsed_ns(clock.mono_ns()).unwrap(), tb.now_utc_ns(&clock).unwrap());
    assert_eq!(before, after);
    assert_eq!(after, (250_000_000, u + 250_000_000));
    // The anchor is unchanged, and a second open is a new timebase, not a re-anchor.
    assert_eq!(tb.anchor_utc_ns(), u);
    assert_ne!(SessionTimebase::open(&clock), tb);
}

#[test]
fn timebase_overflow_is_typed() {
    let tb = SessionTimebase::anchor(i64::MIN, i64::MAX);
    assert_eq!(tb.elapsed_ns(i64::MAX).unwrap_err().code(), "time_overflow");
    let tb = SessionTimebase::anchor(0, i64::MAX);
    assert_eq!(tb.to_utc_ns(1).unwrap_err().code(), "time_overflow");
    assert_eq!(tb.to_utc_ns(-1), Ok(i64::MAX - 1));
}

/// 20 clean pairs with a 1 ms step and a 7 ns per step drift, for the hostile tests below.
fn clean(n: usize) -> Vec<(i64, i64)> {
    (0..n as i64).map(|i| (i * 1_000_000, 500 + i * 1_000_007)).collect()
}

#[test]
fn hostile_inputs_are_typed_errors() {
    let src = TimeSource::DeviceClock;
    assert!(estimate(&clean(20), src).is_ok());
    assert_eq!(estimate(&[], src), Err(TimeError::EstimateInsufficient { n: 0 }));

    let mut p = clean(20);
    p[7].0 = p[6].0;
    assert_eq!(estimate(&p, src), Err(TimeError::Duplicate { index: 7 }));

    let mut p = clean(20);
    p[9].0 = p[8].0 - 1;
    assert_eq!(estimate(&p, src), Err(TimeError::NonMonotonic { index: 9 }));

    // Host-time inversion (receive jitter, buffered delivery) is not an error: it still estimates.
    let mut p = clean(20);
    p[4].1 = p[3].1 - 1;
    p[11].1 = p[10].1 - 250;
    let m = estimate(&p, src).unwrap();
    assert_eq!(m.n, 20);
    assert!(m.sigma_ns > 0.0 && m.sigma_ns.is_finite());
    assert!((m.drift - 7e-6).abs() < 1e-3);

    // A device gap beyond 2^53 ns.
    let mut p = clean(20);
    for q in p.iter_mut().skip(12) {
        q.0 += MAX_SPAN_NS;
        q.1 += MAX_SPAN_NS;
    }
    assert_eq!(estimate(&p, src), Err(TimeError::SpanTooLarge { index: 12 }));

    // Host jumps by more than 2^53 relative to the device clock.
    let mut p = clean(20);
    for q in p.iter_mut().skip(15) {
        q.1 += MAX_SPAN_NS;
    }
    assert_eq!(estimate(&p, src), Err(TimeError::SpanTooLarge { index: 15 }));

    // Extreme i64 values: centring in i128 cannot overflow; the span check rejects them.
    let mut p: Vec<(i64, i64)> = (0..20).map(|i| (i64::MIN + i, i64::MIN + i)).collect();
    assert!(estimate(&p, src).is_ok());
    p[19] = (i64::MAX, i64::MAX);
    assert_eq!(estimate(&p, src), Err(TimeError::SpanTooLarge { index: 19 }));
    let p: Vec<(i64, i64)> = (0..20).map(|i| (i64::MIN + i, i64::MAX - 20 + i)).collect();
    let m = estimate(&p, src).unwrap();
    assert_eq!(m.offset_ns, (i128::from(i64::MAX - 20) - i128::from(i64::MIN)) as f64);
    assert_eq!(m.map_to_host(i64::MIN + 3), Ok(i64::MAX - 17));
    assert_eq!(m.map_to_host(i64::MIN + 40).unwrap_err().code(), "time_overflow");
    assert_eq!(m.map_to_host(0), Err(TimeError::MapSpanTooLarge));
    assert_eq!(m.map_to_host(0).unwrap_err().code(), "time_map_span_too_large");
}

/// A noise-free line is recovered exactly, sigma is zero, and mapping is exact.
#[test]
fn exact_line() {
    let m = estimate(&clean(20), TimeSource::Ptp).unwrap();
    assert_eq!(m.drift, 7.0 / 1_000_000.0);
    assert_eq!(m.offset_ns, 500.0);
    assert_eq!(m.sigma_ns, 0.0);
    assert_eq!(m.map_to_host(100 * 1_000_000), Ok(500 + 100 * 1_000_007));
    let mapping = ClockMapping::from_estimate(Ok(m)).unwrap();
    assert_eq!(mapping.source(), TimeSource::Ptp);
    assert_eq!(
        mapping.map(Timestamp { t_dev: Some(2_000_000), t_host: 9 }),
        Ok(MappedTime { t_session_ns: 2_000_514, source: TimeSource::Ptp })
    );
    // No device time: host-receive fallback.
    assert_eq!(
        mapping.map(Timestamp { t_dev: None, t_host: 9 }),
        Ok(MappedTime { t_session_ns: 9, source: TimeSource::HostReceive })
    );
    assert_eq!(mapping.sync(1.0), m.sync());
    assert_eq!(ClockMapping::HostReceive.sync(2e6), StreamSync { source: TimeSource::HostReceive, sigma_ns: 2e6 });
    // Other errors are not swallowed by the fallback.
    assert_eq!(
        ClockMapping::from_estimate(Err(TimeError::Duplicate { index: 1 })),
        Err(TimeError::Duplicate { index: 1 })
    );
}
