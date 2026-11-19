---
id: HAL.ACQ.TIME
title: Session timebase, clock offset and drift estimation, sync flag
level: 3
parent: HAL.ACQ
kind: task
status: leaf
phase: P2
size: M
owners: []
claimed_by:
built_in: [fid-hal-runtime]
built_pr: 164
satisfies: [REQ-HAL.ACQ-006]
covers: []
depends_on: [HAL.IDL.UNITS]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# HAL.ACQ.TIME: Session timebase, clock offset and drift estimation, sync flag

## Purpose

Gives every session one timebase and every device a mapping onto it. It anchors the host monotonic clock to UTC once per session, estimates each device clock's offset and drift with an uncertainty, and decides whether two streams may be fused or must be flagged `time_sync_insufficient`. It is a separate leaf because time mapping is a pure computation over recorded timestamp pairs and must be reproducible on replay.

## Scope

**In:**
- Session timebase: host monotonic i64 ns, anchored once to UTC at session open
- Offset and drift estimation per device from (t_dev, t_host) pairs, with residual standard deviation as sync uncertainty
- Mapping t_dev to session time
- The fusion rule and the `time_sync_insufficient` flag

**Out:**
- Collecting timestamps from drivers (owned by `HAL.ACQ.INGEST`)
- Adding timing contributors to uncertainty (owned by `UNC`)
- PTP or NTP configuration of the host (owned by `PLAT`)

## Requirements

- **REQ-HAL.ACQ.TIME-001** When a session opens, the timebase shall record one UTC anchor for the host monotonic clock and shall never re-anchor during the session, so clock steps on the host do not change session time.
- **REQ-HAL.ACQ.TIME-002** Given at least 16 (t_dev, t_host) pairs, the estimator shall return offset, drift (dimensionless rate) and the residual standard deviation as sync uncertainty in ns, using ordinary least squares in a fixed summation order.
- **REQ-HAL.ACQ.TIME-003** If fewer than 16 pairs are available, then the estimator shall return `time_estimate_insufficient` and the mapping shall fall back to host-receive time with the source recorded as `HOST_RECEIVE`.
- **REQ-HAL.ACQ.TIME-004** When two streams are fused, the rule shall pass if both share a `HW_TRIGGER` or `PTP` source, or if the root-sum-square of their sync uncertainties is at or below the capability's declared threshold; otherwise it shall return `time_sync_insufficient`.
- **REQ-HAL.ACQ.TIME-005** Given the same recorded pairs, the estimator and fusion rule shall return bit-identical results on every supported platform.

## Acceptance criteria

- **AC-1** Given `fixtures/hal/acq/time/drift_50ppm.json` (synthetic pairs, offset 3.000 ms, drift 50 ppm, Gaussian jitter σ = 200 ns, generator seed 7, 1 000 pairs), when the estimator runs, then offset is within 3.000 ± 0.001 ms, drift within 50 ± 0.5 ppm and sync σ within 200 ± 30 ns.
- **AC-2** Given the AC-1 fixture, when CI runs the estimator on Windows, Linux and macOS, then the f64 outputs are bit-identical.
- **AC-3** (error path) Given `fixtures/hal/acq/time/ten_pairs.json`, when the estimator runs, then it returns `time_estimate_insufficient`.
- **AC-4** Given streams A (PTP, σ 800 ns) and B (PTP, σ 900 ns), when the fusion rule runs, then it passes.
- **AC-5** Given streams A (DEVICE_CLOCK, σ 40 µs) and B (HOST_RECEIVE, σ 2 ms) and a threshold of 100 µs, when the fusion rule runs, then it returns `time_sync_insufficient`.
- **AC-6** Given a session opened at anchor U and a simulated host wall-clock step of +1 s, when session time is read, then it is unaffected by the step.

## Design

- **Registry commands:** none; library used by `HAL.ACQ.INGEST` and `HAL.ACQ.STAMP`.
- **Data structures:** `SessionTimebase { anchor_utc_ns: i64, anchor_mono_ns: i64 }`, `ClockModel { offset_ns: f64, drift: f64, sigma_ns: f64, source: TimeSource, n: u32 }`, `FusionVerdict { Ok, TimeSyncInsufficient { rss_ns } }`.
- **Algorithm:** centre the pairs on their first sample to keep magnitudes small; OLS slope and intercept with sums accumulated in index order, no FMA; residual σ with n − 2 degrees of freedom; RSS for fusion.
- **Errors:** `time_estimate_insufficient`.
- **Determinism:** D0 for recorded inputs; no FMA, fixed-order sums, sqrt only through the `fid-num` facade.
- **Performance budget:** 10 000 pairs in < 1 ms on reference hardware [unverified until measured].
- **Crate:** `fid-hal-runtime` (module `time`).

## Test fixtures

Synthetic generator `fixtures/hal/acq/time/gen.rs` with fixed seeds, producing the JSON fixtures above (CC-BY-4.0). Uses the counter-based RNG from `fid-num`.

## Build prompt

> In crate `fid-hal-runtime`, module `time`, implement: `SessionTimebase::anchor(mono_ns, utc_ns)` recorded once and never re-anchored; `estimate(pairs: &[(i64, i64)]) -> Result<ClockModel, TimeError>` using OLS on pairs centred at the first sample, sums in index order, no fused multiply-add, sqrt through `fid-num`, returning offset (ns), drift (rate), residual σ (ns, n−2 dof) and failing `time_estimate_insufficient` below 16 pairs (callers then fall back to `HOST_RECEIVE`); `fuse(a: &ClockModel, b: &ClockModel, threshold_ns: f64) -> FusionVerdict` passing when both share `HW_TRIGGER` or `PTP`, or RSS(σa, σb) ≤ threshold, else `TimeSyncInsufficient`. Types `TimeSource` and `Timestamp` come from `fid-hal-idl::units`. No panic, unwrap, expect or unsafe; BTreeMap only (CONSTITUTION §8). Tests: AC-1..AC-6 with fixtures under `fixtures/hal/acq/time/`, AC-2 on all three OS runners.

## Open questions

- Whether drift should be re-estimated over a sliding window during long sessions (thermal drift of device clocks) is left to the first live-tracker integration.

## Children

_Leaf: none._
