---
id: VIZ.CAPTURE.OSCAP.SRC
title: Window frame source contract, permission fallback and redaction
level: 4
parent: VIZ.CAPTURE.OSCAP
kind: task
status: leaf
phase: P2
size: M
owners: []
claimed_by:
satisfies: [REQ-VIZ.CAPTURE.OSCAP-001, REQ-VIZ.CAPTURE.OSCAP-002, REQ-VIZ.CAPTURE.OSCAP-003, REQ-VIZ.CAPTURE.OSCAP-004]
covers: []
depends_on: [VIZ.CAPTURE.VIDEO.PIPE, VIZ.CAPTURE.REC.SESSION, VIZ.CAPTURE.REC.POLICY, VIZ.CAPTURE.REC.REDACT]
uses_artifacts: []
side_effect: R0
determinism: N
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# VIZ.CAPTURE.OSCAP.SRC: Window frame source contract, permission fallback and redaction

## Purpose

The OS-independent half of full-window capture: the `WindowCaptureBackend` trait the per-OS backends implement, the `window_video` session observer that drives one backend per app window, the rule that only the app's own windows are ever targeted, the fallback to viewport-only capture when the OS denies permission, redaction of marked panels before frames reach the pipeline, and journal-clock timestamps.

## Scope

**In:** `trait WindowCaptureBackend { fn probe() -> Availability; fn open(own_window: OwnWindowHandle) -> Result<Stream, CaptureError>; }`; `OwnWindowHandle` (constructible only from the app's own window registry); the `window_video` observer; timestamp mapping from OS frame times to the journal monotonic clock; redaction through `VIZ.CAPTURE.REC.REDACT`; fallback and reporting.
**Out:** per-OS APIs (`VIZ.CAPTURE.OSCAP.WIN`, `VIZ.CAPTURE.OSCAP.MAC`, `VIZ.CAPTURE.OSCAP.LINUX`); segments (`VIZ.CAPTURE.VIDEO.PIPE`); policy (`VIZ.CAPTURE.REC.POLICY`); OS support tiers (`PLAT`).

## Requirements

- **REQ-VIZ.CAPTURE.OSCAP.SRC-001** Every window frame pushed to the pipeline shall carry a journal monotonic timestamp obtained by mapping the OS frame time through an offset measured at stream open and re-measured every 10 s, and timestamps shall strictly increase.
- **REQ-VIZ.CAPTURE.OSCAP.SRC-002** Backends shall be opened only with an `OwnWindowHandle`, which the type system allows to be created only from the app's own window registry, so no code path can target another application's window or a whole display.
- **REQ-VIZ.CAPTURE.OSCAP.SRC-003** If a backend reports permission denied or unavailable, then the observer shall report `WindowCaptureDenied` to the recording session, which records it, and the recording shall continue with its other sources.
- **REQ-VIZ.CAPTURE.OSCAP.SRC-004** Every window frame shall pass through `REC.REDACT.apply_mask` with the layout snapshot nearest its timestamp before it is pushed to the pipeline.
- **REQ-VIZ.CAPTURE.OSCAP.SRC-005** If `REC.POLICY.check(WindowVideo)` forbids window capture, then the observer shall open no backend.

## Acceptance criteria

- **AC-1** Given a fake backend producing 300 frames with OS timestamps and a known clock offset, then pushed frames carry the mapped journal timestamps within 1 ms and strictly increase.
- **AC-2** Given the crate's public API, then a compile-fail test shows `OwnWindowHandle` cannot be built from a raw OS handle.
- **AC-3** (error path) Given a fake backend returning `PermissionDenied`, then the session records `WindowCaptureDenied`, viewport video continues and no window frames are pushed.
- **AC-4** Given fixture `fixtures/viz/capture/redact-layout.json` and a fake backend, then no pushed frame contains a marked panel's pixels.
- **AC-5** Given policy `capture.window_capture = forbidden`, then no backend `open` is called.

## Design

- **Registry command(s):** none; enabled by `capture.start_recording@1` with source `window_video`.
- **Data structures:** `OwnWindowHandle` (private constructor in the shell window registry module); `Availability { Available, NeedsPermission, Unsupported }`; `ClockMap { os_to_mono_offset_ns, measured_at }`.
- **Algorithm:** per window, open a stream; on each OS frame, map time, mask, convert to `TimedFrame`, push; on `Busy`, drop and report the interval.
- **Errors:** `WindowCaptureDenied`, `Unsupported`, `BackendFailed { detail }`.
- **Determinism:** N.
- **Performance budget:** per-frame overhead excluding OS copy < 2 ms at 1080p [unverified until measured].
- **Crate:** `fid-capture-os` (module `src`).

## Test fixtures

Fake backend in the test crate; `fixtures/viz/capture/redact-layout.json` (shared with REC.REDACT).

## Build prompt

> Create crate `fid-capture-os` with module `src`: trait `WindowCaptureBackend` (probe, open with an `OwnWindowHandle` that can only be constructed from the app's own window registry), and a `window_video` `SessionObserver` that opens one stream per app window when policy allows (`capture.window_capture`), maps OS frame times to the journal monotonic clock (offset measured at open and every 10 s; strictly increasing), masks redacted panels with `rec::redact::apply_mask` before pushing to `VideoPipeline`, and reports `WindowCaptureDenied` to the session while the recording continues with its other sources. Tests AC-1..AC-5 with a fake backend, including a compile-fail test for the handle. Rules: never capture other applications or whole displays; no panics; `unsafe` only in per-OS backend crates under named exceptions; deny unwrap/expect/panic/todo/unimplemented/unreachable.

## Open questions

- None blocking.

## Children

_Leaf: none._
