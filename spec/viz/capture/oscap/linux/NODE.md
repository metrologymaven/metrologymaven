---
id: VIZ.CAPTURE.OSCAP.LINUX
title: Linux window-capture backend
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

# VIZ.CAPTURE.OSCAP.LINUX: Linux window-capture backend

## Purpose

Implements `WindowCaptureBackend` on Linux. On X11 the backend reads the app's own window pixmap (composite extension) directly. On Wayland, compositors do not let an app read its own window from outside its renderer [unverified for all compositors], and the desktop screencast portal asks the user to pick a source, which cannot be restricted to our window; so on Wayland the backend composes the window itself: it renders the app's own surfaces (panels and viewport) off-screen into one frame, which by construction contains only the app's window.

## Scope

**In:** X11 path (own window ID, composite redirection, frame copy); Wayland path (self-composition of the app's own UI frame into RGBA8 through the shell's render-to-texture hook); session detection; probe.
**Out:** the trait, fallback and redaction (`VIZ.CAPTURE.OSCAP.SRC`); the desktop portal (not used); OS support tiers (`PLAT`).

## Requirements

- **REQ-VIZ.CAPTURE.OSCAP.LINUX-001** On X11 the backend shall capture only the window ID inside the `OwnWindowHandle` and shall never read the root window.
- **REQ-VIZ.CAPTURE.OSCAP.LINUX-002** On Wayland the backend shall produce frames only from the app's own rendered UI and shall not use the screencast portal or any compositor-wide capture protocol.
- **REQ-VIZ.CAPTURE.OSCAP.LINUX-003** If neither path is available, then `probe` shall return `Unsupported`.

## Acceptance criteria

- **AC-1** Given an X11 CI session (Xvfb) with a test window, when captured for 3 s, then frames match the window's own render within tolerance and contain no pixels of an overlapping foreign window.
- **AC-2** Given a headless Wayland compositor in CI [unverified feasibility], then frames equal the app's self-composed UI frame byte for byte.
- **AC-3** Given a symbol scan, then no root-window read or portal call appears.

## Design

- **Registry command(s):** none.
- **Algorithm:** detect session type; X11: redirect the own window, fetch its pixmap per frame on a timer at the capture rate; Wayland: request an RGBA8 copy of each composed UI frame from the shell's render-to-texture hook.
- **Errors:** `Unsupported`, `BackendFailed`.
- **Determinism:** N.
- **Performance budget:** 1080p at 30 fps [unverified].
- **Crate:** `fid-capture-os-linux` (named `unsafe` exception for X11 bindings).

## Test fixtures

Xvfb and a headless compositor in CI; foreign test window binary.

## Build prompt

> Create crate `fid-capture-os-linux` implementing `fid_capture_os::WindowCaptureBackend`: on X11 capture only the app's own window ID through the composite extension (never the root window); on Wayland compose the app's own UI frame through the shell's render-to-texture hook (no screencast portal, no compositor-wide capture); `Unsupported` otherwise. Tests AC-1..AC-3. Rules: `unsafe` only in the X11 bindings module under a named exception; no panics; deny unwrap/expect/panic/todo/unimplemented/unreachable.

## Open questions

- The Wayland self-composition path needs a render-to-texture hook for the whole UI from `VIZ.SHELL.DOCK`; requested here. Compositor behaviour is [unverified].

## Children

_Leaf: none._
