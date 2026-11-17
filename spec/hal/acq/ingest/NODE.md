---
id: HAL.ACQ.INGEST
title: Data-plane ingest, back-pressure, sequencing and gaps
level: 3
parent: HAL.ACQ
kind: module
status: open
phase: P2
size: L
owners: []
claimed_by:
satisfies: [REQ-HAL.ACQ-003, REQ-HAL.ACQ-004]
covers: []
depends_on: [HAL.ACQ.SESS, HAL.ACQ.CONV, HAL.ACQ.TIME, HAL.ACQ.STAMP]
cli: [fid device start-acquisition, fid device stop-acquisition]
mcp: [device_start_acquisition, device_stop_acquisition]
journal: [device.start_acquisition@1, device.stop_acquisition@1]
updated: 2026-11-16
---

# HAL.ACQ.INGEST: Data-plane ingest, back-pressure, sequencing and gaps

## Purpose

The streaming path from a driver's data plane into stamped batches: start and stop of acquisition, credit-based back-pressure, contiguous sequence numbers, explicit gap events, timestamp collection and hand-off to conversion and stamping. It is separate because it carries the throughput budget of the whole HAL and its "no silent drops" rule.

## Scope

**In:**
- `device.start_acquisition@1` and `device.stop_acquisition@1` (D1) with stop summaries (counts, gaps, last sequence)
- Credit-based flow control towards drivers, simulators and playback sources
- Sequence checking and `Gap { from, to, reason }` events
- Shared-memory ring for same-host sources and a stream reader for remote hosts (transport supplied by `HAL.HOST.PROTO`)
- Batch assembly within the recommended size (1–8 MB or 50 ms) and hand-off to `HAL.ACQ.CONV` and `HAL.ACQ.STAMP`
- Per-batch telemetry summaries (rate, latency, gap count)

**Out** (and which node owns it instead):
- Host transport protocol (owned by `HAL.HOST.PROTO`)
- Conversion (owned by `HAL.ACQ.CONV`), time mapping (owned by `HAL.ACQ.TIME`), stamping and journaling (owned by `HAL.ACQ.STAMP`)
- Session control (owned by `HAL.ACQ.SESS`)

## Requirements

- **REQ-HAL.ACQ.INGEST-001** While a stream is active, the ingest shall grant credits to the source and shall never accept data beyond granted credits, so that a slow consumer slows the source instead of dropping samples.
- **REQ-HAL.ACQ.INGEST-002** If a sequence number arrives that is not the successor of the previous one, then the ingest shall record a gap covering the missing range with a reason from the closed gap-reason set, and shall carry it into the next stamp.
- **REQ-HAL.ACQ.INGEST-003** The ingest shall sustain one LineScan stream of 10 000 profiles per second at 2 000 points per profile for 10 minutes without a gap on reference hardware [unverified until measured].
- **REQ-HAL.ACQ.INGEST-004** When `device.stop_acquisition@1` completes, the journal entry shall record the batch count, sample count, gap count and last sequence number.
- **REQ-HAL.ACQ.INGEST-005** If a source sends a batch larger than 64 MB or a schema not negotiated for the session, then the ingest shall reject that batch, record a gap with reason `REJECTED`, and keep the session open.

## Interfaces

- **Offers:** the two D1 commands; `BatchSink` used by drivers, simulators (`HAL.SIM`) and playback (`HAL.XSCR.PLAY`); telemetry summaries.
- **Consumes:** `HAL.ACQ.SESS` session handles; `HAL.ACQ.CONV`; `HAL.ACQ.TIME`; `HAL.ACQ.STAMP`.

## Decomposition hints

CREDIT (flow control), SEQ (sequencing and gaps), SHM (same-host ring), CMDS (start and stop commands and summaries), BENCH (throughput benchmark harness against the LineScan simulator).

## Open questions

- The 64 MB per-batch rejection limit is a proposal [unverified]; the conformance profile recommends 1–8 MB.
- The gap reasons, including `REJECTED`, are defined in `HAL.IDL.STAMP`.

## Children

_None yet: status `open`._
