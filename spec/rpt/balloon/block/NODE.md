---
id: RPT.BALLOON.BLOCK
title: Ballooned-drawing report block
level: 3
parent: RPT.BALLOON
kind: module
status: open
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-RPT.BALLOON-009]
covers: []
depends_on: [RPT.BALLOON.REG, RPT.BALLOON.SHEET, RPT.MODEL.DOC]
cli: [fid report set-drawing-balloon]
mcp: [report_set_drawing_balloon]
journal: [report.set_drawing_balloon@1]
updated: 2026-11-16
---

# RPT.BALLOON.BLOCK: Ballooned-drawing report block

## Purpose

A report block kind `drawing` that shows a drawing sheet with its balloons drawn at their stored sheet coordinates, optionally coloured by each characteristic's decision outcome. It lets FAI packages and general reports include the ballooned drawing that the form 3 rows refer to.

## Scope

**In:**
- Block kind `drawing` registered with the `RPT.MODEL.DOC` block registry: sheet ID, crop region, scale-to-fit, balloon styling, outcome colouring option
- Resolution of balloon positions and numbers from the register at generation time
- Vector balloon overlay for PDF output and raster overlay for preview

**Out** (and which node owns it instead):
- Rendering to PDF (`RPT.OUTPUT.PDF`)
- Balloon register and sheets (`RPT.BALLOON.REG`, `RPT.BALLOON.SHEET`)

## Requirements

- **REQ-RPT.BALLOON.BLOCK-001** The `drawing` block shall place each balloon at its stored sheet coordinates scaled by the block's sheet-to-page transform, so that it stays on its callout at any block size.
- **REQ-RPT.BALLOON.BLOCK-002** The `drawing` block shall show balloon numbers resolved from the register at generation time, so renumbering is reflected without editing the block.
- **REQ-RPT.BALLOON.BLOCK-003** When outcome colouring is on, each balloon shall show its characteristic's decision outcome with both a colour and a symbol.
- **REQ-RPT.BALLOON.BLOCK-004** Balloons shall be emitted as vector graphics over the sheet image, so that they remain sharp in PDF output.

## Interfaces

- **Offers:** block kind `drawing`; `report.set_drawing_balloon@1` (W2) to edit its payload.
- **Consumes:** block registry (`RPT.MODEL.DOC`), register and sheets (siblings), decisions through `RPT.MODEL.FMT`.

## Decomposition hints

Likely one leaf.

## Open questions

- None yet.

## Children

_None yet: status `open`._
