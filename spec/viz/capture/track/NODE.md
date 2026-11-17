---
id: VIZ.CAPTURE.TRACK
title: View track and re-rendered replay
level: 3
parent: VIZ.CAPTURE
kind: module
status: open
phase: P1b
size: L
owners: []
claimed_by:
satisfies: [REQ-VIZ.CAPTURE-002, REQ-VIZ.CAPTURE-003]
covers: []
depends_on: [VIZ.CAPTURE.REC, VIZ.VIEW.STATE, CORE.JRN]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# VIZ.CAPTURE.TRACK: View track and re-rendered replay

## Purpose

The presentation track of CONSTITUTION §6: camera, visibility and highlight keyframes sampled at about 10 Hz on the journal clock while recording, stored compactly and attached to the journal, and the re-rendered replay that plays a journal range back with the recorded view track (with captions from command titles) without needing pixel video.

## Scope

**In:**
- Keyframe sampling of ViewState deltas at 10 Hz on the journal clock
- View-track storage format (segments, digests, attachment entries)
- Re-rendered replay of a journal range with the view track, live or headless
- Captions generated from localised command titles

**Out** (and which node owns it instead):
- Recording session and policy (`VIZ.CAPTURE.REC`)
- Pixel video (`VIZ.CAPTURE.VIDEO`)
- Timeline panel UI (`VIZ.SHELL`); journal replay engine (`CORE.RPL`)

## Requirements

- **REQ-VIZ.CAPTURE.TRACK-001** While recording, the system shall sample the view track at 10 Hz ± 1 Hz [unverified].
- **REQ-VIZ.CAPTURE.TRACK-002** Every keyframe shall carry a timestamp from the journal's monotonic clock.
- **REQ-VIZ.CAPTURE.TRACK-003** The view track shall be stored in content-addressed segments, each attached to the journal by an entry carrying its digest.
- **REQ-VIZ.CAPTURE.TRACK-004** A keyframe shall store only changes against the previous keyframe, plus a full ViewState at the start of every segment.
- **REQ-VIZ.CAPTURE.TRACK-005** Re-rendered replay of a journal range shall show the camera and visibility recorded in the view track at each journal-clock time.

## Interfaces

- **Offers:** view-track format; replay API for the timeline.
- **Consumes:** `VIZ.VIEW.STATE`; `CORE.JRN` clock and attachment entries; `VIZ.CAPTURE.REC` session state.

## Decomposition hints

SAMPLE, FORMAT (segments and deltas), ATTACH, REPLAY, CAPTION.

## Open questions

- Whether CORE.RPL drives the replay clock or TRACK does is to be fixed with CORE.

## Children

_None yet: status `open`._
