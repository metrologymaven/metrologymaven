---
id: RPT.MODEL.BLOCKS
title: Content block types
level: 3
parent: RPT.MODEL
kind: module
status: open
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-RPT.MODEL-001, REQ-RPT.MODEL-010, REQ-RPT.MODEL-012]
covers: [GOM.RPT.03, GOM.RPT.04, GOM.RPT.05, PCD.RPT.03, PW.RPT.03, SA.RPT.02, SA.RPT.04]
depends_on: [RPT.MODEL.DOC, VIZ.VIEW, VIZ.CAPTURE]
cli: [fid report set-table-block, fid report set-chart-block, fid report set-view-block]
mcp: [report_set_table_block, report_set_chart_block, report_set_view_block]
journal: [report.set_table_block@1, report.set_chart_block@1, report.set_view_block@1]
updated: 2026-11-16
---

# RPT.MODEL.BLOCKS: Content block types

## Purpose

The standard block kinds a report is built from, each with a versioned payload schema registered with the `RPT.MODEL.DOC` block-kind registry: text with bound fields, characteristic tables with column customisation, chart blocks, restorable 3D view blocks, images, and page header and footer. Blocks describe *what* to show; they never compute values (series come from `RPT.STATS` or `LVM`) and never fix position or style (`RPT.DESIGN`).

## Scope

**In:**
- `text` block with bound fields
- `table` block: columns chosen, ordered and renamed from ResultRecord fields and characteristic attributes; row filters by decision outcome and tag
- `chart` block: lays out a `ChartSeries` (trend, histogram, control chart, vector group plot) supplied by another node
- `view` block: stores a VIZ saved-view ID plus the snapshot image digest; restores the scene when opened
- `image` block stored by content digest
- `header` and `footer` blocks with page number, report ID and digest fields

**Out** (and which node owns it instead):
- Block envelope, IDs and registry (`RPT.MODEL.DOC`)
- Computing chart series (`RPT.STATS`); vector group computation (`LVM`)
- Saved-view storage and snapshot rendering (`VIZ.VIEW`, `VIZ.CAPTURE`)
- Ballooned-drawing block (`RPT.BALLOON`) and form-field blocks (`RPT.FORMS`)

## Requirements

- **REQ-RPT.MODEL.BLOCKS-001** Each block kind shall register a payload schema with a version number, and a payload that fails its schema shall be rejected by the registry.
- **REQ-RPT.MODEL.BLOCKS-002** A `table` block shall let the user choose, order and rename columns from ResultRecord fields and characteristic attributes, and shall store the column list in the block payload.
- **REQ-RPT.MODEL.BLOCKS-003** A `table` block shall support row filters by decision outcome and by tag, applied at generation time.
- **REQ-RPT.MODEL.BLOCKS-004** A `chart` block shall reference a `ChartSeries` by stable ID and chart type, and shall not compute any series value itself.
- **REQ-RPT.MODEL.BLOCKS-005** A `view` block shall store the VIZ saved-view ID and the snapshot image digest, and when the user opens the block the system shall restore the scene to that saved view.
- **REQ-RPT.MODEL.BLOCKS-006** If a `view` block's saved view no longer exists, then the block shall keep its stored snapshot and generation shall report warning `report.view_missing`.
- **REQ-RPT.MODEL.BLOCKS-007** An `image` block shall store its image by content digest, and an image larger than 50 MiB [unverified limit] shall be rejected with error `report.image_too_large`.
- **REQ-RPT.MODEL.BLOCKS-008** `header` and `footer` blocks shall offer page number, page count, report ID and resolved-report digest fields.
- **REQ-RPT.MODEL.BLOCKS-009** A `text` block's bound fields shall be resolved only through `RPT.MODEL.FMT` and shall never evaluate user text as an expression or instruction (CONSTITUTION C6).

## Interfaces

- **Offers:** block kinds `text`, `table`, `chart`, `view`, `image`, `header`, `footer`; commands `report.set_table_block@1`, `report.set_chart_block@1`, `report.set_view_block@1` (W2) that edit payloads.
- **Consumes:** block registry (`RPT.MODEL.DOC`), `ChartSeries` (`RPT.STATS.TREND` contract; LVM for vector group plots), saved views and snapshots (`VIZ.VIEW`, `VIZ.CAPTURE`).

## Decomposition hints

TABLE (table block and column model), CHART (chart block), VIEW (view block and restore), TEXT (text, image, header and footer blocks).

## Open questions

- Chart blocks are P1a in this node, but their main series source (`RPT.STATS`) starts in P1b; P1a can ship table, text, view and image blocks first.

## Children

_None yet: status `open`._
