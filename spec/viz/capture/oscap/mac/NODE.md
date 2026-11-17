---
id: VIZ.CAPTURE.OSCAP.MAC
title: macOS window-capture backend
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

# VIZ.CAPTURE.OSCAP.MAC: macOS window-capture backend

## Purpose

Implements `WindowCaptureBackend` on macOS with the system screen-capture framework filtered to a single window of the app (ScreenCaptureKit with a content filter for one window [unverified API details]). macOS requires the user to grant screen-recording permission; a denial maps to `PermissionDenied`, and the source layer falls back to viewport-only capture.

## Scope

**In:** single-window content filter from an `OwnWindowHandle`; stream configuration; frame copy to RGBA8; OS frame times; permission probe and denial mapping.
**Out:** the trait, fallback and redaction (`VIZ.CAPTURE.OSCAP.SRC`); OS support tiers and entitlements in packaging (`PLAT`, `ECO.REL`).

## Requirements

- **REQ-VIZ.CAPTURE.OSCAP.MAC-001** The backend shall build its content filter from exactly one window belonging to the app process and shall never use a display-wide filter.
- **REQ-VIZ.CAPTURE.OSCAP.MAC-002** When screen-recording permission has not been granted, `probe` shall return `NeedsPermission` and `open` shall return `PermissionDenied` without blocking.
- **REQ-VIZ.CAPTURE.OSCAP.MAC-003** Frames shall be delivered as RGBA8 at the window's backing size with the OS presentation timestamp.

## Acceptance criteria

- **AC-1** Given a macOS CI runner with permission pre-granted [unverified feasibility on hosted runners], when a test window is captured for 3 s, then frames match the window's own render within tolerance.
- **AC-2** Given a foreign window overlapping the app window, then no frame contains its unique colour.
- **AC-3** (error path) Given permission not granted, then `open` returns `PermissionDenied` within 1 s.

## Design

- **Registry command(s):** none.
- **Algorithm:** enumerate shareable content, select the window whose ID equals the handle's, create a single-window filter, start a stream, convert sample buffers to RGBA8.
- **Errors:** `NeedsPermission`, `PermissionDenied`, `Unsupported`, `BackendFailed`.
- **Determinism:** N.
- **Performance budget:** 1080p at 30 fps with < 5 % CPU [unverified].
- **Crate:** `fid-capture-os-mac` (named `unsafe` exception for OS bindings).

## Test fixtures

macOS CI runner with a test window; foreign test window binary.

## Build prompt

> Create crate `fid-capture-os-mac` implementing `fid_capture_os::WindowCaptureBackend` with the macOS screen-capture framework restricted to a single window of the app (never display-wide), RGBA8 frames with presentation timestamps, `NeedsPermission`/`PermissionDenied` when screen-recording permission is missing. Tests AC-1..AC-3. Rules: `unsafe` only in the bindings module under a named exception; no panics; deny unwrap/expect/panic/todo/unimplemented/unreachable.

## Open questions

- API names, minimum macOS version and CI feasibility are [unverified].

## Children

_Leaf: none._
