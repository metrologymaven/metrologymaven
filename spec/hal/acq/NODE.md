---
id: HAL.ACQ
title: Device runtime and acquisition manager
level: 2
parent: HAL
kind: subsystem
status: split
phase: P1a
size: XL
owners: []
claimed_by:
satisfies: [REQ-HAL-004, REQ-HAL-008]
covers: []
depends_on: [HAL.IDL, CORE]
cli: [fid device *-session, fid device *-acquisition, fid device configure-*, fid device show-*]
mcp: [device_*_session, device_*_acquisition, device_configure_*, device_show_*]
journal: [device.*_session, device.*_acquisition, device.configure_*, device.acquisition_batch]
updated: 2026-11-16
---

# HAL.ACQ: Device runtime and acquisition manager

## Purpose

The engine-side runtime that turns any driver, simulator, playback source or file pseudo-device into journaled, stamped observations. It keeps the capability registry, opens sessions, supervises health, owns time mapping, applies back-pressure, and is the **sole writer** of `AcquisitionBatch` and its provenance stamp. It is separate because every device path converges here, and the stamp-before-journal invariant must hold in exactly one place.

## Scope

**In:**
- Device registry of connected instances and their negotiated capabilities
- Sessions: one controlling writer plus telemetry subscribers; open, close, interrupt and resume
- Configuration changes as D1 commands, journaled with before and after digests
- Health supervision and the health state machine; telemetry summaries per batch
- Session timebase (host monotonic ns anchored once to UTC), per-device offset and drift estimation, and the time-sync-insufficient flag
- Data-plane ingest: credit-based back-pressure, contiguous sequence numbers, explicit gap events, conversion from declared encodings to f64 metres
- Provenance stamping: the runtime adds what it knows independently (host digest, receive time, session, negotiation, trust grade, `simulated`)
- Content-addressed storage of raw batches and journaling of `AcquisitionBatch` events to CORE
- Device-computed values: classifying them as calibrated observations or `ExternalClaim`s
- Calibration and qualification state checks at acquisition time (flag or block per policy)
- File pseudo-devices: stamping observations imported through IO readers (`declared` trust) and manual gauge entries (`entered` provenance)

**Out** (and which node owns it instead):
- Contracts and schemas (owned by `HAL.IDL`)
- Driver processes, tiers and supervision of host processes (owned by `HAL.HOST`)
- Recording and replaying raw I/O transcripts (owned by `HAL.XSCR`)
- Trust inputs from pairing and driver signatures (owned by `HAL.PAIR`)
- Portable-device workflows built on sessions (owned by `HAL.LIVE`)
- Release blocking of results derived from simulated data (owned by `TRUST` and `RPT`; this node only sets the flag)

## Requirements

- **REQ-HAL.ACQ-001** The acquisition manager shall be the only component that writes `AcquisitionBatch` events, and it shall attach a complete provenance stamp to every batch before the batch is journaled.
- **REQ-HAL.ACQ-002** When the serving source is a simulator, emulator or transcript playback, the acquisition manager shall set `simulated = true` on every batch, and no later command shall be able to clear that flag on the batch or on anything derived from it.
- **REQ-HAL.ACQ-003** While a session is streaming, the acquisition manager shall apply credit-based back-pressure so that no sample is silently dropped; every missing sequence range shall be recorded as a gap with a reason in the stamp.
- **REQ-HAL.ACQ-004** The acquisition manager shall ingest a single LineScan stream at 10 000 profiles per second of 2 000 points each without gaps on reference hardware [unverified until measured].
- **REQ-HAL.ACQ-005** When a driver supplies data in a declared scaled-integer or f32 encoding, the acquisition manager shall convert it to f64 metres deterministically (D0) and store the raw batch by digest, so replay never depends on a conversion done inside a driver.
- **REQ-HAL.ACQ-006** When two streams are fused (for example pose plus profile) and their time sources differ with sync uncertainty above the capability threshold, the acquisition manager shall flag the fused batch `time_sync_insufficient`.
- **REQ-HAL.ACQ-007** If a device reports an expired calibration or qualification, then the acquisition manager shall flag every affected batch by default, and shall refuse acquisition when the tenant policy says block.
- **REQ-HAL.ACQ-008** When a driver reports a device-derived quantity (for example an on-board diameter), the acquisition manager shall record it as an `ExternalClaim` with the device as algorithm source and never as a value of record.
- **REQ-HAL.ACQ-009** Every state-changing call to a driver shall originate from a journaled registry command of class D1 (or M1 through the Motion Executive), and MCP shall expose D1 commands only as requests (CONSTITUTION §3).

## Interfaces

- **Offers:** `device.open_session@1` (D1), `device.close_session@1` (D1), `device.start_acquisition@1` (D1), `device.stop_acquisition@1` (D1), `device.configure_*` (D1), `device.show_*` (R0); events `device.session_opened`, `device.acquisition_batch`, `device.event`; observation handles for LIVE, LVM, SCAN and CMM.
- **Consumes:** HAL.IDL schemas; CORE command bus, journal and content-addressed store; trust grades from HAL.PAIR; driver channels from HAL.HOST.

## Decomposition hints

REG (device registry and negotiation execution), SESS (sessions, configuration, health), TIME (timebase and sync estimation), INGEST (data plane, back-pressure, gaps, encoding conversion), STAMP (stamping and journaling), POLICY (calibration state, `ExternalClaim` classification), FILEDEV (file and manual-entry pseudo-devices).

## Open questions

- Production `device_io` on MCP: concept doc 08 (H-D18) proposes none in P2 beyond status and configuration drafts. CONSTITUTION §3 already makes D1 request-only; confirm whether request-only D1 tools ship in P2 or later.

## Split rationale

The split follows **the life of one batch** through the runtime: control (SESS) opens the session; data arrives through ingest (INGEST), is time-mapped (TIME) and converted (CONV), is checked against calibration and classified (POLICY), and is stamped, stored and journaled by the single writer (STAMP). File imports and manual entries (FILEDEV) join the same path without a live driver, which is why they are a sibling rather than an IO concern. TIME, CONV and POLICY are pure functions and are finished as leaves.

Alternatives considered:
- **Control plane versus data plane (two children).** Rejected: each would be XL, and the pure functions would be buried inside them.
- **FILEDEV inside STAMP.** Rejected: it is the P1a deliverable and has its own commands and IO dependency.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| HAL.ACQ.SESS | Device registry, sessions, configuration and health supervision | 009 | none |
| HAL.ACQ.TIME | Session timebase, clock offset and drift estimation, sync flag — leaf | 006 | none |
| HAL.ACQ.CONV | Deterministic conversion of declared encodings to f64 SI values — leaf | 005 | none |
| HAL.ACQ.INGEST | Data-plane ingest, back-pressure, sequencing and gaps | 003 004 | none |
| HAL.ACQ.STAMP | Provenance stamping, raw-batch storage and batch journaling | 001 002 | none |
| HAL.ACQ.POLICY | Calibration-state policy and ExternalClaim classification — leaf | 007 008 | none |
| HAL.ACQ.FILEDEV | File pseudo-devices and manual-entry provenance | 001 | none |
