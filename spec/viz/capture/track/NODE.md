---
id: VIZ.CAPTURE.TRACK
title: View track and re-rendered replay
level: 3
parent: VIZ.CAPTURE
kind: module
status: split
phase: P1b
size: L
owners: []
claimed_by:
satisfies: [REQ-VIZ.CAPTURE-002, REQ-VIZ.CAPTURE-003]
covers: []
depends_on: [VIZ.CAPTURE.REC, VIZ.VIEW.STATE, CORE.JRN]
cli: [fid capture read-track, fid capture render-replay]
mcp: [capture_read_track, capture_render_replay]
journal: [capture.attach_track_segment@*, capture.read_track@*, capture.render_replay@*]
updated: 2026-11-17
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

- Settled: the view track drives the replay clock; `CORE.RPL.RECON.FOLD` only reconstructs document state at an entry (`VIZ.CAPTURE.TRACK.REPLAY`).
- Split: recording (SAMPLE, with the internal `capture.attach_track_segment@1` entry and the published `fid-viewtrack@1` format) and playback (REPLAY, `capture.read_track@1` R0 and `capture.render_replay@1` W2) have different inputs and tests.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| VIZ.CAPTURE.TRACK.SAMPLE | View-track sampling, delta format and attachment (leaf) | 001 002 003 004 | none |
| VIZ.CAPTURE.TRACK.REPLAY | Re-rendered replay with captions (leaf) | 005 | none |
