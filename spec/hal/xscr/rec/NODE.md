---
id: HAL.XSCR.REC
title: Transcript recording at the HAL boundary
level: 3
parent: HAL.XSCR
kind: module
status: open
phase: P2
size: L
owners: []
claimed_by:
satisfies: [REQ-HAL.XSCR-001, REQ-HAL.XSCR-002, REQ-HAL.XSCR-006]
covers: []
depends_on: [HAL.XSCR.FMT, HAL.ACQ.SESS, HAL.ACQ.INGEST, HAL.HOST.PROTO, HAL.HOST.TIER1]
cli: [fid device configure-transcript]
mcp: []
journal: [device.configure_transcript@1]
updated: 2026-11-16
---

# HAL.XSCR.REC: Transcript recording at the HAL boundary

## Purpose

Captures every exchange that crosses the HAL boundary into a transcript while a session is open: control requests and responses, device events and batch digests, for every driver tier and for simulators. Recording is on by default with a size budget, and only an administrator can turn it off. It is separate from the format because it hooks into the runtime's hot paths and carries the overhead budget.

## Scope

**In:**
- Recording taps on the Tier-1 call path, the Tier-2 protocol client and the ingest path (batch digests only)
- Default-on recording per session; the A0 setting `device.configure_transcript@1` to disable it or change budgets
- Size budget per session and per project, with rotation into new segments and a journaled event when the budget is reached
- Writing segment digests into provenance stamps through `HAL.ACQ.STAMP`
- Asynchronous writing so recording never blocks control or data paths

**Out** (and which node owns it instead):
- Format and writer (owned by `HAL.XSCR.FMT`)
- Retention policy beyond the size budget, and deletion (owned by `CORE.STO` policies)
- Recording the semantic journal (owned by `CORE.JRN`)

## Requirements

- **REQ-HAL.XSCR.REC-001** While a session is open and recording is not disabled, the recorder shall write every control request, control response, device event and batch digest crossing the boundary, in the order observed.
- **REQ-HAL.XSCR.REC-002** Disabling recording shall be possible only through `device.configure_transcript@1`, class A0, never on MCP, and the setting change shall be journaled.
- **REQ-HAL.XSCR.REC-003** The recorder shall copy the trace ID from the call context of each control request into its entry and into the matching response entry.
- **REQ-HAL.XSCR.REC-004** If the recorder's queue fills, then it shall record a `gap` entry with the count of entries lost and raise a `device.event`, and shall never block the control or data path.
- **REQ-HAL.XSCR.REC-005** Recording shall add no more than 5 % CPU to a 10 000 profiles-per-second LineScan session on reference hardware [unverified until measured].
- **REQ-HAL.XSCR.REC-006** When the session's size budget is reached, the recorder shall close the current segment, journal a `device.event`, and continue recording control and event entries while dropping only batch-digest entries beyond the budget, each drop counted in a `gap` entry.

## Interfaces

- **Offers:** recording taps; `device.configure_transcript@1` (A0); segment digests for stamps.
- **Consumes:** `HAL.XSCR.FMT` writer; `HAL.ACQ.SESS` call contexts; `HAL.ACQ.INGEST` batch path; `HAL.HOST.PROTO` and `HAL.HOST.TIER1` call paths.

## Decomposition hints

TAP (taps per tier), QUEUE (asynchronous writer and back-pressure-free queue), BUDGET (size budgets and rotation), CFG (A0 setting; likely a leaf), BENCH (overhead benchmark).

## Open questions

- Whether a budget overrun should ever stop acquisition for regulated tenants (to keep the transcript complete) is a `TRUST.PROFILE` question; this node assumes acquisition continues.

## Children

_None yet: status `open`._
