---
id: HAL.ACQ.INGEST.BENCH
title: LineScan throughput benchmark harness
level: 4
parent: HAL.ACQ.INGEST
kind: task
status: leaf
phase: P2
size: S
owners: []
claimed_by:
satisfies: [REQ-HAL.ACQ.INGEST-003]
covers: []
depends_on: [HAL.ACQ.INGEST.CMDS, HAL.SIM.SCANSIM]
side_effect: I1
determinism: N
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# HAL.ACQ.INGEST.BENCH: LineScan throughput benchmark harness

## Purpose

The repeatable benchmark that proves the ingest budget: one LineScan stream of 10 000 profiles per second at 2 000 points per profile for 10 minutes, with no gap, measured end to end from simulator to journal. It is a separate leaf because the number is a release gate and must be measured the same way every time.

## Scope

**In:**
- A benchmark binary that opens a session on the LineScan simulator, starts acquisition, runs for a configured duration and stops
- Metrics: sustained profile rate, gap count, p50 and p99 latency, peak memory, CPU use
- A machine-readable result file and a CI job on reference hardware (nightly, not per PR)

**Out:**
- The simulator (owned by `HAL.SIM.SCANSIM`)
- Reference hardware definition (owned by `PLAT`)
- Optimising the pipeline (owned by `HAL.ACQ.INGEST.SINK`)

## Requirements

- **REQ-HAL.ACQ.INGEST.BENCH-001** The harness shall run one LineScan stream at 10 000 profiles per second of 2 000 points for 600 s and report the gap count, which shall be zero on reference hardware [unverified until measured].
- **REQ-HAL.ACQ.INGEST.BENCH-002** The harness shall write a result file `ingest-bench/1` with rate, gap count, p50 and p99 latency, peak resident memory and CPU time, plus the runtime and simulator versions.
- **REQ-HAL.ACQ.INGEST.BENCH-003** If the gap count is non-zero or the sustained rate is below 10 000 profiles per second, then the nightly job shall fail.
- **REQ-HAL.ACQ.INGEST.BENCH-004** The harness shall accept a shortened duration (minimum 10 s) for developer machines and shall label such results as non-gating.

## Acceptance criteria

- **AC-1** Given the LineScan simulator at 10 000 profiles per second, when the harness runs for 10 s on a developer machine, then it writes a valid `ingest-bench/1` file labelled `non_gating`.
- **AC-2** Given the nightly job on reference hardware, when the harness runs for 600 s, then the gap count is 0 and the rate is at least 10 000 profiles per second [unverified until measured].
- **AC-3** (error path) Given an injected sink delay of 5 ms per batch, when the harness runs for 10 s, then the result shows a reduced rate and the job exits non-zero.

## Design

- **Registry commands:** none of its own; drives `device.open_session@1`, `device.start_acquisition@1` and `device.stop_acquisition@1` against a simulator (I1).
- **Data structures:** `BenchConfig { rate_hz, points_per_profile, duration_s, gating }`, `BenchResult` (`ingest-bench/1`).
- **Algorithm:** warm-up 5 s excluded; metrics from the stop summary and the sink telemetry.
- **Errors:** non-zero exit on gate failure.
- **Determinism:** N (performance measurement).
- **Performance budget:** this leaf is the budget.
- **Crate:** `fid-hal-runtime` (bench target `ingest_linescan`), CI job in the product repository.

## Test fixtures

None beyond the LineScan simulator configuration `fixtures/hal/acq/ingest/bench/ls_10k.json` (CC-BY-4.0).

## Build prompt

> In crate `fid-hal-runtime`, add bench target `ingest_linescan`: open a session on the `HAL.SIM.SCANSIM` line-profile simulator (I1), start acquisition at the configured rate (default 10 000 profiles/s x 2 000 points), run for `duration_s` (default 600, minimum 10; under 600 label `non_gating`), stop, and write `ingest-bench/1` JSON with rate, gap count, p50/p99 latency, peak RSS, CPU time and versions. Exit non-zero when gating and gaps > 0 or rate < 10 000/s. Add a nightly CI job on reference hardware. No panic, unwrap, expect or unsafe (CONSTITUTION §8). Tests: AC-1..AC-3.

## Open questions

- Reference hardware is defined by `PLAT`; until then the gate is advisory [unverified].

## Children

_Leaf: none._
