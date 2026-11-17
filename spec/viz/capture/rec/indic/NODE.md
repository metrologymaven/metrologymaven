---
id: VIZ.CAPTURE.REC.INDIC
title: Unsuppressible recording indicator
level: 4
parent: VIZ.CAPTURE.REC
kind: task
status: leaf
phase: P1b
size: S
owners: []
claimed_by:
satisfies: [REQ-VIZ.CAPTURE.REC-001]
covers: []
depends_on: [VIZ.CAPTURE.REC.SESSION, VIZ.SHELL.DOCK, VIZ.OVERLAY.STATUS]
uses_artifacts: []
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# VIZ.CAPTURE.REC.INDIC: Unsuppressible recording indicator

## Purpose

While a recording session is active, every window of the app shows a recording indicator (icon, the word "Recording" and elapsed time) that no theme, skin, kiosk layout or agent can hide, move off-screen or recolour into invisibility. It is drawn in a reserved layer after all panels, from fixed assets that are not theme tokens.

## Scope

**In:** the indicator widget; the reserved top-most draw layer per window; fixed icon, text and colours; elapsed time from the journal monotonic clock; a screen-reader announcement on start and stop.
**Out:** session state (`VIZ.CAPTURE.REC.SESSION`); window frame and panel host (`VIZ.SHELL.DOCK`); theme tokens (`VIZ.SHELL.THEME`).

## Requirements

- **REQ-VIZ.CAPTURE.REC.INDIC-001** While a recording is active, every app window shall draw the indicator within 100 ms of the start entry and every frame after that until stop.
- **REQ-VIZ.CAPTURE.REC.INDIC-002** The indicator's visibility, position, size, icon and colours shall not be read from theme tokens, skins, layouts or settings, and no registry command shall hide it.
- **REQ-VIZ.CAPTURE.REC.INDIC-003** The indicator shall be at least 24 px high at 100 % UI scale and scale up with UI scale, never down.
- **REQ-VIZ.CAPTURE.REC.INDIC-004** The indicator shall convey recording by icon and text, not by colour alone.
- **REQ-VIZ.CAPTURE.REC.INDIC-005** When a recording starts or stops, the accessibility tree shall announce it once.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/viz/capture/indicator-matrix` (4 themes x 2 skins x the kiosk layout x 2 windows), when a fake session starts, then a pixel probe of each window's indicator region matches the indicator golden in every combination.
- **AC-2** Given a skin fixture that sets all tokens to the background colour, then the indicator is still drawn with its fixed colours.
- **AC-3** Given the registry manifest, then no command parameter or setting key controls indicator visibility (manifest scan test).
- **AC-4** Given UI scale 75 %, then the indicator is drawn at 24 px; at 200 %, at 48 px.
- **AC-5** Given start and stop, then the accessibility test harness records exactly two announcements.

## Design

- **Registry command(s):** none.
- **Data structures:** `IndicatorState { active, recording_id, started_mono_ns }` subscribed from `SessionObserver`.
- **Algorithm:** each window's frame callback draws the indicator in a reserved layer after the toolkit's panels using a built-in icon atlas entry and fixed sRGB colours; elapsed time formatted as `hh:mm:ss` from the monotonic clock (display text, not a value of record).
- **Errors:** none; if the reserved layer cannot draw (for example GPU device lost), the window shows the toolkit fallback label "Recording".
- **Determinism:** D0 for the state and layout; pixels follow VIZ.RENDER tolerances.
- **Performance budget:** < 0.1 ms per frame [unverified until measured].
- **Crate:** `fid-capture` (module `rec::indic`), drawn through the shell's reserved-layer hook.

## Test fixtures

`fixtures/viz/capture/indicator-matrix`: generated window captures and goldens; icon is an original asset listed in ATTRIBUTION.md.

## Build prompt

> In crate `fid-capture`, add module `rec::indic`: a recording indicator drawn in a reserved top-most layer of every app window (through the shell's reserved-layer hook) while a recording session is active, from fixed built-in assets (icon, the text "Recording", elapsed `hh:mm:ss` from the journal monotonic clock) and fixed colours that no theme token, skin, layout, setting or command can change; at least 24 px high, scaling up with UI scale; announce start and stop once in the accessibility tree. Subscribe as a `SessionObserver`. Tests AC-1..AC-5. Rules: no panics or `unsafe`; deny unwrap/expect/panic/todo/unimplemented/unreachable; status never by colour alone; clean-room icon.

## Open questions

- `VIZ.SHELL.DOCK` must offer a reserved draw layer per window that themes cannot reach; that hook is requested here and is not yet in DOCK's interface.

## Children

_Leaf: none._
