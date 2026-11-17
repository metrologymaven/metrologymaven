---
id: VIZ.OVERLAY.GDTGLYPH
title: GD&T annotation rendering
level: 3
parent: VIZ.OVERLAY
kind: module
status: split
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-VIZ.OVERLAY-004, REQ-VIZ.OVERLAY-005]
covers: [PW.VIZ.04]
depends_on: [VIZ.RENDER.FRAME, VIZ.VIEW.CAMERA, GDT.CHAR, GDT.PMI.LINK, IO.CAD.PMI.READ, KRN.NUM.FMT.REG]
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# VIZ.OVERLAY.GDTGLYPH: GD&T annotation rendering

## Purpose

Compose feature control frames, datum feature symbols and datum target glyphs in 3D from the semantic characteristic model (type, value, modifiers, datum references), not from graphic PMI polylines, with dialect-aware symbols, annotation-plane anchoring and hover highlighting of the toleranced feature, datum features and DRF axes. Graphic PMI is an optional QA overlay showing semantic versus graphic disagreement.

## Scope

**In:**
- FCF and datum glyph composition from GDT.CHAR semantics
- Dialect-aware symbol sets per the characteristic's standard reference
- GD&T symbol MSDF atlas (symbols drawn clean-room)
- Anchoring on annotation planes, saved views or billboards; occlusion fade
- Hover: highlight toleranced and datum features, show standard reference ID (never standards text)
- Optional graphic-PMI QA overlay

**Out** (and which node owns it instead):
- The characteristic model and dialects (`GDT.CHAR`)
- Status chips on frames (`VIZ.OVERLAY.STATUS`)
- PMI import (`GDT.PMI`, `IO.CAD`)

## Requirements

- **REQ-VIZ.OVERLAY.GDTGLYPH-001** Every feature control frame shall be composed from the semantic characteristic (type, tolerance value, modifiers, datum references) and not from graphic PMI geometry.
- **REQ-VIZ.OVERLAY.GDTGLYPH-002** Tolerance values in frames shall be produced by the kernel formatter.
- **REQ-VIZ.OVERLAY.GDTGLYPH-003** Symbols and modifiers shall render according to the dialect of the characteristic's standard reference.
- **REQ-VIZ.OVERLAY.GDTGLYPH-004** Hovering a frame shall highlight its toleranced feature and datum features and shall show the standard reference ID, never standards text.
- **REQ-VIZ.OVERLAY.GDTGLYPH-005** When the graphic-PMI overlay is on, every frame whose semantic content disagrees with its graphic PMI shall be marked.
- **REQ-VIZ.OVERLAY.GDTGLYPH-006** All GD&T glyphs shall be original drawings listed in ATTRIBUTION.md, with no copied fonts or icons.

## Interfaces

- **Offers:** FCF and datum overlay passes; hover events for `VIZ.PICK` readouts.
- **Consumes:** `GDT.CHAR` characteristics and dialect tags; `KRN.NUM`; `VIZ.RENDER` MSDF text.

## Decomposition hints

COMPOSE (frame layout), SYMBOLS (atlas and dialect sets), ANCHOR, HOVER, PMIQA.

## Open questions

- P1a characteristics come from drawings and QIF MBD without CAD; annotation-plane anchoring needs CAD PMI (P1b), so P1a anchors on billboards [unverified].
- Symbols are drawn by `UX.ICONS.GDT` (`gdt-symbols@1`) and read here as an artifact; this module only converts them into an MSDF atlas (`VIZ.OVERLAY.GDTGLYPH.ATLAS`), so the clean-room rule is enforced at the drawing source.
- Authored tolerances are exact decimal text in the characteristic model; frames show them verbatim in their authored unit and use the kernel formatter only for converted values (`VIZ.OVERLAY.GDTGLYPH.COMPOSE`).

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| VIZ.OVERLAY.GDTGLYPH.COMPOSE | Feature control frame and datum glyph composition (leaf) | 001 002 003 | PW.VIZ.04 |
| VIZ.OVERLAY.GDTGLYPH.ATLAS | GD&T symbol MSDF atlas from the original drawings (leaf) | 003 006 | none |
| VIZ.OVERLAY.GDTGLYPH.PLACE | GD&T annotation anchoring, occlusion fade and hover (leaf) | 004 | PW.VIZ.04 |
| VIZ.OVERLAY.GDTGLYPH.PMIQA | Graphic PMI quality-check overlay (leaf) | 005 | none |
