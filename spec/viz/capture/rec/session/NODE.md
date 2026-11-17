---
id: VIZ.CAPTURE.REC.SESSION
title: Recording session start, stop and status
level: 4
parent: VIZ.CAPTURE.REC
kind: feature
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-VIZ.CAPTURE.REC-002, REQ-VIZ.CAPTURE.REC-003, REQ-VIZ.CAPTURE.REC-004, REQ-VIZ.CAPTURE.REC-005]
covers: []
depends_on: [VIZ.CAPTURE.REC.POLICY, CORE.JRN.STREAM.ALOG, CORE.BUS.POL.PROJ, CORE.REG.DEF, CORE.STO.CAS]
uses_artifacts: []
side_effect: [W2, W2, R0]
determinism: D0
cli: [fid capture start-recording, fid capture stop-recording, fid capture get-recording-status]
mcp: [capture_start_recording, capture_stop_recording, capture_get_recording_status]
journal: [capture.start_recording@1, capture.stop_recording@1, capture.get_recording_status@1]
updated: 2026-11-17
---

# VIZ.CAPTURE.REC.SESSION: Recording session start, stop and status

## Purpose

The recording session that the view track and video capture run inside. Start and stop are human-only W2 commands, so an agent's call becomes a pending request that a person approves in the Proposal Inbox (C5). Starting sets the Activity Log recording flag (`CORE.JRN.STREAM.ALOG`), so R0 and R1 calls are logged while recording (D-031). Media written during the session stays in the local content-addressed store; this module has no network code, and the only way media leaves the machine is `egress.send@1` with a media digest.

## Scope

**In:**
- `capture.start_recording@1` (W2, `human_only: true`) with requested sources `view_track`, `viewport_video`, `window_video`
- `capture.stop_recording@1` (W2, `human_only: true`)
- `capture.get_recording_status@1` (R0)
- `RecordingSession` record (ID, sources, start and stop monotonic time, `retain_until`, attached segment digests) and the `SessionObserver` trait that TRACK, VIDEO and OSCAP implement to start and stop their sampling
- Mandated start at app session start, and refusal of stop while mandated
- Setting and clearing the Activity Log recording flag

**Out** (and which node owns it instead):
- Policy keys and evaluation (`VIZ.CAPTURE.REC.POLICY`); indicator (`VIZ.CAPTURE.REC.INDIC`); redaction (`VIZ.CAPTURE.REC.REDACT`)
- Sampling, encoding and segment attachment (`VIZ.CAPTURE.TRACK.SAMPLE`, `VIZ.CAPTURE.VIDEO.PIPE`)
- Pending-request filing and approval (`CORE.BUS.POL.PROJ`); the approval UI (`VIZ.INBOX.ACTIONS.REQS`)
- Local export (`VIZ.CAPTURE.EXPORT`); network egress (`CORE.BUS.POL.EGRESS`)

## Requirements

- **REQ-VIZ.CAPTURE.REC.SESSION-001** `capture.start_recording@1` and `capture.stop_recording@1` shall be declared `human_only: true`, so that an agent call files a pending request through `CORE.BUS.POL.PROJ` and starts or stops nothing until a human approves it.
- **REQ-VIZ.CAPTURE.REC.SESSION-002** If `REC.POLICY.check` returns `Forbid` for any requested source, then start shall fail with `PolicyForbidden` naming the key, start no source and journal nothing.
- **REQ-VIZ.CAPTURE.REC.SESSION-003** When start succeeds, the module shall journal one entry with the recording ID, the sources and the journal monotonic start time, call `set_recording(true, id)` on the Activity Log, and notify every registered `SessionObserver` before returning.
- **REQ-VIZ.CAPTURE.REC.SESSION-004** While policy mandates recording, the app shall start a recording with sources `view_track` and `viewport_video` when a session opens, and `capture.stop_recording@1` shall fail with `PolicyMandated` and journal nothing.
- **REQ-VIZ.CAPTURE.REC.SESSION-005** When stop succeeds, the module shall notify the observers, wait up to 5 s for them to flush their last segment, call `set_recording(false, id)`, and journal one entry listing every segment digest attached during the session.
- **REQ-VIZ.CAPTURE.REC.SESSION-006** If start is called while a recording is active, then it shall fail with `AlreadyRecording`; if stop is called while none is active, then it shall fail with `NotRecording`; neither shall journal anything.
- **REQ-VIZ.CAPTURE.REC.SESSION-007** Recorded media shall be written only to the project content-addressed store, and the module shall contain no call that opens a network connection (checked by the `CORE.BUS.POL.EGRESS` socket-denial harness).
- **REQ-VIZ.CAPTURE.REC.SESSION-008** `capture.get_recording_status@1` shall return whether a recording is active, its ID, sources, start time, segment count and the mandated flag.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/viz/golden-cube` and a GUI human principal, when `fid capture start-recording --sources view_track --json` runs (human-upgraded CLI), then one `capture.start_recording@1` entry is journaled, the Activity Log flag is set (the next R0 call adds one Activity Log line), and a fake observer received `started`.
- **AC-2** (agent) Given an MCP agent call to `capture_start_recording`, then the result is `PendingRequest`, one `request.file_request@1` entry exists, the Activity Log flag stays off and no observer is notified; when a human approves with `approval_of`, then recording starts once.
- **AC-3** (error path) Given policy `capture.recording = forbidden`, when start runs, then it fails with `PolicyForbidden { key: capture.recording }` and journals nothing.
- **AC-4** Given policy `mandated` and a new app session, then a recording is active without a user command; `fid capture stop-recording` fails with `PolicyMandated`.
- **AC-5** Given an active recording with two fake segments attached, when stop runs, then the stop entry lists both digests, the flag is cleared and `get-recording-status` reports inactive.
- **AC-6** Given the net-guard harness, when start, stop and status run, then no socket is opened.
- **AC-7** (replay) Given the journal of AC-1 and AC-5, when replayed in exact mode, then the outcome is `IDENTICAL` for the start and stop entries (media bytes are referenced by digest, not re-recorded).

## Design

- **Registry command(s):**
  - `capture.start_recording@1` W2, `human_only: true` (MCP request-only); params `{ sources: [ViewTrack | ViewportVideo | WindowVideo] (1..=3, no duplicates), viewport?: ViewportId }`; result `{ recording_id, started_mono_ns, retain_until? }`.
  - `capture.stop_recording@1` W2, `human_only: true`; params `{}`; result `{ recording_id, segments: [Digest] }`.
  - `capture.get_recording_status@1` R0; result `RecordingStatus { active, recording_id?, sources, started_mono_ns?, segment_count, mandated }`.
- **Data structures:** `RecordingSession { id: Uuid v7, sources, started_mono_ns, stopped_mono_ns?, retain_until?, segments: Vec<Digest> }` stored as a project entity; `trait SessionObserver { fn started(&self, s: &RecordingSession); fn stopping(&self, id) -> FlushHandle; fn segment_attached(&self, d: Digest); }`.
- **Algorithm:** policy check → state check → journal start → set flag → notify. Stop: mandated check → notify `stopping` and wait for flush handles (5 s cap; late segments are attached with the next status read and logged as `late_segment`) → clear flag → journal stop with the digests reported through `segment_attached`.
- **Errors:** `PolicyForbidden { key }`, `PolicyMandated`, `AlreadyRecording`, `NotRecording`, `SourceUnavailable { source }` (for example window capture without an OS backend).
- **Determinism:** D0 for the state machine and the journaled content; media bytes are produced elsewhere.
- **Performance budget:** start and stop < 50 ms excluding the flush wait [unverified until measured].
- **Crate:** `fid-capture` (module `rec::session`).

## Test fixtures

`fixtures/viz/golden-cube` (shared); policy fixtures from `VIZ.CAPTURE.REC.POLICY`; a fake `SessionObserver` in the test crate.

## Build prompt

> In crate `fid-capture`, add module `rec::session` with three registry commands: `capture.start_recording@1` (W2, `human_only: true`; CLI `fid capture start-recording`, MCP `capture_start_recording` request-only), `capture.stop_recording@1` (W2, `human_only: true`) and `capture.get_recording_status@1` (R0). Start: check `RecordingPolicy::check` for every requested source (`PolicyForbidden`), refuse if already recording, journal one entry (recording ID UUIDv7, sources, journal monotonic start), call `set_recording(true, id)` from `fid-journal::stream`, notify registered `SessionObserver`s. Under a `mandated` policy, start view track and viewport video when an app session opens and refuse stop with `PolicyMandated`. Stop: notify observers, wait up to 5 s for flushes, clear the flag, journal the list of attached segment digests. Agent calls must become pending requests through the bus (`human_only`); never start on an agent's call. Media goes only into the project content-addressed store; no network code (C7, CONSTITUTION §3.1: media leaves only via `egress.send@1`). Tests AC-1..AC-7. Rules: no panics or `unsafe`; deny unwrap/expect/panic/todo/unimplemented/unreachable.

## Open questions

- The mandated start runs under the session's human principal; for headless sessions under a mandate (no human present) the start is recorded with the session's principal kind, and whether a `script` session may run at all under a mandate is for CORE.BUS to say.

## Children

_Leaf: none._
