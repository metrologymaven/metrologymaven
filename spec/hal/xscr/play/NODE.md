---
id: HAL.XSCR.PLAY
title: Transcript playback device and divergence detection
level: 3
parent: HAL.XSCR
kind: feature
status: leaf
phase: P2
size: M
owners: []
claimed_by:
satisfies: [REQ-HAL.XSCR-003, REQ-HAL.XSCR-004]
covers: []
depends_on: [HAL.XSCR.FMT, HAL.ACQ.SESS, HAL.ACQ.INGEST, CORE.STO]
side_effect: I1
determinism: D1
cli: [fid device play-transcript]
mcp: [device_play_transcript]
journal: [device.play_transcript@1]
updated: 2026-11-16
---

# HAL.XSCR.PLAY: Transcript playback device and divergence detection

## Purpose

Turns a recorded transcript into a device. The playback source implements the capabilities recorded in the transcript header, answers each control request with the recorded response, and re-emits the recorded batches from the content-addressed store. Any workflow, test or bug report can then run again without the instrument. If the replayed commands differ from what was recorded, playback stops at the first difference.

## Scope

**In:**
- `device.play_transcript@1` (I1): registers a playback source for a transcript and returns a device handle usable with `device.open_session@1`
- Matching of incoming control requests against recorded requests, ignoring volatile fields (idempotency key, deadline, trace ID)
- Re-emission of recorded batches by digest, as fast as possible or at recorded timing
- Typed divergence error naming the first differing entry
- Registration of the source with kind `playback`, so the runtime stamps `simulated = true`

**Out:**
- Stamping (owned by `HAL.ACQ.STAMP`)
- Presenting a transcript as an I++ server on the wire (owned by `HAL.SIM.REPLAY`)
- Journal replay semantics and outcomes (owned by `CORE.RPL`)

## Requirements

- **REQ-HAL.XSCR.PLAY-001** When `device.play_transcript@1` runs on a valid transcript, the system shall register a playback source declaring the capabilities and versions recorded in the transcript header, with source kind `playback`.
- **REQ-HAL.XSCR.PLAY-002** When the session issues a control request equal to the next recorded request after removing volatile fields, the playback source shall return the recorded response.
- **REQ-HAL.XSCR.PLAY-003** If a control request differs from the next recorded request, then playback shall stop with `transcript_divergence` naming the entry index and the first differing JSON pointer, and shall emit nothing further.
- **REQ-HAL.XSCR.PLAY-004** Batches re-emitted by playback shall have payload digests equal to the recorded digests, and if a payload is missing from the store, playback shall stop with `transcript_payload_missing`.
- **REQ-HAL.XSCR.PLAY-005** The command shall accept `--timing asap|recorded`; with `recorded`, inter-entry delays shall match recorded host-time deltas within 5 ms [unverified until measured].

## Acceptance criteria

- **AC-1** Given `fixtures/hal/xscr/play/linescan_sim.xscr` and its payloads, when `fid device play-transcript --json` runs and the recorded command sequence is replayed, then every batch digest equals the recorded digest and every stamp has `simulated: true`.
- **AC-2** (error path) Given the same transcript and a replayed sequence whose second `configure` uses a different exposure, when replayed, then playback stops with `transcript_divergence` at that entry with pointer `/exposure/value`.
- **AC-3** (error path) Given the transcript with one payload removed from the store, when replayed, then playback stops with `transcript_payload_missing` naming the digest.
- **AC-4** Given `--timing recorded`, when replayed, then each inter-batch delay is within 5 ms of the recorded delta on reference hardware.
- **AC-5** (error path) Given a truncated transcript, when `device.play_transcript@1` runs, then it fails with `transcript_truncated` and registers no source.
- **AC-6** (replay) Given the journal of AC-1, when replayed in exact mode on the same platform, then the outcome is `IDENTICAL`.

## Design

- **Registry command:** `device.play_transcript@1`, class I1 (simulator I/O, exposed on MCP); parameters `transcript: Handle` (approved root handle, never a raw path), `timing: asap|recorded`; result `DeviceHandle`.
- **Data structures:** `PlaybackSource { header, cursor, timing }` implementing the generic capability dispatch; volatile-field list per message type.
- **Algorithm:** canonicalize request, strip volatile fields, compare with recorded canonical bytes; on match return recorded response and advance; batches fetched by digest from the store and pushed through the credit-limited sink.
- **Errors:** `transcript_divergence`, `transcript_payload_missing`, plus reader errors from `HAL.XSCR.FMT`.
- **Determinism:** D1 (same platform, same timing mode); `asap` output is D0 by digest.
- **Performance budget:** `asap` playback at ≥ 2× the recorded LineScan rate [unverified until measured].
- **Crate:** `fid-hal-xscr` (module `play`).

## Test fixtures

Transcripts recorded from the project LineScan and gauge simulators under `fixtures/hal/xscr/play/` with payload objects (CC-BY-4.0).

## Build prompt

> In crate `fid-hal-xscr`, module `play`, implement the playback source and registry command `device.play_transcript@1` (side effect I1; CLI `fid device play-transcript`, MCP `device_play_transcript`; parameters: transcript handle from an approved root, `timing: asap|recorded`; result: device handle). The source declares the capabilities recorded in the header, registers with source kind `playback` (the runtime then stamps `simulated = true`), answers each control request whose canonical JSON minus volatile fields (idempotency key, deadline, trace ID) equals the next recorded request with the recorded response, and re-emits batches by digest from the content-addressed store through the credit-limited sink. Divergence → stop with `transcript_divergence {entry, pointer}`; missing payload → `transcript_payload_missing`. Use the reader from `HAL.XSCR.FMT`. No raw file paths; no panic, unwrap, expect or unsafe (CONSTITUTION §8). Tests: AC-1..AC-6 with fixtures under `fixtures/hal/xscr/play/`.

## Open questions

- Whether a divergence that only changes a non-acquisition-relevant configuration field should be tolerated is deliberately not allowed in v1.

## Children

_Leaf: none._
