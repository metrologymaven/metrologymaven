---
id: VIZ.OVERLAY.GDTGLYPH.COMPOSE
title: Feature control frame and datum glyph composition
level: 4
parent: VIZ.OVERLAY.GDTGLYPH
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-VIZ.OVERLAY.GDTGLYPH-001, REQ-VIZ.OVERLAY.GDTGLYPH-002, REQ-VIZ.OVERLAY.GDTGLYPH-003]
covers: [PW.VIZ.04]
depends_on: [GDT.CHAR.MODEL.TYPES, GDT.CHAR.SPEC, KRN.NUM.FMT.REG]
uses_artifacts: [UX.ICONS.GDT]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# VIZ.OVERLAY.GDTGLYPH.COMPOSE: Feature control frame and datum glyph composition

## Purpose

A pure layout function from a semantic characteristic to a 2D glyph description: feature control frames (characteristic symbol, zone-shape symbol, tolerance, material and other modifiers, datum references with their modifiers, composite and simultaneous segments) and datum feature and datum target symbols. Input is the `FeatureControlFrame` of `GDT.CHAR.MODEL.TYPES`, never graphic PMI geometry. The symbol set follows the dialect of the characteristic's governing spec (`GDT.CHAR.SPEC`), and proportions follow the UX GD&T drawing specification (`gdt-symbols@1`).

## Scope

**In:** `compose(characteristic, governing_spec, display_unit) -> FrameGlyph`; cell layout in units of text height h; dialect-based symbol choice (ASME or ISO flags from `gdt-symbols@1`); tolerance text; datum feature symbol and datum target glyphs; composite and simultaneous frames stacked as the model gives them.
**Out:** the symbol atlas (`VIZ.OVERLAY.GDTGLYPH.ATLAS`); 3D placement and hover (`VIZ.OVERLAY.GDTGLYPH.PLACE`); status chips on frames (`VIZ.OVERLAY.STATUS`); the characteristic model and dialects (`GDT.CHAR`).

## Requirements

- **REQ-VIZ.OVERLAY.GDTGLYPH.COMPOSE-001** `compose` shall build every frame only from the characteristic's semantic `FeatureControlFrame` and governing spec, and shall take no graphic PMI input.
- **REQ-VIZ.OVERLAY.GDTGLYPH.COMPOSE-002** A tolerance shown in its authored unit shall be the authored decimal text unchanged, and a tolerance shown in another display unit shall be the kernel formatter's output (`fid_num::fmt`) for the converted value, with the formatter ID recorded in the glyph.
- **REQ-VIZ.OVERLAY.GDTGLYPH.COMPOSE-003** Every symbol and modifier shall be chosen from the `gdt-symbols@1` entries flagged for the characteristic's dialect, and if a required symbol has no entry for that dialect, then `compose` shall return `SymbolNotInDialect` naming it.
- **REQ-VIZ.OVERLAY.GDTGLYPH.COMPOSE-004** If the characteristic has no complete governing spec, then `compose` shall still lay out the frame and mark it `dialect_unknown` instead of guessing a dialect.
- **REQ-VIZ.OVERLAY.GDTGLYPH.COMPOSE-005** `compose` shall be deterministic, and its output serialised to canonical JSON shall be byte-identical across platforms for the same inputs.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/viz/gdt/frames.json` (40 characteristics covering every characteristic type, modifiers, composite and simultaneous frames, in ASME and ISO dialects) with expected `FrameGlyph` JSON, when `compose` runs, then every output matches byte for byte.
- **AC-2** Given a position tolerance authored `0.05 mm` and display unit mm, then the text is `0.05`; with display unit in, then the text equals `fid_num::fmt::format` of the converted value and the formatter ID is set.
- **AC-3** (error path) Given an ISO-only modifier under ASME, then `SymbolNotInDialect` names it.
- **AC-4** Given no governing spec, then the frame is laid out and flagged `dialect_unknown`.

## Design

- **Registry command(s):** none.
- **Data structures:** `FrameGlyph { cells: [Cell { kind, symbol?: SymbolId, text?: String, formatter_id?, rect_h: [f64; 4] }], separators, flags }` in units of h.
- **Algorithm:** walk the FCF in model order; choose symbols by dialect; measure text with fixed advance tables from the atlas metrics; place cells left to right with paddings from `gdt-symbols@1` frame proportions.
- **Errors:** `SymbolNotInDialect`, `UnsupportedKind` (with the kind).
- **Determinism:** D0 (f64 layout with fixed operation order, no FMA).
- **Performance budget:** 1,000 frames composed < 20 ms [unverified].
- **Crate:** `fid-overlay` (module `gdt::compose`).

## Test fixtures

`fixtures/viz/gdt/frames.json`, written from the characteristic model with original expected layouts; CC-BY-4.0. No standards figures are used.

## Build prompt

> In crate `fid-overlay`, add module `gdt::compose`: `compose(&Characteristic, &GoverningSpec, DisplayUnit) -> Result<FrameGlyph, ComposeError>` laying out feature control frames, composite and simultaneous segments, datum feature and datum target symbols from the semantic model only (no graphic PMI), with symbols chosen from `gdt-symbols@1` entries for the characteristic's dialect (`SymbolNotInDialect` otherwise; `dialect_unknown` flag without a governing spec) and proportions in units of text height. Authored tolerances in their own unit show the authored text verbatim; other units use `fid_num::fmt` with the formatter ID recorded. Deterministic canonical JSON output. Tests AC-1..AC-4. Rules: never paste or store standards text; cite symbols by ID only; no panics or `unsafe`; deny unwrap/expect/panic/todo/unimplemented/unreachable.

## Open questions

- REQ-VIZ.OVERLAY.GDTGLYPH-002 says "kernel formatter"; authored tolerances are exact decimal text (`GDT.CHAR.MODEL.TYPES`), so showing them verbatim is treated as satisfying it; only converted values go through `fid_num::fmt`.

## Children

_Leaf: none._
