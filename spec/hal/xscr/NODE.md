---
id: HAL.XSCR
title: Instrument I/O transcripts and playback
level: 2
parent: HAL
kind: subsystem
status: split
phase: P2
size: L
owners: []
claimed_by:
satisfies: [REQ-HAL-005]
covers: []
depends_on: [HAL.IDL, HAL.ACQ, CORE]
cli: [fid device *-transcript]
mcp: [device_*_transcript]
journal: [device.*_transcript]
updated: 2026-11-16
---

# HAL.XSCR: Instrument I/O transcripts and playback

## Purpose

Recording layer 3 (CONSTITUTION §6): every exchange between the runtime and a driver, captured at the HAL boundary, aligned with the journal, and replayable as a device. Playback lets any workflow, test or bug report run again without the instrument. It is separate because it is a cross-cutting recording facility with its own format, retention and privacy concerns.

## Scope

**In:**
- Transcript format: control messages, data-plane batch digests (payloads by reference to content-addressed storage), events, timings and trace IDs
- Recording at the HAL boundary for every tier, on by default per session with a size budget
- Alignment with the semantic journal through trace IDs and segment digests in the provenance stamp
- Playback device: a source that implements the recorded capabilities and replays a transcript, as-fast-as-possible or at recorded timing
- Transcript inspection, trimming and export as a bug bundle (with redaction of serials and untrusted text on request)
- Retention and storage limits

**Out** (and which node owns it instead):
- Journal replay semantics and replay outcomes (owned by `CORE`)
- I++ controller replay at fidelity L3, built on this playback (owned by `HAL.SIM`)
- Stamping `simulated` on playback batches (owned by `HAL.ACQ`)
- Egress of bug bundles off the machine (an X1 command owned by `AUTO`/`ECO` support tooling)

## Requirements

- **REQ-HAL.XSCR-001** While a device session is open, the HAL shall record every control request, control response, device event and data-batch digest crossing the HAL boundary into a transcript, unless recording is disabled by an admin (A0) setting.
- **REQ-HAL.XSCR-002** Every transcript entry shall carry the trace ID of the journaled command that caused it, so each journal entry can be matched to its transcript segment.
- **REQ-HAL.XSCR-003** When a transcript is played back as a device, the playback source shall produce batches whose payload digests equal those recorded, and the runtime shall stamp them `simulated = true`.
- **REQ-HAL.XSCR-004** If a replayed command sequence diverges from the recorded control requests, then playback shall stop with a typed divergence error naming the first differing entry.
- **REQ-HAL.XSCR-005** The transcript reader shall be bounded and fail closed: a malformed or truncated transcript shall produce a typed error, never a crash.
- **REQ-HAL.XSCR-006** Transcript recording shall add no more than 5 % CPU overhead to a 10 000 profiles-per-second LineScan session on reference hardware [unverified until measured].

## Interfaces

- **Offers:** transcript format; playback device; `device.export_transcript@1` (R0 to an approved export root), `device.play_transcript@1` (I1), `device.show_transcript@1` (R0), `device.trim_transcript@1` (W2).
- **Consumes:** HAL.IDL schemas; HAL.ACQ hooks at the boundary; CORE content-addressed store and journal.

## Decomposition hints

FMT (transcript format and reader), REC (boundary recording), PLAY (playback device and divergence detection), TOOLS (inspect, trim, redact, export).

## Open questions

- Transcripts of NDA vendor-SDK hosts may reveal protected protocol detail; whether redaction is enough or such transcripts must stay local needs a decision when the first NDA host exists.

## Split rationale

The split follows **the hint list and the data flow of a transcript**: one format with a bounded reader that everything shares (FMT), capture on the hot path (REC), turning a transcript back into a device (PLAY), and the user-facing inspection, trim and export commands (TOOLS). FMT, PLAY and TOOLS are small and concrete and are finished as leaves; REC touches every driver tier and carries the overhead budget, so it stays open.

Alternatives considered:
- **Format and recording together.** Rejected: the format is a pure library needed by playback and tools before recording is wired into every tier.
- **Export inside AUTO support tooling.** Rejected: local export with redaction is a transcript operation; only egress off the machine (X1) belongs elsewhere.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| HAL.XSCR.FMT | Transcript format, writer and bounded reader — leaf | 002 005 | none |
| HAL.XSCR.REC | Transcript recording at the HAL boundary | 001 002 006 | none |
| HAL.XSCR.PLAY | Transcript playback device and divergence detection — leaf | 003 004 | none |
| HAL.XSCR.TOOLS | Transcript inspection, trimming, redaction and export — leaf | 002 | none |
