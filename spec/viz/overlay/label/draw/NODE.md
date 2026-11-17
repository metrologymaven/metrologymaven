---
id: VIZ.OVERLAY.LABEL.DRAW
title: Label value resolution, kernel formatting, leaders and text
level: 4
parent: VIZ.OVERLAY.LABEL
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-VIZ.OVERLAY.LABEL-001]
covers: [GOM.VIZ.05]
depends_on: [VIZ.OVERLAY.LABEL.MODEL, VIZ.OVERLAY.STATUS, SCAN.CMP.LABEL, GDT.CHAR.RECORD, KRN.NUM.FMT.REG, VIZ.RENDER.FRAME]
uses_artifacts: [UX.TOKENS.TYPE]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# VIZ.OVERLAY.LABEL.DRAW: Label value resolution, kernel formatting, leaders and text

## Purpose

Turns each label into what the user reads: it reads the source's value of record (a SCAN deviation label's stored value, or a ResultRecord's nominal, actual, deviation, limits, uncertainty and decision), formats every number with the kernel formatter from the stored f64, attaches a provenance badge and status glyph, measures the text box with fixed font metrics, and draws the box, text and leader line. It also offers the deterministic box-size function that label layout uses.

## Scope

**In:** `resolve(label) -> LabelContent` (strings, formatter IDs, status, provenance, anchor point in metres); `measure(content) -> BoxSize` from fixed advance tables of the bundled UI font at a given scale; drawing boxes, MSDF text and leader lines in a frame-graph pass; "source changed" refresh when the source digest changes; "no value" display for `NO_MATCH` labels.
**Out:** the label entity and commands (`VIZ.OVERLAY.LABEL.MODEL`); placement (`VIZ.OVERLAY.LABEL.LAYOUT`); values (`SCAN.CMP.LABEL`, `GDT.CHAR.RECORD`); glyph mapping (`VIZ.OVERLAY.STATUS`).

## Requirements

- **REQ-VIZ.OVERLAY.LABEL.DRAW-001** Every number in a label shall be the kernel formatter's string for the source's stored f64 value of record, and the label content shall record the formatter ID used for each number.
- **REQ-VIZ.OVERLAY.LABEL.DRAW-002** Every number shall carry a provenance badge, and every label with a decision shall carry the status glyph from `VIZ.OVERLAY.STATUS`.
- **REQ-VIZ.OVERLAY.LABEL.DRAW-003** When a source's digest changes, the label shall show the new value within one frame of the new digest becoming available and never mix old and new fields.
- **REQ-VIZ.OVERLAY.LABEL.DRAW-004** `measure` shall be a pure function of content and UI scale, using fixed font advance tables, so box sizes are identical across platforms.
- **REQ-VIZ.OVERLAY.LABEL.DRAW-005** A label whose source has status `NO_MATCH` shall show "no value" and no number.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/viz/labels/part20`, when every label is resolved, then each number string equals `fid_num::fmt::format` of the stored f64 (property test) and each carries a formatter ID, provenance and, where decided, a status glyph.
- **AC-2** Given a source recomputed to a new digest, then all fields switch in the same frame (UI test with a frame log).
- **AC-3** Given the same content on Linux, Windows and macOS CI, then `measure` returns identical sizes.
- **AC-4** Given a `NO_MATCH` label, then "no value" is shown.
- **AC-5** Given a headless render of 20 labels, then the image matches the golden within tolerance.

## Design

- **Registry command(s):** none.
- **Data structures:** `LabelContent { lines: [Line { field, text, formatter_id?, provenance? }], status?, anchor_m: [f64; 3], source_digest }`; `BoxSize { w, h }` in device-independent px.
- **Algorithm:** read source element; for deviation labels take the stored value; for characteristics read the ResultRecord via `gdt.get_result@1`; format; measure with advance tables of the font named by `UX.TOKENS.TYPE`; draw.
- **Errors:** `SourceMissing` (label drawn with "source missing").
- **Determinism:** D0 for content and sizes; pixels follow VIZ.RENDER tolerances.
- **Performance budget:** 500 labels resolved and measured < 20 ms [unverified].
- **Crate:** `fid-overlay` (module `label::draw`).

## Test fixtures

`fixtures/viz/labels/part20` (shared) plus a golden image.

## Build prompt

> In crate `fid-overlay`, add module `label::draw`: `resolve(&Label) -> LabelContent` reading values of record from the source (SCAN deviation-label element value, or ResultRecord via `gdt.get_result@1`), formatting every number with `fid_num::fmt` from the stored f64 and recording formatter IDs, adding provenance badges and status glyphs from the status module, "no value" for `NO_MATCH`; `measure(&LabelContent, scale) -> BoxSize` from fixed font advance tables (deterministic across platforms); and a frame-graph pass drawing boxes, MSDF text and leaders, refreshing atomically when the source digest changes. Tests AC-1..AC-5. Rules: no value computed or re-rounded in the UI (C1, REQ-VIZ-002); no panics or `unsafe`; deny unwrap/expect/panic/todo/unimplemented/unreachable.

## Open questions

- Dimension sources (non-GD&T dimensions) are assumed to be characteristics with ResultRecords; confirm with GDT.

## Children

_Leaf: none._
