---
id: RPT.OUTPUT.PDF.DRAW
title: Block painters for tables, text, images, views, drawings and form fields
level: 4
parent: RPT.OUTPUT.PDF
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-RPT.OUTPUT.PDF-004, REQ-RPT.OUTPUT.PDF-005]
covers: [PW.RPT.04, SA.RPT.03]
depends_on: [RPT.OUTPUT.PDF.LAYOUT, RPT.OUTPUT.PDF.TEXT, CORE.STO.CAS]
uses_artifacts: [VIZ.CAPTURE.SHOT, RPT.BALLOON.BLOCK, RPT.FORMS.LAYOUT.MAP]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
built_in: [fid-report-pdf]
updated: 2026-11-23
---

# RPT.OUTPUT.PDF.DRAW: Block painters for tables, text, images, views, drawings and form fields

## Purpose

Paints each laid-out block into PDF content-stream operators: tables (cells, rules, outcome colouring), text, header and footer, images and view snapshots (embedded as image XObjects from their stored bytes), ballooned drawing sheets with balloons as vector overlays, and form fields. It keeps a registry from block kind to painter; a kind with no painter makes the export fail rather than silently drop content.

## Scope

**In:** `Painter` trait and registry; painters for `table`, `text`, `header`, `footer`, `image`, `view`, `drawing`, `form_field`; image decoding of PNG and JPEG into PDF image objects (JPEG passed through, PNG re-encoded with Flate at a fixed level); vector balloons; the `report.unsupported_block` check over a whole document before any byte is written.

**Out:** charts (`RPT.OUTPUT.PDF.CHART`, registers its own painter); positions (`RPT.OUTPUT.PDF.LAYOUT`); fonts (`RPT.OUTPUT.PDF.TEXT`); object serialisation (`RPT.OUTPUT.PDF.WRITE`).

## Requirements

- **REQ-RPT.OUTPUT.PDF.DRAW-001** Every number painted shall be the `ValueText` display string from the resolved report, unchanged, and a painter shall never format, round or convert a value.
- **REQ-RPT.OUTPUT.PDF.DRAW-002** Before painting, the system shall check every block of the resolved report against the painter registry, and if any kind has no painter it shall fail with error `report.unsupported_block` naming the first such block ID, before any output is produced.
- **REQ-RPT.OUTPUT.PDF.DRAW-003** Image and view-snapshot bytes shall be read from the content-addressed store by digest and verified, and embedding the same bytes twice shall give byte-identical image objects.
- **REQ-RPT.OUTPUT.PDF.DRAW-004** A `drawing` block shall paint the sheet image and each balloon as a vector circle with its number at the stored sheet coordinates, converted exactly to page units.
- **REQ-RPT.OUTPUT.PDF.DRAW-005** Values carrying the nonconformance marker shall be painted with the marker visible next to the value.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/rpt/model/golden/bracket_3stage.resolved.json`, when painted, then a text extraction of the content streams contains every `ValueText` display string exactly once per occurrence and nothing else numeric beyond page numbers.
- **AC-2** (error path) Given a resolved report containing a `chart` block and no chart painter registered, when exported, then it fails with `report.unsupported_block` naming that block and no output is produced.
- **AC-3** Given fixture `fixtures/rpt/output/draw/ballooned_sheet` (a sheet with 12 balloons), when painted, then each balloon circle centre equals its stored coordinate within 1 µm.
- **AC-4** Given the same PNG embedded in two reports, then both image objects are byte-identical; a corrupted digest fails with `report.snapshot_missing`.
- **AC-5** Given a value with a tie marker, then the marker glyph appears next to the value in the content stream.

## Design

- **Data structures:** `trait Painter { fn kind(&self) -> BlockKindId; fn paint(&self, laid: &LaidBox, block: &ResolvedBlock, ctx: &mut PaintCtx) -> Result<(), PaintError>; }`; `PaintCtx` collects content-stream operators, font usage and image objects.
- **Algorithm:** registry check over all blocks first; then per page, per box in order, call the painter; JPEG pass-through, PNG decode (bounded) and Flate at level 6 with a pure-Rust deflater [unverified determinism across versions: pin the library].
- **Errors:** `report.unsupported_block`, `report.snapshot_missing`, `report.image_invalid`.
- **Determinism:** D0.
- **Performance budget:** paint 50 pages with 20 images in under 5 s [unverified until measured].
- **Crate:** `fid-report-pdf` (module `draw`).

## Test fixtures

Resolved fixtures from `RPT.MODEL.BIND.GEN`; `fixtures/rpt/output/draw/ballooned_sheet` (synthetic drawing and register). CC-BY-4.0.

## Build prompt

> In crate `fid-report-pdf`, add module `draw`: a `Painter` trait and registry plus painters for `table`, `text`, `header`, `footer`, `image`, `view`, `drawing` (sheet image plus vector balloons) and `form_field`. Check all blocks against the registry before painting (`report.unsupported_block` naming the first unsupported block; no output). Paint numbers only as the resolved `ValueText` display strings, with markers visible; never format numbers. Read images by digest from the content-addressed store and verify them; JPEG pass-through, PNG re-encoded with pinned Flate settings so bytes are reproducible. Acceptance: AC-1..AC-5 in `spec/rpt/output/pdf/draw/NODE.md`. Constraints: CONSTITUTION C1, C2 (snapshots are display-only images), §8 (bounded decoders, no panic, no `unsafe`).

## Open questions

- Outcome colours come from the style set (`RPT.DESIGN.STYLE`); in P1a a fixed default palette applies.

## Children

_Leaf: none._
