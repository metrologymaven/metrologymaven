---
id: VIZ.CAPTURE.TRACK.SAMPLE
title: View-track sampling, delta format and attachment
level: 4
parent: VIZ.CAPTURE.TRACK
kind: task
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-VIZ.CAPTURE.TRACK-001, REQ-VIZ.CAPTURE.TRACK-002, REQ-VIZ.CAPTURE.TRACK-003, REQ-VIZ.CAPTURE.TRACK-004]
covers: []
depends_on: [VIZ.CAPTURE.REC.SESSION, VIZ.VIEW.STATE, CORE.STO.CAS, CORE.JRN.CANON, CORE.REG.DEF]
uses_artifacts: []
side_effect: W2
determinism: D0
cli: []
mcp: []
journal: [capture.attach_track_segment@1]
updated: 2026-11-17
---

# VIZ.CAPTURE.TRACK.SAMPLE: View-track sampling, delta format and attachment

## Purpose

While a recording with source `view_track` is active, samples each viewport's ViewState (camera, clipping, visibility, transparency, colour-map and label references) plus the current highlight set at 10 Hz on the journal clock, stores keyframes as changes against the previous keyframe with a full ViewState at the start of each 10 s segment, and attaches each segment to the journal with one internal entry carrying its digest. The format `fid-viewtrack@1` is published with a JSON Schema so the replay and the web viewer can read it.

## Scope

**In:** the `view_track` `SessionObserver`; a 100 ms sampling timer aligned to the journal monotonic clock; ViewState diffing by canonical JSON Pointer paths (RFC 6902-style `replace`, `add`, `remove` operations); segment cutting at 100 keyframes; the `fid-viewtrack@1` format and `viewtrack.schema.json`; `capture.attach_track_segment@1` (W2, exposure `internal`).
**Out:** ViewState model and canonical form (`VIZ.VIEW.STATE`); session and policy (`VIZ.CAPTURE.REC.SESSION`); replay (`VIZ.CAPTURE.TRACK.REPLAY`); verification (`VIZ.CAPTURE.VIDEO.VERIFY`).

## Requirements

- **REQ-VIZ.CAPTURE.TRACK.SAMPLE-001** While a recording with source `view_track` is active, the sampler shall take one keyframe per viewport every 100 ms ± 10 ms of journal monotonic time, so the rate stays within 10 Hz ± 1 Hz.
- **REQ-VIZ.CAPTURE.TRACK.SAMPLE-002** Every keyframe shall carry the journal monotonic timestamp at which its ViewState was read.
- **REQ-VIZ.CAPTURE.TRACK.SAMPLE-003** The first keyframe of each segment shall hold the full canonical ViewState, and every later keyframe shall hold only the patch operations from the previous keyframe's ViewState (an empty list when nothing changed).
- **REQ-VIZ.CAPTURE.TRACK.SAMPLE-004** Applying a segment's patches in order to its first ViewState shall reproduce, byte for byte in canonical form, the ViewState read at every keyframe.
- **REQ-VIZ.CAPTURE.TRACK.SAMPLE-005** Each segment of 100 keyframes (or fewer at stop) shall be stored in the content-addressed store and attached by one `capture.attach_track_segment@1` entry carrying its SHA-256, the recording ID, the viewport ID and the sequence number; the command shall have exposure `internal`.
- **REQ-VIZ.CAPTURE.TRACK.SAMPLE-006** If sampling falls behind by more than one interval, then the sampler shall skip to the next interval boundary and record the skipped interval in the segment, never emitting two keyframes for one interval.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/viz/capture/camera-script-25s` (a scripted camera path on `fixtures/viz/golden-cube`) and a fake clock, when recorded, then 250 keyframes ± 1 are produced in three segments (100, 100, 50), each attached once with a digest equal to the stored blob's SHA-256.
- **AC-2** Given AC-1's segments, when patches are applied, then every reconstructed ViewState is byte-identical to the one the test logged at that keyframe.
- **AC-3** Given a static scene for 2 s, then those keyframes carry empty patch lists.
- **AC-4** Given a sampler stalled for 350 ms, then three intervals are recorded as skipped and no interval holds two keyframes.
- **AC-5** Given the segment files, then each validates against `viewtrack.schema.json`.

## Design

- **Registry command(s):** `capture.attach_track_segment@1`, W2, exposure `internal`; params `{ recording_id, viewport, seq: u32, segment: Digest }`.
- **Data structures:** `TrackSegment { format: "fid-viewtrack@1", recording_id, viewport, seq, base: ViewState, keyframes: [{ mono_ns, ops: [PatchOp] }], skipped: [(u64, u64)] }` in canonical JSON (RFC 8785), uncompressed so the format stays simple to verify.
- **Algorithm:** timer on the capture thread; read ViewState snapshots from the view-state store; diff by walking both canonical trees in key order; cut and attach every 100 keyframes.
- **Errors:** `StorageFull` (stops the recording through the session).
- **Determinism:** D0 for diffing and serialisation given the sampled states (sampling instants are observed).
- **Performance budget:** one keyframe < 1 ms for a ViewState with 10,000 visibility entries [unverified until measured].
- **Crate:** `fid-capture` (module `track::sample`).

## Test fixtures

`fixtures/viz/capture/camera-script-25s` (generated script and expected states), `fixtures/viz/golden-cube` (shared); CC-BY-4.0.

## Build prompt

> In crate `fid-capture`, add module `track::sample`: a `SessionObserver` for source `view_track` that samples every viewport's canonical ViewState (from `VIZ.VIEW.STATE`) every 100 ms of journal monotonic time, writes keyframes as RFC 6902-style patch lists against the previous keyframe with a full base ViewState per segment, cuts segments at 100 keyframes and at stop, stores each segment (format `fid-viewtrack@1`, canonical JSON, schema `viewtrack.schema.json`) in the project CAS and commits `capture.attach_track_segment@1` (W2, exposure internal, no CLI/MCP) with its digest, recording ID, viewport and seq. Skip, and record, intervals the sampler missed. Tests AC-1..AC-5 with a fake clock. Rules: no panics or `unsafe`; deny unwrap/expect/panic/todo/unimplemented/unreachable.

## Open questions

- Highlight sets live in `VIZ.VIEW.CAMERA` (open); if they are not part of ViewState, the sampler needs a second read; assumed to be in ViewState here.

## Children

_Leaf: none._
