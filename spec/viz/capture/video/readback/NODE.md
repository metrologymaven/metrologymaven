---
id: VIZ.CAPTURE.VIDEO.READBACK
title: Viewport frame readback and capture-rate control
level: 4
parent: VIZ.CAPTURE.VIDEO
kind: task
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-VIZ.CAPTURE.VIDEO-001, REQ-VIZ.CAPTURE.VIDEO-004, REQ-VIZ.CAPTURE.VIDEO-005]
covers: []
depends_on: [VIZ.CAPTURE.VIDEO.PIPE, VIZ.CAPTURE.REC.SESSION, VIZ.RENDER.FRAME]
uses_artifacts: []
side_effect: R0
determinism: N
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# VIZ.CAPTURE.VIDEO.READBACK: Viewport frame readback and capture-rate control

## Purpose

Copies the frames the viewport actually presented into CPU memory without stalling rendering: a ring of GPU staging buffers mapped asynchronously, each frame stamped with the journal monotonic time at presentation, then pushed into the segment pipeline. When readback or encoding cannot keep up, it lowers the capture rate and records the drop instead of slowing the viewport.

## Scope

**In:** the `viewport_video` `SessionObserver`; per-presented-frame copy into one of 3 staging buffers; async map and hand-off; presentation timestamps from the journal monotonic clock; capture-rate ladder 30 → 15 → 10 → 5 fps; drop intervals in `FrameMeta`; overhead measurement hook.
**Out:** frame graph and presentation (`VIZ.RENDER.FRAME`); encoding and segments (`VIZ.CAPTURE.VIDEO.ENCODE`, `VIZ.CAPTURE.VIDEO.PIPE`); OS window capture (`VIZ.CAPTURE.OSCAP`).

## Requirements

- **REQ-VIZ.CAPTURE.VIDEO.READBACK-001** Each captured frame shall carry the journal monotonic timestamp taken when that frame was submitted for presentation.
- **REQ-VIZ.CAPTURE.VIDEO.READBACK-002** Readback shall use at most 3 staging buffers per viewport and shall never wait on the GPU from the render thread.
- **REQ-VIZ.CAPTURE.VIDEO.READBACK-003** If no staging buffer is free or the pipeline returns `Busy`, then the frame shall be skipped and the skip recorded as a dropped interval in the next pushed frame's metadata.
- **REQ-VIZ.CAPTURE.VIDEO.READBACK-004** When more than 10 % of frames are skipped over a 2 s window, the capture rate shall step down one level of 30, 15, 10, 5 fps, and step back up after 10 s with no skips.
- **REQ-VIZ.CAPTURE.VIDEO.READBACK-005** While capturing 1920x1080 at 30 fps, the viewport's frame rate shall drop by no more than 10 % against the same scene without capture on the reference profile [unverified].

## Acceptance criteria

- **AC-1** Given fixture `fixtures/viz/golden-cube` animated by a scripted camera for 5 s and a fake pipeline, then every pushed frame's timestamp equals the presentation timestamp logged by the frame graph and timestamps strictly increase.
- **AC-2** Given a fake pipeline that returns `Busy` for 1 s, then skipped frames are reported as one drop interval covering that second, and the render thread never blocked (frame-time trace shows no wait on map).
- **AC-3** Given a sustained 50 % skip rate, then the capture rate steps to 15 fps within 2 s, and returns to 30 fps 10 s after skips stop.
- **AC-4** Given the benchmark scene on the reference profile, then the viewport fps with capture is at least 90 % of the fps without (nightly; informative until measured).

## Design

- **Registry command(s):** none; started and stopped by the recording session.
- **Data structures:** `StagingRing { buffers: [StagingBuffer; 3], in_flight }`; `RateLadder { level, window_stats }`.
- **Algorithm:** after presentation, if the rate admits this frame and a buffer is free, encode a copy of the presented texture into the buffer and request an async map; on map completion (polled on a capture thread), convert to `TimedFrame` and `push`; track skips for the ladder.
- **Errors:** `DeviceLost` (stops the source; the session records a stop reason; recovery is `VIZ.RENDER.RECOVER`'s).
- **Determinism:** N (timing).
- **Performance budget:** REQ-005 [unverified].
- **Crate:** `fid-capture-video` (module `readback`), using the frame-graph post-present hook of `fid-render`.

## Test fixtures

`fixtures/viz/golden-cube` (shared); fake pipeline in the test crate.

## Build prompt

> In crate `fid-capture-video`, add module `readback`: a `SessionObserver` for source `viewport_video` that, after each presented viewport frame, copies the presented texture into one of 3 staging buffers and maps it asynchronously (never waiting on the GPU from the render thread), stamps the journal monotonic presentation time, and pushes `TimedFrame` into `pipe::VideoPipeline`. Skip a frame when no buffer is free or the pipeline is `Busy`, and report skipped intervals in the next frame's `FrameMeta`. Step the capture rate down 30→15→10→5 fps when > 10 % of frames skip over 2 s, back up after 10 s clean. Tests AC-1..AC-4. Rules: no panics or `unsafe`; deny unwrap/expect/panic/todo/unimplemented/unreachable; pixels are illustrations, never values of record (C2).

## Open questions

- `VIZ.RENDER.FRAME` (open) must offer a post-present hook with the presentation timestamp; requested here.

## Children

_Leaf: none._
