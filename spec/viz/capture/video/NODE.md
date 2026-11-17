---
id: VIZ.CAPTURE.VIDEO
title: Viewport video capture in hash-chained segments
level: 3
parent: VIZ.CAPTURE
kind: module
status: open
phase: P1b
size: L
owners: []
claimed_by:
satisfies: [REQ-VIZ.CAPTURE-002, REQ-VIZ.CAPTURE-004, REQ-VIZ.CAPTURE-005, REQ-VIZ.CAPTURE-011]
covers: []
depends_on: [VIZ.CAPTURE.REC, VIZ.RENDER.FRAME, CORE.JRN]
cli: [fid capture attach-segment]
mcp: [capture_attach_segment]
journal: [capture.attach_segment@*]
updated: 2026-11-16
---

# VIZ.CAPTURE.VIDEO: Viewport video capture in hash-chained segments

## Purpose

Record what the viewport actually displayed: GPU frame readback through staging buffers, encoding (hardware where available), about 10 s content-addressed segments each attached to the journal by an entry carrying its digest, so the video is tamper-evident through the hash chain without bloating the journal; and verification that reports tampered segments.

## Scope

**In:**
- Asynchronous frame readback with bounded staging buffers
- Encoder integration (codec to be chosen) and segmenting at about 10 s
- Frame timestamps on the journal clock
- `capture.attach_segment@1` entries with segment digests
- Segment verification against journal digests
- Overhead budget enforcement (drop to lower capture rate rather than stall the viewport)

**Out** (and which node owns it instead):
- Recording session and policy (`VIZ.CAPTURE.REC`)
- Full-window OS capture (`VIZ.CAPTURE.OSCAP`)
- Package-level verification reports (`TRUST.VERIFY`)

## Requirements

- **REQ-VIZ.CAPTURE.VIDEO-001** Every encoded frame shall carry a timestamp from the journal's monotonic clock.
- **REQ-VIZ.CAPTURE.VIDEO-002** Video shall be stored in content-addressed segments of 10 s ± 1 s, each attached by a journal entry carrying its SHA-256 digest.
- **REQ-VIZ.CAPTURE.VIDEO-003** If a stored segment's digest does not match its journal entry, then verification shall report that segment as tampered.
- **REQ-VIZ.CAPTURE.VIDEO-004** Capture at 1920×1080 and 30 fps shall reduce viewport frame rate by no more than 10 % on the reference profile [unverified].
- **REQ-VIZ.CAPTURE.VIDEO-005** If capture cannot keep up, then the system shall reduce the capture frame rate and record the drop in the segment metadata rather than stall the viewport.

## Interfaces

- **Offers:** `capture.attach_segment@1`; segment format and verification function.
- **Consumes:** `VIZ.RENDER.FRAME` presented frames; `CORE.JRN` clock and append; `VIZ.CAPTURE.REC` session.

## Decomposition hints

READBACK, ENCODE, SEGMENT, ATTACH, VERIFY, BUDGET.

## Open questions

- Codec choice must be licence-compatible and hardware-encodable; it depends on D-001's licence allowlist and may need its own decision before ENCODE can be finished.

## Children

_None yet: status `open`._
