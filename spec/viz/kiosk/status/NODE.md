---
id: VIZ.KIOSK.STATUS
title: Large live status display
level: 3
parent: VIZ.KIOSK
kind: task
status: leaf
phase: P1b
size: S
owners: []
claimed_by:
satisfies: [REQ-VIZ.KIOSK-005]
covers: []
depends_on: [VIZ.OVERLAY.STATUS, VIZ.KIOSK.SESSION]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# VIZ.KIOSK.STATUS: Large live status display

## Purpose

The big conformance indicator an operator reads from a distance with gloves on: the cycle's kernel-supplied outcome as icon, text and pattern at a guaranteed physical size, with stale and not-evaluated states impossible to mistake for a pass.

## Scope

**In:** the status widget, physical sizing from display DPI, live update from cycle results, the summary counts line.
**Out:** status glyph mapping and locked palette (`VIZ.OVERLAY.STATUS`); computing outcomes (`GDT`, `SCAN`, `UNC`).

## Requirements

- **REQ-VIZ.KIOSK.STATUS-001** The display shall show the cycle outcome as text and icon with a minimum glyph height of 20 mm on the reference kiosk display [unverified].
- **REQ-VIZ.KIOSK.STATUS-002** The display shall use the shared status mapping, so every state has icon, pattern and text in addition to colour.
- **REQ-VIZ.KIOSK.STATUS-003** While any characteristic of the cycle is stale or not evaluated, the overall display shall not show CONFORMS.
- **REQ-VIZ.KIOSK.STATUS-004** When a new kernel result for the cycle arrives, the display shall update within 500 ms [unverified].

## Acceptance criteria

- **AC-1** Given fixture `fixtures/viz/kiosk-results` (cycle with 10 characteristics, all CONFORMS) on the reference kiosk display profile (DPI from the profile), then the rendered outcome glyph height is at least 20 mm and the text reads the CONFORMS label.
- **AC-2** Given the same cycle with one characteristic stale, then the overall display shows the stale state, not CONFORMS.
- **AC-3** Given each of the six display states, then the widget uses the icon, pattern and text from the shared mapping (snapshot test).
- **AC-4** Given a result event, then the widget updates within 500 ms in the test harness.

## Design

- **Registry command(s):** none.
- **Data structures:** `CycleOutcome` derived from kernel ResultRecords by the rule "worst state wins" in the order NONCONFORMS, NOT_DECIDABLE, INDETERMINATE, stale, not evaluated, CONFORMS.
- **Algorithm:** subscribe to cycle results; compute the overall display state by the fixed precedence (a display rule, not a value of record: the per-characteristic decisions are the kernel's); size glyphs as `mm × dpi / 25.4` pixels.
- **Errors:** `UnknownDpi` falls back to the largest glyph size that fits.
- **Determinism:** D0.
- **Performance budget:** see REQ-004.
- **Crate:** `fid-kiosk` (module `status`).

## Test fixtures

`fixtures/viz/kiosk-results`: generated cycle results; in-repo.

## Build prompt

> In crate `fid-kiosk`, add module `status`: a large status widget showing the cycle's overall display state with icon, pattern and text from `fid-overlay`'s status mapping, glyph height at least 20 mm computed from the display DPI, updated within 500 ms of new kernel results. Overall state uses fixed precedence NONCONFORMS > NOT_DECIDABLE > INDETERMINATE > stale > not evaluated > CONFORMS, so a stale or unevaluated characteristic can never show CONFORMS. Tests: physical size on the reference profile; stale precedence; six-state snapshot; update latency. Rules: no panics or `unsafe`; deny unwrap/expect/panic/todo/unimplemented/unreachable; status never by colour alone.

## Open questions

- The overall-state precedence is a display convention; confirm with GDT/UNC that it never conflicts with a program-declared part-level decision rule.

## Children

_Leaf: none._
