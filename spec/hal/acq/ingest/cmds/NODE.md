---
id: HAL.ACQ.INGEST.CMDS
title: Start and stop acquisition commands and stop summary
level: 4
parent: HAL.ACQ.INGEST
kind: feature
status: leaf
phase: P2
size: M
owners: []
claimed_by:
satisfies: [REQ-HAL.ACQ.INGEST-004]
covers: []
depends_on: [HAL.ACQ.INGEST.SINK, HAL.ACQ.SESS.LIFE, HAL.ACQ.SESS.GATE, HAL.ACQ.POLICY]
side_effect: [D1, D1]
determinism: N
cli: [fid device start-acquisition, fid device stop-acquisition]
mcp: [device_start_acquisition, device_stop_acquisition]
journal: [device.start_acquisition@1, device.stop_acquisition@1]
updated: 2026-11-17
---

# HAL.ACQ.INGEST.CMDS: Start and stop acquisition commands and stop summary

## Purpose

The two registry commands that start and stop a data stream on an open session, with the calibration-policy check at start and a stop summary that makes every stream's completeness auditable from the journal alone.

## Scope

**In:**
- `device.start_acquisition@1` (D1): checks the session holds control, applies the calibration policy, creates the stream pipeline, tells the source to start
- `device.stop_acquisition@1` (D1): tells the source to stop, drains the pipeline, journals the stop summary
- The stop summary: batch count, sample count, gap count, gap list digest, last sequence number, end reason
- MCP projection as request-only (D1)

**Out:**
- The pipeline itself (owned by `HAL.ACQ.INGEST.SINK`)
- Session control and the writer lock (owned by `HAL.ACQ.SESS.LIFE`)
- The driver-call gate (owned by `HAL.ACQ.SESS.GATE`)
- The calibration decision (owned by `HAL.ACQ.POLICY`)

## Requirements

- **REQ-HAL.ACQ.INGEST.CMDS-001** When `device.start_acquisition@1` runs, the system shall require that the caller holds the session's controlling-writer lock, and shall fail with `session_not_controlled` otherwise.
- **REQ-HAL.ACQ.INGEST.CMDS-002** When the calibration check returns `Block`, `device.start_acquisition@1` shall fail with that reason code and start no stream; when it returns `Flag`, the stream shall start and every batch shall carry the flag.
- **REQ-HAL.ACQ.INGEST.CMDS-003** When `device.stop_acquisition@1` completes, the journal entry shall record the batch count, sample count, gap count, a digest of the gap list, the last sequence number and the end reason (`requested`, `source_ended`, `comm_lost`).
- **REQ-HAL.ACQ.INGEST.CMDS-004** `device.stop_acquisition@1` shall complete only after every admitted batch has been journaled or recorded as a gap, or after a drain timeout of 5 s [unverified], whereupon the remaining ranges shall be recorded as gaps with reason `COMM_LOST`.
- **REQ-HAL.ACQ.INGEST.CMDS-005** Both commands shall be class D1 and shall be projected to MCP only as requests that a human confirms (CONSTITUTION §3).
- **REQ-HAL.ACQ.INGEST.CMDS-006** If a stream ends without a stop command (source end or loss), then the system shall journal a stop summary event with the end reason, so that every started stream has exactly one summary.

## Acceptance criteria

- **AC-1** Given an open session on the LineScan simulator held by a human principal, when `fid device start-acquisition --json` and, after 1 000 batches, `fid device stop-acquisition --json` run, then the stop entry records 1 000 batches, the sample count, zero gaps, the last sequence number and end reason `requested`.
- **AC-2** (error path) Given a session controlled by another principal, when `start-acquisition` runs, then it fails with `session_not_controlled` and journals only the failure.
- **AC-3** (error path) Given device calibration expired and tenant policy `block` (`fixtures/hal/acq/policy/cal_expired.json`), when `start-acquisition` runs, then it fails with `CALIBRATION_EXPIRED` and no stream exists.
- **AC-4** Given the same state under policy `flag`, then the stream starts and every stamp carries calibration status `EXPIRED`.
- **AC-5** Given a simulator that stops answering mid-stream, when `stop-acquisition` runs, then it completes within 5 s and the undrained range is a `COMM_LOST` gap in the summary.
- **AC-6** Given the generated MCP tool list, then `device_start_acquisition` and `device_stop_acquisition` are request-only.

## Design

- **Registry commands:** `device.start_acquisition@1` (D1; params `session_id`, `capability`, optional `stream_options`; result `StreamHandle`); `device.stop_acquisition@1` (D1; params `stream_handle`; result `StopSummary`).
- **Data structures:** `StopSummary { batches: u64, samples: u64, gaps: u32, gaps_digest, last_seq: u64, end_reason }`.
- **Algorithm:** start = lock check -> `check_calibration(state, policy, now)` with `now` taken once from the session timebase and journaled -> create `StreamPipeline` -> gated driver call `start`. Stop = gated driver call `stop` -> drain with timeout -> summary.
- **Errors:** `session_not_controlled`, `CALIBRATION_EXPIRED`, `NOT_QUALIFIED`, `stream_not_found`, driver errors from the `Device@1` error model.
- **Determinism:** N (live device); summaries of a transcript playback are identical across runs.
- **Performance budget:** start and stop overhead under 50 ms excluding the device [unverified until measured].
- **Crate:** `fid-hal-runtime` (module `ingest::cmds`), layer L2.

## Test fixtures

LineScan simulator from `HAL.SIM.SCANSIM` (or a mock source until it exists); policy fixtures reused from `fixtures/hal/acq/policy/`.

## Build prompt

> In crate `fid-hal-runtime`, module `ingest::cmds`, implement registry commands `device.start_acquisition@1` (D1; CLI `fid device start-acquisition`, MCP `device_start_acquisition` request-only) and `device.stop_acquisition@1` (D1; CLI `fid device stop-acquisition`, MCP `device_stop_acquisition` request-only). Start: require the controlling-writer lock (`session_not_controlled`), run `policy::check_calibration` with a journaled `now` (Block -> fail with the reason; Flag -> start and flag every batch), build the `ingest::sink` pipeline, then issue the driver `start` call through the driver-call gate. Stop: issue `stop`, drain with a 5 s timeout, record undrained ranges as `COMM_LOST` gaps, journal `StopSummary { batches, samples, gaps, gaps_digest, last_seq, end_reason }`. A stream that ends without stop still gets exactly one summary. No panic, unwrap, expect or unsafe (CONSTITUTION §8). Tests: AC-1..AC-6.

## Open questions

- The 5 s drain timeout is a proposal [unverified].

## Children

_Leaf: none._
