---
id: HAL.ACQ.INGEST.SEQ
title: Sequencing, gap detection and batch admission
level: 4
parent: HAL.ACQ.INGEST
kind: task
status: leaf
phase: P2
size: S
owners: []
claimed_by:
satisfies: [REQ-HAL.ACQ.INGEST-002, REQ-HAL.ACQ.INGEST-005]
covers: []
depends_on: [HAL.IDL.STAMP, HAL.IDL.NEG]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# HAL.ACQ.INGEST.SEQ: Sequencing, gap detection and batch admission

## Purpose

Checks every arriving batch against the stream's expected sequence number and against the session's negotiated schema and size limit, and turns every irregularity into an explicit gap with a reason. It is the place where "no silent drops" becomes a recorded fact in the next stamp.

## Scope

**In:**
- Expected-successor check per stream and the `Gap { from, to, reason }` it produces
- Admission checks: payload schema negotiated for the session, batch size at most 64 MB [unverified]
- A pending-gap list carried into the next stamped batch, and flushed into the stop summary at stream end
- Handling of duplicates and out-of-order sequence numbers

**Out:**
- Credit accounting (owned by `HAL.ACQ.INGEST.CREDIT`)
- The gap-reason enum and gap type (owned by `HAL.IDL.STAMP`)
- Writing gaps into stamps (owned by `HAL.ACQ.STAMP`)

## Requirements

- **REQ-HAL.ACQ.INGEST.SEQ-001** If a batch's first sequence number is greater than the expected successor, then the checker shall record a gap covering the missing range with reason `DEVICE_DROP`, or `COMM_LOST` or `DRIVER_RESTART` when the session reported that event since the last batch.
- **REQ-HAL.ACQ.INGEST.SEQ-002** If a batch's sequence range starts before the expected successor (duplicate or reordered data), then the checker shall reject the whole batch with reason `REJECTED` and shall not move the expected successor backwards.
- **REQ-HAL.ACQ.INGEST.SEQ-003** If a batch is larger than 64 MB or declares a payload schema not negotiated for the session, then the checker shall reject it, record its sequence range as a gap with reason `REJECTED`, and leave the session open.
- **REQ-HAL.ACQ.INGEST.SEQ-004** Every recorded gap shall be attached to the next batch that is stamped on the same stream, or to the stop summary if no further batch is stamped.
- **REQ-HAL.ACQ.INGEST.SEQ-005** Given the same batch-header sequence and session events, the checker shall produce the same admissions and gaps.

## Acceptance criteria

- **AC-1** Given headers with sequence ranges `[0,99] [100,199] [300,399]` from `fixtures/hal/acq/ingest/seq/drop.json`, when checked, then the third batch is admitted and gap `{200, 299, DEVICE_DROP}` is pending.
- **AC-2** Given the same headers with a `COMM_LOST` session event before the third batch, then the gap reason is `COMM_LOST`.
- **AC-3** (error path) Given a batch `[150,249]` after `[100,199]`, when checked, then it is rejected with `REJECTED` and the expected successor stays 200.
- **AC-4** (error path) Given a 65 MB batch header, when checked, then it is rejected, gap reason `REJECTED` covers its range and the session state is unchanged.
- **AC-5** (error path) Given a header declaring payload schema `areascan-frame/1` in a session that negotiated only `LineScan@1`, when checked, then it is rejected with `REJECTED`.
- **AC-6** Given AC-1 followed by stream stop with no more batches, then the stop summary carries the pending gap.

## Design

- **Registry commands:** none; called by `HAL.ACQ.INGEST.SINK` for each arriving batch header.
- **Data structures:** `SeqState { expected: u64, pending_gaps: Vec<Gap> }`, `BatchHeader { seq_from, seq_to, bytes, payload_schema }`, `Admission { Admit | Reject(Gap) }`, `SessionEvent { CommLost, DriverRestart }`. Negotiated schemas come from the `NegotiationResult` type of `HAL.IDL.NEG`.
- **Algorithm:** compare against `expected`; on admit set `expected = seq_to + 1`; gaps appended in arrival order.
- **Errors:** none raised to the caller; rejections are values.
- **Determinism:** D0.
- **Performance budget:** under 200 ns per header on reference hardware [unverified until measured].
- **Crate:** `fid-hal-runtime` (module `ingest::seq`), layer L2.

## Test fixtures

Project-authored header scripts under `fixtures/hal/acq/ingest/seq/` (CC-BY-4.0).

## Build prompt

> In crate `fid-hal-runtime`, module `ingest::seq`, implement `SeqState::check(&mut self, h: &BatchHeader, events: &[SessionEvent], negotiated: &NegotiationResult) -> Admission`. Rules: forward jump -> admit and push `Gap { from: expected, to: seq_from - 1, reason }` with reason `DEVICE_DROP`, or `COMM_LOST` / `DRIVER_RESTART` if that session event occurred since the last batch; overlap or reorder -> reject with `REJECTED` without moving `expected` backwards; size > 64 MB or schema not in the negotiated set -> reject and record the range as `REJECTED`, session stays open. Provide `take_pending_gaps()` for the stamping path and the stop summary. Gap types from `fid-hal-idl`. No panic, unwrap, expect or unsafe (CONSTITUTION §8). Tests: AC-1..AC-6 with fixtures in `fixtures/hal/acq/ingest/seq/`.

## Open questions

- The 64 MB rejection limit is a proposal [unverified]; the conformance profiles recommend 1 to 8 MB (`HAL.IDL.SENS.STREAM`).

## Children

_Leaf: none._
