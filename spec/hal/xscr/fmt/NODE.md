---
id: HAL.XSCR.FMT
title: Transcript format, writer and bounded reader
level: 3
parent: HAL.XSCR
kind: task
status: leaf
phase: P2
size: M
owners: []
claimed_by:
satisfies: [REQ-HAL.XSCR-002, REQ-HAL.XSCR-005]
covers: []
depends_on: [HAL.IDL.STAMP]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# HAL.XSCR.FMT: Transcript format, writer and bounded reader

## Purpose

Defines the on-disk instrument I/O transcript (recording layer 3) and provides its append-only writer and a bounded, fail-closed reader. Every other transcript feature (recording, playback, inspection, trimming, export) reads and writes through this one implementation.

## Scope

**In:**
- Container: a header followed by length-prefixed frames, grouped into segments, each segment closed by a SHA-256 digest chained to the previous segment's digest
- Header: format version, session ID, driver identity, negotiated capabilities, timebase anchor
- Entry kinds: `control_request`, `control_response`, `device_event`, `batch_ref` (payload by content-addressed digest, never inline), `heartbeat`, `gap`
- Per-entry fields: entry index, host time (ns), direction, trace ID, canonical JSON body
- Writer: append-only, segment close on size or time, digest output for the provenance stamp's `transcript_segment`
- Reader: bounded frame size and entry count; typed errors

**Out:**
- Deciding what to record and when (owned by `HAL.XSCR.REC`)
- Playback (owned by `HAL.XSCR.PLAY`)
- Inspection, trimming and export commands (owned by `HAL.XSCR.TOOLS`)

## Requirements

- **REQ-HAL.XSCR.FMT-001** Every entry shall carry its entry index, host timestamp in ns, direction, entry kind and the trace ID of the journaled command that caused it (empty only for unsolicited device events and heartbeats).
- **REQ-HAL.XSCR.FMT-002** Batch payloads shall be referenced by content-addressed digest only, never stored inline.
- **REQ-HAL.XSCR.FMT-003** Each segment shall end with a SHA-256 digest over its frames and the previous segment's digest, and the writer shall return that digest for the stamp.
- **REQ-HAL.XSCR.FMT-004** The reader shall reject any frame longer than 16 MiB, any transcript with more than 2^32 entries, and any truncated frame, with typed errors `transcript_frame_too_large`, `transcript_too_many_entries` and `transcript_truncated`, and shall never panic.
- **REQ-HAL.XSCR.FMT-005** If a segment digest does not match its contents, then the reader shall fail with `transcript_digest_mismatch` naming the segment index.
- **REQ-HAL.XSCR.FMT-006** Writing the same entries with the same segment boundaries shall produce byte-identical files on every supported platform.

## Acceptance criteria

- **AC-1** Given `fixtures/hal/xscr/fmt/gauge_session.xscr` (recorded from the gauge simulator, 3 segments), when read, then all entries parse, all segment digests verify, and the last segment digest equals `gauge_session.sha256`.
- **AC-2** Given the entries of AC-1, when re-written with the same segment boundaries on Windows, Linux and macOS, then the files are byte-identical to the fixture.
- **AC-3** (error path) Given `truncated.xscr` (last frame cut), when read, then it fails with `transcript_truncated` at the last frame's index.
- **AC-4** (error path) Given `tampered.xscr` (one byte changed in segment 2), when read, then it fails with `transcript_digest_mismatch` naming segment 2.
- **AC-5** (error path) Given `huge_frame.xscr` declaring a 1 GiB frame, when read, then it fails with `transcript_frame_too_large` without allocating more than 16 MiB.
- **AC-6** A fuzz target over the reader runs for 10 minutes in CI with no crash, hang or allocation above 64 MiB.

## Design

- **Registry commands:** none; library task.
- **Data structures:** `TranscriptHeader`, `Entry { index: u64, t_host_ns: i64, dir: Dir, kind: EntryKind, trace_id: Option<TraceId>, body: CanonicalJson }`, `SegmentDigest`, `Writer`, `Reader` (iterator of `Result<Entry, XscrError>`).
- **Algorithm:** frame = u32 little-endian length + body; segment trailer = digest frame; chain digest = SHA-256(previous digest ‖ frames). Body is RFC 8785 canonical JSON.
- **Errors:** `transcript_frame_too_large`, `transcript_too_many_entries`, `transcript_truncated`, `transcript_digest_mismatch`, `transcript_version_unsupported`.
- **Determinism:** D0 for writing; reading is pure.
- **Performance budget:** write 100 000 entries per second with < 1 % CPU on reference hardware [unverified until measured].
- **Crate:** `fid-hal-xscr`.

## Test fixtures

Transcripts recorded from project simulators under `fixtures/hal/xscr/fmt/` (CC-BY-4.0), plus hand-damaged variants and a `cargo-fuzz` corpus seeded from them.

## Build prompt

> Create crate `fid-hal-xscr` with the transcript format: header (version, session ID, driver identity, negotiated capabilities, timebase anchor), frames (u32 LE length + RFC 8785 canonical JSON body), entries `{index, t_host_ns, dir, kind (control_request|control_response|device_event|batch_ref|heartbeat|gap), trace_id, body}` with batch payloads referenced by digest only, segments closed by a chained SHA-256 digest (SHA-256 of previous digest ‖ frames) returned to the caller. Reader is an iterator that fails closed: frame > 16 MiB → `transcript_frame_too_large`, > 2^32 entries → `transcript_too_many_entries`, cut frame → `transcript_truncated`, bad digest → `transcript_digest_mismatch`, unknown version → `transcript_version_unsupported`; never allocate beyond the frame limit. Writing must be byte-identical across platforms. No panic, unwrap, expect or unsafe (CONSTITUTION §8); add a cargo-fuzz target. Tests: AC-1..AC-6 with fixtures under `fixtures/hal/xscr/fmt/`.

## Open questions

- Transcripts of NDA vendor-SDK hosts may reveal protected protocol detail; the format allows redaction of bodies, but whether such transcripts may leave the machine is a decision for when the first NDA host exists (inherited from the parent).

## Children

_Leaf: none._
