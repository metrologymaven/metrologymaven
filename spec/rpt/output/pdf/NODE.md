---
id: RPT.OUTPUT.PDF
title: PDF renderer
level: 3
parent: RPT.OUTPUT
kind: module
status: open
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-RPT.OUTPUT-001, REQ-RPT.OUTPUT-002, REQ-RPT.OUTPUT-005, REQ-RPT.OUTPUT-008, REQ-RPT.OUTPUT-009]
covers: [GOM.RPT.06, PW.RPT.04, SA.RPT.03]
depends_on: [RPT.OUTPUT.META, RPT.MODEL.BIND, VIZ.CAPTURE]
cli: [fid report export-pdf]
mcp: [report_export_pdf]
journal: [report.export_pdf@1]
updated: 2026-11-16
---

# RPT.OUTPUT.PDF: PDF renderer

## Purpose

Renders a resolved report to PDF: page layout (shared with the designer preview), text shaping with embedded font subsets, tables, vector charts, embedded view snapshots and drawing sheets, and metadata from `RPT.OUTPUT.META`. It is where byte reproducibility is decided: the same resolved report and renderer version must give byte-identical files.

## Scope

**In:**
- Page layout engine shared with `RPT.DESIGN.PREVIEW` (row heights, page breaks)
- Text shaping and font subsetting with all fonts embedded
- Vector drawing of tables, chart series and balloon overlays; raster embedding of view snapshots and drawing sheets
- Deterministic PDF serialisation (fixed object order, no timestamps, deterministic IDs)
- `report.export_pdf@1` command (W1, `fs_effect: write_root`) through the export sink

**Out** (and which node owns it instead):
- Spreadsheet and CSV (`RPT.OUTPUT.SHEET`)
- Metadata record and export-root writes (`RPT.OUTPUT.META`)
- Snapshot rendering (`VIZ.CAPTURE`)
- Signing PDFs (`TRUST`)

## Requirements

- **REQ-RPT.OUTPUT.PDF-001** The renderer shall embed every font it uses, as subsets, and shall not reference system fonts.
- **REQ-RPT.OUTPUT.PDF-002** When the same resolved report is rendered twice with the same renderer version, the two PDF files shall be byte-identical.
- **REQ-RPT.OUTPUT.PDF-003** The PDF shall contain no creation time, host name or random file ID; its document ID shall be derived from the resolved-report digest.
- **REQ-RPT.OUTPUT.PDF-004** Every number in the PDF shall be the exact text produced by the formatter layer for that value, unchanged.
- **REQ-RPT.OUTPUT.PDF-005** If a block kind has no PDF renderer, then the export shall fail with error `report.unsupported_block` naming the block ID and write no file.
- **REQ-RPT.OUTPUT.PDF-006** The PDF shall carry the `RPT.OUTPUT.META` record in its document metadata.
- **REQ-RPT.OUTPUT.PDF-007** Rendering a 50-page report with 20 images shall complete in under 10 s on reference hardware [unverified].

## Interfaces

- **Offers:** `report.export_pdf@1` (W1, `write_root`); the page layout engine used by `RPT.DESIGN.PREVIEW`.
- **Consumes:** `ResolvedReport` (`RPT.MODEL.BIND`), export envelope (`RPT.OUTPUT.META`), snapshot images (`VIZ.CAPTURE`), style sets and fonts (`RPT.DESIGN.STYLE`).

## Decomposition hints

LAYOUT (page layout and breaks), TEXT (shaping and font subsetting), DRAW (tables, charts, images), WRITE (deterministic serialisation and metadata), CMD (export command).

## Open questions

- PDF/A conformance (ISO 19005) for archiving is likely wanted by regulated users [unverified]; not required yet.
- Choice of PDF and text-shaping libraries must pass the D-001 licence allowlist and the determinism test.

## Children

_None yet: status `open`._
