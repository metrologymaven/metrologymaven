---
id: VIZ.CAPTURE.TRACK.REPLAY
title: Re-rendered replay with captions
level: 4
parent: VIZ.CAPTURE.TRACK
kind: feature
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-VIZ.CAPTURE.TRACK-005]
covers: []
depends_on: [VIZ.CAPTURE.TRACK.SAMPLE, VIZ.CAPTURE.REC.POLICY, VIZ.RENDER.HEADLESS, CORE.RPL.RECON.FOLD, CORE.STO.CAS, CORE.REG.QRY]
uses_artifacts: []
side_effect: [R0, W2]
determinism: D1
cli: [fid capture read-track, fid capture render-replay]
mcp: [capture_read_track, capture_render_replay]
journal: [capture.read_track@1, capture.render_replay@1]
updated: 2026-11-17
---

# VIZ.CAPTURE.TRACK.REPLAY: Re-rendered replay with captions

## Purpose

Plays a journal range back as pictures without pixel video: for each output time, the document state is reconstructed at the last journal entry at or before that time (`CORE.RPL.RECON.FOLD`), the view track supplies the camera and visibility at that time, and the frame is rendered. Captions show the localised title of the command whose entry is current. The timeline panel uses the live path; `capture.render_replay@1` renders the same thing headlessly into a stored frame sequence that `VIZ.CAPTURE.VIDEO` can encode or `VIZ.CAPTURE.EXPORT` can export. The view track drives the replay clock; CORE only reconstructs state.

## Scope

**In:**
- `capture.read_track@1` (R0): ViewState of a viewport at a journal monotonic time, from the track
- `capture.render_replay@1` (W2): headless frames over a time range at a chosen frame rate, stored as a content-addressed frame sequence with a manifest
- Replay clock: output time to (journal entry, track keyframe)
- Captions from the registry's localised command titles (`CORE.REG.QRY`), drawn as an overlay

**Out** (and which node owns it instead):
- Track recording (`VIZ.CAPTURE.TRACK.SAMPLE`); state reconstruction (`CORE.RPL.RECON.FOLD`)
- Timeline panel UI (`VIZ.SHELL`); off-screen rendering (`VIZ.RENDER.HEADLESS`)
- Pixel video of the replay (`VIZ.CAPTURE.VIDEO`), export (`VIZ.CAPTURE.EXPORT`)

## Requirements

- **REQ-VIZ.CAPTURE.TRACK.REPLAY-001** For an output time t, the replay shall use the document state after the last Record Journal entry whose monotonic timestamp is at or before t, and the latest track keyframe at or before t.
- **REQ-VIZ.CAPTURE.TRACK.REPLAY-002** `capture.read_track@1` shall return the canonical ViewState reconstructed from the track for a viewport and time, byte-identical to what the sampler read at that keyframe.
- **REQ-VIZ.CAPTURE.TRACK.REPLAY-003** `capture.render_replay@1` shall render frames at a requested rate of 1 to 30 fps over a range of at most 1 h, store them as a content-addressed sequence with a manifest listing each frame's time, entry hash, keyframe and digest, and journal the manifest digest.
- **REQ-VIZ.CAPTURE.TRACK.REPLAY-004** Each frame's caption shall be the localised title of the command of the current entry, rendered as plain text, and the manifest shall record the command ID rather than the text.
- **REQ-VIZ.CAPTURE.TRACK.REPLAY-005** If the range has no view track, then both commands shall fail with `NoTrack`; if `REC.POLICY.check(ViewportVideo)` forbids recording, then `capture.render_replay@1` shall fail with `PolicyForbidden` and journal nothing.
- **REQ-VIZ.CAPTURE.TRACK.REPLAY-006** Rendering the same range twice on the same adapter shall give identical frame digests.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/viz/capture/replay-session` (journal with 20 commands and a 30 s view track on `fixtures/viz/golden-cube`), when `fid capture read-track --viewport main --at <t> --json` runs at 10 keyframe times, then each ViewState equals the logged one byte for byte.
- **AC-2** Given the same fixture, when `fid capture render-replay --from <t0> --to <t0+10s> --fps 5 --json` runs, then 50 frames are stored, the manifest lists for each frame the entry hash expected by REQ-001, and one `capture.render_replay@1` entry holds the manifest digest.
- **AC-3** Given AC-2 run twice on the same adapter, then frame digests are identical; across adapters, frames match within the VIZ.RENDER golden tolerance.
- **AC-4** (error path) Given a range before the recording started, then `NoTrack`; given policy `forbidden`, then `PolicyForbidden` with nothing journaled.
- **AC-5** Given a command title containing markup characters, then the caption shows them literally.

## Design

- **Registry command(s):**
  - `capture.read_track@1` R0; params `{ recording_id, viewport, at_mono_ns }`; result `{ view_state, keyframe_mono_ns }`.
  - `capture.render_replay@1` W2; params `{ recording_id, viewport, from_mono_ns, to_mono_ns, fps: 1..=30, width, height (16..=4096) }`; result `{ manifest_digest, frame_count }`; long-running job.
- **Data structures:** `ReplayManifest { frames: [{ t, entry, keyframe, digest, caption_command }] }` in canonical JSON.
- **Algorithm:** index the track's keyframes and the journal's entry timestamps; for each output time, binary-search both, ask `CORE.RPL.RECON.FOLD` for the state at the entry (reusing it across frames until the entry changes), render with `fid-render::headless`, draw the caption overlay, store.
- **Errors:** `NoTrack`, `PolicyForbidden`, `RangeTooLong`, `UnknownViewport`, `cancelled`.
- **Determinism:** D1 (same adapter bit-exact; frames are illustrations bound by manifest).
- **Performance budget:** 1080p replay frames at ≥ 5 fps headless on the reference profile [unverified].
- **Crate:** `fid-capture` (module `track::replay`).

## Test fixtures

`fixtures/viz/capture/replay-session`: generated by a committed script; CC-BY-4.0.

## Build prompt

> In crate `fid-capture`, add module `track::replay` with `capture.read_track@1` (R0; CLI `fid capture read-track`, MCP `capture_read_track`) and `capture.render_replay@1` (W2, long-running job; CLI `fid capture render-replay`, MCP `capture_render_replay`). The view track drives the clock: for each output time, take the last Record Journal entry at or before it and the latest track keyframe at or before it; reconstruct document state with the replay engine's fold (`CORE.RPL.RECON.FOLD`), apply the keyframe's ViewState, render headlessly, draw the localised command title as a plain-text caption, store frames in the CAS with a canonical `ReplayManifest`, journal its digest. Fail with `NoTrack`, `PolicyForbidden` (recording policy), `RangeTooLong` (> 1 h). Tests AC-1..AC-5. Rules: no panics or `unsafe`; deny unwrap/expect/panic/todo/unimplemented/unreachable; frames are illustrations, never values of record.

## Open questions

- Settles the parent's question: TRACK drives the replay clock; CORE.RPL only reconstructs state at an entry.

## Children

_Leaf: none._
