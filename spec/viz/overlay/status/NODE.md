---
id: VIZ.OVERLAY.STATUS
title: Status markers, locked palette guard and provenance badges
level: 3
parent: VIZ.OVERLAY
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-VIZ.OVERLAY-005, REQ-VIZ.OVERLAY-006, REQ-VIZ.OVERLAY-008]
covers: []
depends_on: [VIZ.RENDER]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# VIZ.OVERLAY.STATUS: Status markers, locked palette guard and provenance badges

## Purpose

One pure mapping from a kernel-supplied status and provenance to what the user sees: an icon, a pattern, a text equivalent and a colour from the locked status palette. A guard rejects any palette, theme or skin that would draw a nonconforming or out-of-tolerance state in a conforming colour. Every overlay, the shell and the kiosk use this mapping, so the rule lives in one place.

## Scope

**In:** the `StatusGlyph` mapping for the six display states (CONFORMS, NONCONFORMS, INDETERMINATE, NOT_DECIDABLE, not evaluated, stale); the `ProvenanceBadge` mapping for five provenances (computed, measured, entered, reported by source, suggested); the palette guard; the stripe pattern for out-of-tolerance regions.
**Out:** the theme token system and theme switching (`VIZ.SHELL`); computing statuses (`GDT`, `SCAN`, `UNC`); colour maps (`VIZ.OVERLAY.CMAP`).

## Requirements

- **REQ-VIZ.OVERLAY.STATUS-001** Every status display state shall map to a distinct icon, a distinct pattern and a text equivalent in addition to its colour.
- **REQ-VIZ.OVERLAY.STATUS-002** If a palette, theme or skin assigns a nonconforming, out-of-tolerance or stale state a colour within the configured perceptual distance of any conforming-set colour, then the guard shall reject it with a typed error and keep the locked default.
- **REQ-VIZ.OVERLAY.STATUS-003** Stale and not-evaluated states shall never be rendered with the conforming or nonconforming colour, icon or text.
- **REQ-VIZ.OVERLAY.STATUS-004** Every displayed number passed through the overlay API shall carry exactly one provenance badge from the five provenances, and an unbadged number shall be rejected at build time of the overlay item.
- **REQ-VIZ.OVERLAY.STATUS-005** The mapping shall be a pure function of status, provenance and theme with no dependence on UI toolkit state.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/viz/status-matrix` (6 states × 5 provenances × 4 themes: light, dark, high-contrast, shop-floor), when the mapping runs for every combination, then every state has a unique icon, unique pattern and non-empty text, identical across themes except colour.
- **AC-2** Given skin fixture `fixtures/viz/skin-green-nonconform.json` that sets NONCONFORMS to the conforming green, when the guard validates it, then it fails with `LockedPaletteViolation { state: NONCONFORMS }` and the active palette remains the locked default.
- **AC-3** Given the CIEDE2000 threshold of 20 [unverified] in the guard config, when a skin sets INDETERMINATE to a colour within that distance of CONFORMS green, then the guard rejects it.
- **AC-4** (error path) Given an overlay item with a number and no provenance, when it is built, then it fails with `MissingProvenance` and is not drawn.
- **AC-5** Given the same inputs, when the mapping runs twice, then outputs are identical (pure function, D0).

## Design

- **Registry command(s):** none (internal mapping used by overlays, shell and kiosk).
- **Data structures:** `DisplayStatus { Conforms, Nonconforms, Indeterminate, NotDecidable, NotEvaluated, Stale }`; `Provenance { Computed, Measured, Entered, ReportedBySource, Suggested }`; `StatusGlyph { icon: IconId, pattern: PatternId, text_key: MessageId, color: LockedColor }`; `PaletteGuardConfig { min_delta_e: f64 }`.
- **Algorithm:** table lookup for icon, pattern and text; colour from the locked palette, overridable by a theme only for states and colours that pass the guard; the guard computes CIEDE2000 (Sharma, Wu and Dalal, 2005) between each non-conforming-set colour and every conforming-set colour.
- **Errors:** `LockedPaletteViolation { state }`, `MissingProvenance`, `UnknownTheme`.
- **Determinism:** D0 (pure table lookup and f64 colour difference without FMA).
- **Performance budget:** mapping is constant time; guard validation under 1 ms per skin.
- **Crate:** `fid-overlay` (module `status`).

## Test fixtures

`fixtures/viz/status-matrix` and `fixtures/viz/skin-green-nonconform.json`: hand-written in-repo; icons and patterns are original assets listed in ATTRIBUTION.md.

## Build prompt

> In crate `fid-overlay`, add module `status`. Implement `status_glyph(status, theme) -> StatusGlyph` and `provenance_badge(provenance) -> Badge` as pure functions over six display states (Conforms, Nonconforms, Indeterminate, NotDecidable, NotEvaluated, Stale) and five provenances (Computed, Measured, Entered, ReportedBySource, Suggested). Each state has a unique icon, unique pattern and a localisable text key independent of theme; only colour varies by theme. Implement a palette guard: a theme or skin may not give a nonconforming, indeterminate, not-decidable, not-evaluated or stale state a colour within a configurable CIEDE2000 distance (default 20) of any conforming colour; reject with `LockedPaletteViolation` and keep the locked default. Overlay items that show a number must carry a provenance or fail with `MissingProvenance`. Tests: full 6×5×4 matrix uniqueness; guard rejects the green-nonconform skin and a near-green indeterminate; missing provenance error; purity. Rules: no panics or `unsafe`; deny unwrap/expect/panic/todo/unimplemented/unreachable; clean-room icons only; status never by colour alone.

## Open questions

- The CIEDE2000 threshold value needs a review by an accessibility specialist; VIZ.SHELL owns theme tokens and must call this guard when loading skins.

## Children

_Leaf: none._
