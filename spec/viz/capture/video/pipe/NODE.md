---
id: VIZ.CAPTURE.VIDEO.PIPE
title: Segment pipeline, storage and journal attachment
level: 4
parent: VIZ.CAPTURE.VIDEO
kind: task
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-VIZ.CAPTURE.VIDEO-001, REQ-VIZ.CAPTURE.VIDEO-002]
covers: []
depends_on: [VIZ.CAPTURE.VIDEO.ENCODE, VIZ.CAPTURE.REC.SESSION, CORE.STO.CAS, CORE.JRN.ENTRY, CORE.REG.DEF]
uses_artifacts: []
side_effect: W2
determinism: N
cli: []
mcp: []
journal: [capture.attach_segment@1]
updated: 2026-11-17
---

# VIZ.CAPTURE.VIDEO.PIPE: Segment pipeline, storage and journal attachment

## Purpose

The common path every video frame source feeds (viewport readback now, OS window capture later): accept timestamped frames, cut them into segments of 10 s ± 1 s, encode each with `VIZ.CAPTURE.VIDEO.ENCODE`, store the segment file and its manifest in the project content-addressed store, and attach each segment to the journal with one internal W2 entry carrying both digests. Because the entry is in the hash chain, a later change to the stored segment is detectable.

## Scope

**In:**
- `VideoPipeline` with `push(TimedFrame, FrameMeta)` and a bounded input queue (depth 8)
- Segment cutting at the first frame at or after 10 s from the segment start, and at stop
- `SegmentManifest` (canonical JSON): recording ID, sequence number, source kind, width, height, frame count, first and last monotonic timestamps, per-frame monotonic timestamps, dropped-frame intervals, redaction counts, encoder ID
- `capture.attach_segment@1` (W2, exposure `internal`: written by the pipeline, never invoked from a surface)
- Reporting attached digests to the recording session (`SessionObserver::segment_attached`)

**Out** (and which node owns it instead):
- Encoding (`VIZ.CAPTURE.VIDEO.ENCODE`); frame readback and rate control (`VIZ.CAPTURE.VIDEO.READBACK`)
- Verification (`VIZ.CAPTURE.VIDEO.VERIFY`); session and policy (`VIZ.CAPTURE.REC.SESSION`, `VIZ.CAPTURE.REC.POLICY`)
- Export (`VIZ.CAPTURE.EXPORT`)

## Requirements

- **REQ-VIZ.CAPTURE.VIDEO.PIPE-001** The pipeline shall accept only frames whose timestamp is a journal monotonic timestamp later than the previous frame's, and shall reject others with `NonMonotonicFrame` without stopping the recording.
- **REQ-VIZ.CAPTURE.VIDEO.PIPE-002** The pipeline shall close a segment at the first frame whose timestamp is at least 10 s after the segment's first frame, so every segment except the last spans 10 s ± 1 s at frame rates of 1 fps or more.
- **REQ-VIZ.CAPTURE.VIDEO.PIPE-003** Each segment file and its manifest shall be stored in the project content-addressed store, and one `capture.attach_segment@1` entry shall carry the segment's SHA-256, the manifest's SHA-256, the recording ID and the sequence number.
- **REQ-VIZ.CAPTURE.VIDEO.PIPE-004** The manifest shall list the monotonic timestamp of every encoded frame and every dropped-frame interval reported by the source.
- **REQ-VIZ.CAPTURE.VIDEO.PIPE-005** `capture.attach_segment@1` shall have exposure `internal` and no CLI or MCP projection.
- **REQ-VIZ.CAPTURE.VIDEO.PIPE-006** When the recording session stops, the pipeline shall finish and attach the open segment before acknowledging the flush.
- **REQ-VIZ.CAPTURE.VIDEO.PIPE-007** If the input queue is full, then `push` shall return `Busy` immediately instead of blocking the caller.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/viz/capture/frames-25s` (750 synthetic frames at 30 fps with monotonic timestamps), when pushed and the session stopped, then three segments are attached (10 s, 10 s, 5 s ± one frame), each entry's digest equals the SHA-256 of the stored blob, and sequence numbers are 0, 1, 2.
- **AC-2** Given the AC-1 manifests, then each lists exactly its frames' timestamps, and the union of all manifests covers all 750 timestamps once.
- **AC-3** (error path) Given a frame with a timestamp equal to its predecessor, then `push` returns `NonMonotonicFrame` and the recording continues.
- **AC-4** Given a source that reports a 2 s drop interval, then the manifest of that segment lists the interval.
- **AC-5** Given the generated surfaces, then no CLI command or MCP tool exists for `capture.attach_segment@1`.
- **AC-6** Given a test encoder that blocks, when 9 frames are pushed, then the ninth returns `Busy` within 1 ms.

## Design

- **Registry command(s):** `capture.attach_segment@1`, W2, exposure `internal`, D0 for the entry; params `{ recording_id, seq: u32, segment: Digest, manifest: Digest, source: Viewport|Window }`.
- **Data structures:** `FrameMeta { source, dropped_before: Option<(u64, u64)>, redactions: u16 }`; `SegmentManifest` serialised with `fid-canon` (RFC 8785).
- **Algorithm:** worker thread drains the queue → encoder → on cut, finish encoder, put blob and manifest durably in CAS, commit the attach entry through the bus as the recording's principal, notify the session.
- **Errors:** `NonMonotonicFrame`, `Busy`, `StorageFull` (stops the recording with a journaled stop reason through REC.SESSION).
- **Determinism:** N for segment bytes (encoder); the manifest is D0 for a given frame sequence.
- **Performance budget:** pipeline overhead excluding encoding < 2 ms per frame [unverified until measured].
- **Crate:** `fid-capture-video` (module `pipe`).

## Test fixtures

`fixtures/viz/capture/frames-25s`: generated in the test (seeded); a blocking fake encoder in the test crate.

## Build prompt

> In crate `fid-capture-video`, add module `pipe`. Implement `VideoPipeline::push(TimedFrame, FrameMeta) -> Result<(), PushError>` with a bounded queue of 8 (`Busy` when full, never block), rejecting non-increasing journal monotonic timestamps (`NonMonotonicFrame`). A worker encodes frames with `encode::SegmentEncoder`, cuts a segment at the first frame ≥ 10 s after the segment start and at session stop, stores the WebM and a canonical-JSON `SegmentManifest` (recording ID, seq, source, size, frame count, every frame timestamp, drop intervals, redaction counts, encoder ID) durably in the project CAS, and commits `capture.attach_segment@1` (W2, exposure internal, no CLI/MCP) carrying segment and manifest SHA-256, recording ID and seq; then report the digest to the recording session. Flush on stop. Tests AC-1..AC-6. Rules: no panics or `unsafe`; deny unwrap/expect/panic/todo/unimplemented/unreachable; media stays in the local store (C7).

## Open questions

- None blocking.

## Children

_Leaf: none._
