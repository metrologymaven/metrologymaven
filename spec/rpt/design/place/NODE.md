---
id: RPT.DESIGN.PLACE
title: Block placement, sizing and overflow
level: 3
parent: RPT.DESIGN
kind: module
status: open
phase: P1b
size: L
owners: []
claimed_by:
satisfies: [REQ-RPT.DESIGN-001, REQ-RPT.DESIGN-002, REQ-RPT.DESIGN-006, REQ-RPT.DESIGN-007]
covers: [PW.RPT.01]
depends_on: [RPT.DESIGN.PAGE, RPT.MODEL.DOC]
cli: [fid report move-block-layout, fid report align-block-layout, fid report group-block-layout, fid report order-block-layout]
mcp: [report_move_block_layout, report_align_block_layout, report_group_block_layout, report_order_block_layout]
journal: [report.move_block_layout@1, report.align_block_layout@1, report.group_block_layout@1, report.order_block_layout@1]
updated: 2026-11-16
---

# RPT.DESIGN.PLACE: Block placement, sizing and overflow

## Purpose

Positions and sizes blocks on a page: move, resize, snap to grid, align and distribute, group, and set z-order. It also detects blocks that leave the printable body area and reports overflow. Every operation is a W2 command on the report model, so the designer UI, scripts and agents use the same path.

## Scope

**In:**
- Block frame (x, y, width, height) in exact 0.01 mm units relative to the page body
- Grid and snapping (grid pitch 0.1 mm or coarser)
- Align and distribute a selection; group and ungroup; z-order
- Overflow detection against the body area from `RPT.DESIGN.PAGE`, both in the designer and at generation time
- Automatic flow of repeated rows or pages onto following pages (page break rules)

**Out** (and which node owns it instead):
- Page size and margins (`RPT.DESIGN.PAGE`)
- Fonts, colours and table styles (`RPT.DESIGN.STYLE`)
- The interactive canvas, handles and mouse interaction (`RPT.DESIGN.UI`)

## Requirements

- **REQ-RPT.DESIGN.PLACE-001** The system shall store each block frame as integers in 0.01 mm units relative to the page body origin.
- **REQ-RPT.DESIGN.PLACE-002** When snapping is on, the system shall snap block edges to a grid whose pitch is a user setting of at least 0.1 mm (default 1 mm).
- **REQ-RPT.DESIGN.PLACE-003** Each move, resize, align, distribute, group, ungroup or z-order change shall be one journaled W2 command, and replaying the journal shall give the same frames.
- **REQ-RPT.DESIGN.PLACE-004** If a block frame extends outside the page body area, then the system shall flag the block in the designer and generation shall report warning `report.block_overflow` naming the block ID.
- **REQ-RPT.DESIGN.PLACE-005** When a repeated table or page group exceeds the body area, the system shall continue it on following pages according to the block's page-break rule, and shall not truncate rows silently.
- **REQ-RPT.DESIGN.PLACE-006** Placement commands shall not change any bound value, decision, binding or formatter choice.

## Interfaces

- **Offers:** commands `report.move_block_layout@1`, `report.align_block_layout@1`, `report.group_block_layout@1`, `report.order_block_layout@1` (W2); type `BlockFrame`; an overflow check used by `RPT.MODEL.BIND` and `RPT.DESIGN.PREVIEW`.
- **Consumes:** body rectangles (`RPT.DESIGN.PAGE`), report model (`RPT.MODEL.DOC`).

## Decomposition hints

FRAME (frames, move, resize, snapping), ARRANGE (align, distribute, group, z-order), FLOW (page breaks and overflow).

## Open questions

- Page-break flow depends on rendered row heights, which depend on fonts (`RPT.OUTPUT.PDF`); the measurement contract between layout and renderer needs to be fixed at leaf level.

## Children

_None yet: status `open`._
