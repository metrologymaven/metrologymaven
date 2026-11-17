---
id: HAL.ACQ.INGEST.SINK
title: BatchSink, batch assembly and hand-off pipeline
level: 4
parent: HAL.ACQ.INGEST
kind: task
status: leaf
phase: P2
size: M
owners: []
claimed_by:
satisfies: [REQ-HAL.ACQ.INGEST-001, REQ-HAL.ACQ.INGEST-002, REQ-HAL.ACQ.INGEST-003]
covers: []
depends_on: [HAL.ACQ.INGEST.CREDIT, HAL.ACQ.INGEST.SEQ, HAL.ACQ.CONV, HAL.ACQ.TIME, HAL.ACQ.POLICY, HAL.ACQ.STAMP, HAL.ACQ.SESS.LIFE]
side_effect: D1
determinism: N
cli: []
mcp: []
journal: [device.acquisition_batch]
updated: 2026-11-17
---

# HAL.ACQ.INGEST.SINK: BatchSink, batch assembly and hand-off pipeline

## Purpose

The `BatchSink` that every source writes into (drivers through `HAL.HOST.PROTO` or Tier-1 calls, simulators, transcript playback) and the fixed pipeline each batch then follows: admission (sequence and credit), assembly into the recommended size, timestamp mapping, conversion, calibration flags and claim classification, and finally stamping and journaling. It is separate because it is the one place where the order of these steps is fixed and where per-batch telemetry is measured.

## Scope

**In:**
- The `BatchSink` trait (`offer(header, payload) -> Admission`, `end_of_stream(summary)`) and its implementation per active stream
- Pipeline order: `SEQ` check -> `CREDIT` admit -> assemble -> `TIME` mapping -> `CONV` decode -> `POLICY` flags and classification -> `STAMP` writer -> credit acknowledgement
- Batch assembly: coalescing small source batches up to 8 MB or 50 ms, whichever comes first; never splitting a source batch
- Fusion of two streams in one batch (for example pose plus profile) with the `time_sync_insufficient` flag from `HAL.ACQ.TIME`
- Per-batch telemetry summary (rate, latency from device time to journal, gap count) as stamp-adjacent telemetry

**Out:**
- Wire transports, shared-memory rings and remote streams (owned by `HAL.HOST.PROTO`, which pushes into this sink)
- Start and stop commands and the stop summary (owned by `HAL.ACQ.INGEST.CMDS`)
- The functions it calls (owned by `HAL.ACQ.INGEST.CREDIT`, `HAL.ACQ.INGEST.SEQ`, `HAL.ACQ.TIME`, `HAL.ACQ.CONV`, `HAL.ACQ.POLICY`, `HAL.ACQ.STAMP`)
- Throughput measurement (owned by `HAL.ACQ.INGEST.BENCH`)

## Requirements

- **REQ-HAL.ACQ.INGEST.SINK-001** The sink shall run the pipeline steps in the fixed order listed in Scope for every batch, and shall return credit for a batch only after the stamping writer has journaled or rejected it.
- **REQ-HAL.ACQ.INGEST.SINK-002** The sink shall attach every pending gap from the sequence checker to the next batch it hands to the stamping writer on that stream.
- **REQ-HAL.ACQ.INGEST.SINK-003** When assembling, the sink shall coalesce consecutive admitted source batches until the assembled batch reaches 8 MB or spans 50 ms of device time, and shall never split one source batch across two assembled batches.
- **REQ-HAL.ACQ.INGEST.SINK-004** When two streams are fused into one batch and the `HAL.ACQ.TIME` fusion rule fails, the sink shall set `time_sync_insufficient` on the fused batch and still journal it.
- **REQ-HAL.ACQ.INGEST.SINK-005** If any pipeline step after admission fails for a batch (conversion, policy input, stamp validation), then the sink shall record that batch's range as a gap with reason `REJECTED`, raise a `device.event` naming the step, and keep the stream active.
- **REQ-HAL.ACQ.INGEST.SINK-006** The sink shall add no more than 2 ms median latency from batch receipt to journal append at the LineScan reference load [unverified until measured].
- **REQ-HAL.ACQ.INGEST.SINK-007** The sink shall take trust grade, negotiation result and source kind from the session context supplied by `HAL.ACQ.SESS.LIFE`, never from the source.

## Acceptance criteria

- **AC-1** Given a mock source emitting 100 LineScan batches of 200 KB at 1 ms intervals (`fixtures/hal/acq/ingest/sink/ls_small.json`), when streamed, then assembled batches are at most 8 MB, none spans more than 50 ms, every source batch appears in exactly one assembled batch, and every stamp is valid.
- **AC-2** Given a mock source that skips sequence range 500..599, when streamed, then the next journaled stamp carries gap `{500, 599, DEVICE_DROP}`.
- **AC-3** (error path) Given a batch whose unmasked payload contains an f32 infinity, when streamed, then it is not journaled, its range is a `REJECTED` gap on the next stamp, a `device.event` names step `conv`, and later batches are journaled.
- **AC-4** Given fused pose (DEVICE_CLOCK, sigma 40 us) and profile (HOST_RECEIVE, sigma 2 ms) streams with a 100 us threshold, when fused, then the batch is journaled with `time_sync_insufficient`.
- **AC-5** Given a source that declares `trust: attested` and `simulated: false` while the session context says `trust: declared` and source kind `simulator`, when streamed, then stamps say `declared` and `simulated: true`.
- **AC-6** Given a stamping writer stub that delays acknowledgement, when the source keeps offering, then admissions stop at the credit window and resume after acknowledgement, with no gap recorded.

## Design

- **Registry commands:** none; instantiated by `device.start_acquisition@1` (D1) and torn down by `device.stop_acquisition@1`.
- **Data structures:** `trait BatchSink`, `StreamPipeline { seq: SeqState, credit: CreditLedger, assembler: Assembler, clock: ClockModel, ctx: SessionContext }`, `TelemetrySummary { rate_hz, p50_latency_ns, p99_latency_ns, gaps }`.
- **Algorithm:** single consumer task per stream; bounded queues sized by the credit window; assembly by byte and device-time thresholds; telemetry by fixed-bucket histograms (no floating-point state of record).
- **Errors:** step failures become `REJECTED` gaps plus `device.event`; the stream stays active.
- **Determinism:** N for live timing; given a recorded arrival order (transcript playback) the journaled batches are identical, which `HAL.XSCR.PLAY` relies on.
- **Performance budget:** see REQ-HAL.ACQ.INGEST.SINK-006 and `HAL.ACQ.INGEST.BENCH`.
- **Crate:** `fid-hal-runtime` (module `ingest::sink`), layer L2.

## Test fixtures

Mock sources and stub writers in the crate's test support; scripts under `fixtures/hal/acq/ingest/sink/` (CC-BY-4.0).

## Build prompt

> In crate `fid-hal-runtime`, module `ingest::sink`, implement the `BatchSink` trait (`offer(header, payload) -> Admission`, `end_of_stream(summary)`) and a per-stream `StreamPipeline` that runs, in this order: sequence check (`ingest::seq`), credit admit (`ingest::credit`), assembly (coalesce to at most 8 MB or 50 ms of device time; never split a source batch), time mapping (`time` module), decode (`conv`), calibration flags and `ExternalClaim` classification (`policy`), then the stamping writer (`HAL.ACQ.STAMP`), and only then acknowledge credit. Attach pending gaps to the next stamped batch; turn any post-admission failure into a `REJECTED` gap plus a `device.event` naming the step, keeping the stream active; set `time_sync_insufficient` on fused batches when the fusion rule fails. Trust, negotiation and source kind come only from the `SessionContext` of `HAL.ACQ.SESS.LIFE`. Emit per-batch telemetry summaries. No panic, unwrap, expect or unsafe; BTreeMap only (CONSTITUTION §8). Tests: AC-1..AC-6 with mock sources and fixtures under `fixtures/hal/acq/ingest/sink/`.

## Open questions

- Whether telemetry summaries are journaled with the batch or only to the Activity Log is a `CORE.JRN` sizing question; this leaf emits them as a separate stream and lets the journal decide.

## Children

_Leaf: none._
