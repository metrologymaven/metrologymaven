---
id: VIZ.KIOSK.STEPS.PICK
title: Element pick step on a simplified 3D view
level: 4
parent: VIZ.KIOSK.STEPS
kind: task
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-VIZ.KIOSK.STEPS-002, REQ-VIZ.KIOSK.STEPS-004]
covers: []
depends_on: [VIZ.PICK.RESOLVE, AUTO.PROG.OPER, AUTO.RUN.PLAY.ANSWER]
uses_artifacts: []
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# VIZ.KIOSK.STEPS.PICK: Element pick step on a simplified 3D view

## Purpose

Renders an `element` field: a simplified viewport (fixed lighting, orbit and zoom by large on-screen buttons and two-finger gestures, no menus) where the operator taps an element. The tap is resolved on the CPU by `view.pick_point@1`; the picked element is highlighted with its name and must be confirmed with a large button before the answer `program.answer_step@1` is sent with the element's stable ID. Pixel coordinates never leave this module.

## Scope

**In:** the reduced viewport host; tap to `view.pick_point@1`; highlight and name display (name as untrusted text); confirm and re-pick; large view controls (fit, front, top, iso, zoom in and out).
**Out:** pick resolution (`VIZ.PICK.RESOLVE`); validation of the element answer (`AUTO.PROG.OPER`); hosting (`VIZ.KIOSK.STEPS.LOCK`).

## Requirements

- **REQ-VIZ.KIOSK.STEPS.PICK-001** A tap shall be resolved by `view.pick_point@1`, and the answer shall carry only the resolved element's stable ID, never pixel coordinates.
- **REQ-VIZ.KIOSK.STEPS.PICK-002** The answer shall be sent only after the operator confirms the highlighted element.
- **REQ-VIZ.KIOSK.STEPS.PICK-003** If a tap resolves to no element, then the screen shall say "nothing picked" and send nothing.
- **REQ-VIZ.KIOSK.STEPS.PICK-004** Every view control and the confirm button shall be at least 15 mm on the reference kiosk display [unverified], and element names shall be shown as plain text.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/viz/golden-cube` with an element step, when the harness taps the top face and confirms, then `program.answer_step@1` receives the face's element ID and no coordinates.
- **AC-2** Given a tap on background, then "nothing picked" shows and nothing is sent.
- **AC-3** Given a tap without confirm, then nothing is sent; re-pick replaces the highlight.
- **AC-4** Given the target-size audit, then all controls are ≥ 15 mm.

## Design

- **Registry command(s):** none owned; invokes `view.pick_point@1` (R1) and `program.answer_step@1`.
- **Algorithm:** map tap position to device pixels in the reduced viewport; call the pick; highlight; on confirm, submit `{ field: element_id }`.
- **Errors:** `nothing_picked` (UI).
- **Determinism:** D0 (pick is D0 per VIZ.PICK.RESOLVE).
- **Performance budget:** highlight within 100 ms of tap [unverified].
- **Crate:** `fid-kiosk` (module `steps::pick`).

## Test fixtures

`fixtures/viz/golden-cube` (shared) with an element-step program.

## Build prompt

> In crate `fid-kiosk`, add module `steps::pick`: a reduced kiosk viewport for `element` fields with large view buttons (fit, front, top, iso, zoom) and touch orbit; resolve taps with `view.pick_point@1`, highlight the hit with its name as plain text, and on a large Confirm send `program.answer_step@1` with only the element's stable ID (no pixel data); "nothing picked" on a miss. Controls ≥ 15 mm. Tests AC-1..AC-4. Rules: picks resolved on the CPU (C2); no panics or `unsafe`; deny unwrap/expect/panic/todo/unimplemented/unreachable.

## Open questions

- Resolves the parent's question: the pick step reuses `VIZ.PICK.RESOLVE` directly.

## Children

_Leaf: none._
