---
id: VIZ.CAPTURE.OSCAP.WIN
title: Windows window-capture backend
level: 4
parent: VIZ.CAPTURE.OSCAP
kind: task
status: leaf
phase: P2
size: M
owners: []
claimed_by:
satisfies: [REQ-VIZ.CAPTURE.OSCAP-002]
covers: []
depends_on: [VIZ.CAPTURE.OSCAP.SRC]
uses_artifacts: []
side_effect: R0
determinism: N
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# VIZ.CAPTURE.OSCAP.WIN: Windows window-capture backend

## Purpose

Implements `WindowCaptureBackend` on Windows 10 and 11 using the system's per-window graphics capture API (Windows.Graphics.Capture with a capture item created for one window handle [unverified API details]), delivering frames of exactly one app window, with the OS capture border left as the OS draws it.

## Scope

**In:** capture item creation from an `OwnWindowHandle`; frame pool and copy to RGBA8; OS frame times; availability probe (OS build check); error mapping.
**Out:** the trait, fallback and redaction (`VIZ.CAPTURE.OSCAP.SRC`); OS support tiers (`PLAT`).

## Requirements

- **REQ-VIZ.CAPTURE.OSCAP.WIN-001** The backend shall create capture items only for the window handle inside an `OwnWindowHandle` and shall not call any display- or monitor-capture API.
- **REQ-VIZ.CAPTURE.OSCAP.WIN-002** When the OS build lacks per-window capture, `probe` shall return `Unsupported` without error dialogs.
- **REQ-VIZ.CAPTURE.OSCAP.WIN-003** Frames shall be delivered as RGBA8 at the window's client size with the OS frame timestamp.
- **REQ-VIZ.CAPTURE.OSCAP.WIN-004** If the window is resized, then the backend shall recreate the frame pool and report a size change so the pipeline starts a new segment.

## Acceptance criteria

- **AC-1** Given a Windows CI runner with a test window of the app, when captured for 3 s, then frames arrive at the client size and their content matches the window's own render within the VIZ.RENDER tolerance.
- **AC-2** Given a second, foreign test window placed on top, then no captured frame contains its unique colour.
- **AC-3** Given a resize mid-capture, then the backend reports the new size and frames continue.
- **AC-4** Given a symbol scan of the crate, then no display-capture API symbol is referenced.

## Design

- **Registry command(s):** none.
- **Data structures:** backend state per stream (capture item, frame pool, D3D device shared with or separate from wgpu [unverified]).
- **Algorithm:** create item for HWND → frame pool → on frame arrived, copy to a staging texture → map → RGBA8 → hand to SRC.
- **Errors:** `Unsupported`, `PermissionDenied`, `BackendFailed`.
- **Determinism:** N.
- **Performance budget:** 1080p at 30 fps with < 5 % CPU on the reference profile [unverified].
- **Crate:** `fid-capture-os-win` (named `unsafe` exception for OS bindings).

## Test fixtures

Windows CI runner with a test window; foreign test window binary in the test crate.

## Build prompt

> Create crate `fid-capture-os-win` implementing `fid_capture_os::WindowCaptureBackend` for Windows 10/11 with the per-window graphics capture API (one capture item per `OwnWindowHandle` window, never a monitor), RGBA8 frames at client size with OS timestamps, frame-pool recreation on resize, `Unsupported` on older builds. Tests AC-1..AC-4 on a Windows CI runner. Rules: `unsafe` only in the bindings module under a named exception; no panics; deny unwrap/expect/panic/todo/unimplemented/unreachable.

## Open questions

- API names and minimum OS build are [unverified] and must be checked against current platform documentation.

## Children

_Leaf: none._
