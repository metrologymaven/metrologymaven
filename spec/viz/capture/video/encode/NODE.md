---
id: VIZ.CAPTURE.VIDEO.ENCODE
title: AV1 encoder and WebM segment muxer
level: 4
parent: VIZ.CAPTURE.VIDEO
kind: task
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-VIZ.CAPTURE.VIDEO-004]
covers: []
depends_on: []
uses_artifacts: [ECO.RULES.SPDX]
side_effect: R0
determinism: N
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# VIZ.CAPTURE.VIDEO.ENCODE: AV1 encoder and WebM segment muxer

## Purpose

Turns timestamped RGBA frames into one self-contained video segment file. The codec is **AV1**, encoded in software by the `rav1e` crate (BSD-2-Clause [unverified], which is on the ECO.RULES.SPDX allowlist) and written into a **WebM** container (the Matroska subset described by IETF RFC 9559) by a small clean-room muxer in this crate. AV1 is offered royalty-free by its patent pool [unverified], which keeps the dependency compatible with D-001 (MIT OR Apache-2.0 code, no copyleft or royalty-bearing dependency). An optional hardware encoder can sit behind the same trait later; it is not required for this leaf.

## Scope

**In:**
- `trait SegmentEncoder { fn begin(params) ; fn push(frame) ; fn finish() -> EncodedSegment }`
- RGBA8 to YUV 4:2:0 (BT.709, limited range) conversion with fixed integer arithmetic
- `rav1e` configuration presets: `realtime` (speed 10, low-latency) and `quality` (speed 6) [unverified speed values]
- WebM writer: EBML header, one video track, SimpleBlocks with millisecond timecodes relative to the segment start, Cues for the first frame, no wall-clock date element
- Encoder identity string (crate name, version, preset) for the segment manifest

**Out** (and which node owns it instead):
- Segment cutting, storage and journal attachment (`VIZ.CAPTURE.VIDEO.PIPE`)
- GPU readback (`VIZ.CAPTURE.VIDEO.READBACK`); OS window frames (`VIZ.CAPTURE.OSCAP.SRC`)
- Hardware encoders through OS media APIs (later; see Open questions)

## Requirements

- **REQ-VIZ.CAPTURE.VIDEO.ENCODE-001** The encoder shall produce AV1 bitstreams in a WebM container, and every produced segment shall start with a key frame so it decodes without any other segment.
- **REQ-VIZ.CAPTURE.VIDEO.ENCODE-002** Every dependency of the encoder crate shall carry a licence on the ECO.RULES.SPDX allowlist, and the encoder shall not link any library under a copyleft or royalty-bearing licence.
- **REQ-VIZ.CAPTURE.VIDEO.ENCODE-003** Each frame's block timecode shall equal its journal monotonic timestamp minus the segment's first frame timestamp, rounded down to whole milliseconds.
- **REQ-VIZ.CAPTURE.VIDEO.ENCODE-004** The WebM output shall contain no wall-clock date, host name, user name or path.
- **REQ-VIZ.CAPTURE.VIDEO.ENCODE-005** With the `realtime` preset, encoding 1920x1080 frames shall sustain at least 30 frames per second on the reference profile using at most half of its logical cores [unverified].
- **REQ-VIZ.CAPTURE.VIDEO.ENCODE-006** If the input frame size differs from the size given at `begin`, then `push` shall fail with `FrameSizeChanged` and the segment shall be finished at the previous frame.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/viz/capture/frames-300` (300 synthetic 1280x720 RGBA frames at 33.333 ms spacing, seed 7), when encoded as one segment, then an independent decoder (the `dav1d` CLI in CI only [unverified availability]) decodes 300 frames and the mean PSNR against the inputs is at least 35 dB with the `quality` preset.
- **AC-2** Given the AC-1 output, then a WebM structure check finds one video track with codec ID `V_AV1`, a key frame first, block timecodes equal to the expected milliseconds, and no `DateUTC`, title or path elements.
- **AC-3** Given the CI licence check (ECO.CI.LIC), then every crate in the encoder's dependency closure is on the allowlist.
- **AC-4** (error path) Given frame 10 at a different size, then `push` returns `FrameSizeChanged` and the finished segment holds frames 0..9.
- **AC-5** Given the benchmark `bench_realtime_1080p` on the reference profile, then throughput is reported and compared with REQ-005 (nightly, informative until measured).

## Design

- **Registry command(s):** none; library.
- **Data structures:** `EncodeParams { width, height, preset: Realtime|Quality, fps_hint }`; `TimedFrame { rgba: Arc<[u8]>, width, height, mono_ns: u64 }`; `EncodedSegment { webm: Vec<u8> or temp-file handle, frame_count, first_mono_ns, last_mono_ns, encoder_id }`.
- **Algorithm:** colour conversion with integer coefficients (no floating point, so the YUV planes are D0 for a given input); `rav1e` context per segment with key-frame interval at least the segment length; muxer writes EBML elements with sizes known after finish (buffered cluster per 1 s).
- **Errors:** `FrameSizeChanged`, `EncoderFailed { detail }`, `TooLarge` (segment above 2 GiB).
- **Determinism:** N (encoder threading may change bytes; segments are evidence of what was shown, bound by digest, not recomputed).
- **Performance budget:** REQ-005 [unverified].
- **Crate:** `fid-capture-video` (module `encode`), separate from `fid-capture` so the encoder's dependency closure is isolated.

## Test fixtures

`fixtures/viz/capture/frames-300`: generated in the test from a seeded pattern generator (seed 7); no external data.

## Build prompt

> Create crate `fid-capture-video` with module `encode`. Implement `SegmentEncoder` producing AV1 in WebM: convert RGBA8 to YUV 4:2:0 (BT.709 limited range) with integer arithmetic, encode with the `rav1e` crate (presets `realtime` and `quality`; key frame first), and write a minimal clean-room WebM (Matroska subset per IETF RFC 9559): EBML header, one `V_AV1` track, SimpleBlocks with millisecond timecodes = (frame monotonic ns − first frame ns) / 1e6 rounded down, Cues for the first key frame, and no date, title, host, user or path. Refuse frame-size changes with `FrameSizeChanged`. Report an encoder ID string. Tests AC-1..AC-5 with a seeded synthetic frame generator. Rules: dependencies only under licences on the project SPDX allowlist (D-001; no GPL/LGPL, no royalty-bearing codecs such as H.264/H.265); no panics or `unsafe` in this crate (named exceptions only if `rav1e` requires none on our side); deny unwrap/expect/panic/todo/unimplemented/unreachable.

## Open questions

- Codec chosen here under D-001's policy rather than by a new decision: AV1 + WebM with a permissive Rust encoder. Licence of `rav1e` (BSD-2-Clause) and the royalty-free status of AV1 are [unverified] and should be confirmed by ECO.CI.LIC and counsel's general dependency review; if either fails, a decision is needed.
- Hardware AV1 encoding through OS media APIs (and its availability per GPU) is a later addition behind `SegmentEncoder`; REQ-005 may not hold on the minimum reference machine with software encoding alone, in which case `VIZ.CAPTURE.VIDEO.READBACK` lowers the capture rate.

## Children

_Leaf: none._
