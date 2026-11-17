---
id: VIZ.CAPTURE.REC.REDACT
title: Panel redaction marks and frame masking
level: 4
parent: VIZ.CAPTURE.REC
kind: task
status: leaf
phase: P1b
size: S
owners: []
claimed_by:
satisfies: [REQ-VIZ.CAPTURE.REC-006]
covers: []
depends_on: [VIZ.SHELL.DOCK, CORE.STO.SET]
uses_artifacts: []
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# VIZ.CAPTURE.REC.REDACT: Panel redaction marks and frame masking

## Purpose

Decides which panels are blanked in captured frames and blanks them before any frame reaches an encoder. Panels can be marked by their declaration (a panel type declares `redact_in_capture`) or by the user through a setting listing panel type IDs. The mask is computed from the shell's panel rectangles for the frame being captured, so it follows docking and resizing.

## Scope

**In:** the `redact_in_capture` panel-type flag; the user-scope setting `capture.redacted_panels` (list of panel type IDs); `mask_for(window_layout) -> Vec<PixelRect>`; `apply_mask(frame, mask)` filling masked pixels with a fixed opaque fill and a diagonal hatch; a redaction count in frame metadata.
**Out:** window frame sources (`VIZ.CAPTURE.OSCAP.SRC`); panel rectangles (`VIZ.SHELL.DOCK`); viewport-only video, which contains no panels (`VIZ.CAPTURE.VIDEO`).

## Requirements

- **REQ-VIZ.CAPTURE.REC.REDACT-001** A panel shall be redacted when its type declares `redact_in_capture` or its type ID is listed in `capture.redacted_panels`.
- **REQ-VIZ.CAPTURE.REC.REDACT-002** `mask_for` shall return, for a window layout snapshot taken at the frame's capture time, the pixel rectangles of every redacted panel, clipped to the window.
- **REQ-VIZ.CAPTURE.REC.REDACT-003** `apply_mask` shall overwrite every pixel inside the mask before the frame is passed to an encoder, so no original pixel value of a redacted panel remains in the frame buffer handed on.
- **REQ-VIZ.CAPTURE.REC.REDACT-004** If the layout snapshot for a frame is missing, then the whole frame shall be masked.
- **REQ-VIZ.CAPTURE.REC.REDACT-005** The frame metadata shall record the number of redacted rectangles.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/viz/capture/redact-layout.json` (a window with five panels, two marked) and a synthetic frame with a unique colour per panel, when masked, then no pixel of either marked panel's colour remains and the other three panels are unchanged.
- **AC-2** Given the user setting listing a third panel type, then three panels are masked.
- **AC-3** (error path) Given no layout snapshot, then every pixel equals the fill pattern.
- **AC-4** Given the same frame and layout twice, then the masked frames are byte-identical.

## Design

- **Registry command(s):** none; the setting is changed with `settings.set_value@1` (CORE.STO.SET).
- **Data structures:** `LayoutSnapshot { window_id, mono_ns, panels: [{ type_id, rect: PixelRect }] }`; `PixelRect { x, y, w, h }` in device pixels.
- **Algorithm:** filter panels; clip rectangles; fill rows in place (RGBA8) with the fixed fill and hatch.
- **Errors:** none (missing snapshot means mask everything).
- **Determinism:** D0.
- **Performance budget:** masking a 1920x1080 frame < 1 ms [unverified until measured].
- **Crate:** `fid-capture` (module `rec::redact`).

## Test fixtures

`fixtures/viz/capture/redact-layout.json` and synthetic frames generated in the test; CC-BY-4.0.

## Build prompt

> In crate `fid-capture`, add module `rec::redact`. Declare the user-scope setting `capture.redacted_panels` (list of panel type IDs, default empty) and read the `redact_in_capture` flag from panel-type declarations. Implement `mask_for(&LayoutSnapshot) -> Vec<PixelRect>` (redacted panels' rectangles clipped to the window) and `apply_mask(&mut Frame, &[PixelRect])` that overwrites every masked pixel with a fixed opaque fill and hatch before the frame goes to any encoder; with no snapshot, mask the whole frame. Record the rectangle count in frame metadata. Tests AC-1..AC-4. Rules: no panics or `unsafe`; deny unwrap/expect/panic/todo/unimplemented/unreachable.

## Open questions

- The panel host must provide a layout snapshot per captured frame; requested from `VIZ.SHELL.DOCK`.

## Children

_Leaf: none._
