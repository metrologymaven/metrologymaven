---
id: RPT.BALLOON.SHEET
title: Drawing sheets and drawing revisions
level: 3
parent: RPT.BALLOON
kind: module
status: open
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-RPT.BALLOON-009, REQ-RPT.BALLOON-010]
covers: []
depends_on: [IO.REG, CORE.STO, CORE.PRJ]
cli: [fid report add-drawing-balloon, fid report list-drawing-balloon]
mcp: [report_add_drawing_balloon, report_list_drawing_balloon]
journal: [report.add_drawing_balloon@1]
updated: 2026-11-16
---

# RPT.BALLOON.SHEET: Drawing sheets and drawing revisions

## Purpose

Brings 2D drawings into the project as drawing revisions made of sheets (pages of a PDF or raster images), stored by content digest, and defines the sheet coordinate system that balloons use. Reading the files is IO's job; this module turns the decoded pages into project documents with stable IDs and exact sheet bounds.

## Scope

**In:**
- Drawing revision: drawing number, revision label, part revision link, ordered sheets
- Sheets from multi-page PDF and raster images (PNG, TIFF), stored by SHA-256 digest
- Sheet coordinate system: origin top-left, 0.01 mm integer units; PDF page size from the page box, raster size from pixel count and stated resolution
- Adding a new drawing revision and carrying balloons forward from the previous revision on request

**Out** (and which node owns it instead):
- Decoding PDF and raster files (`IO`; see Open questions)
- Balloons (`RPT.BALLOON.REG`)
- Sheet display, zoom and pan (VIZ)

## Requirements

- **REQ-RPT.BALLOON.SHEET-001** When a drawing file is imported, the system shall create a drawing revision whose sheets are stored by SHA-256 content digest and each have a UUIDv7 sheet ID.
- **REQ-RPT.BALLOON.SHEET-002** Each sheet shall define its bounds in integer 0.01 mm units with the origin at the top-left corner, so that balloon positions do not change with zoom, pan or export.
- **REQ-RPT.BALLOON.SHEET-003** If a raster image carries no resolution, then the system shall require the user to state the sheet size or resolution before balloons can be placed on it.
- **REQ-RPT.BALLOON.SHEET-004** Drawing text extracted from files and all user-entered drawing metadata shall be typed as untrusted content and never interpreted (CONSTITUTION C6).
- **REQ-RPT.BALLOON.SHEET-005** If a drawing file exceeds 200 pages or 500 MiB [unverified limits], then the import shall fail closed with a typed error and journal nothing.
- **REQ-RPT.BALLOON.SHEET-006** When the user adds a new revision of a drawing and asks to carry balloons forward, the system shall copy each balloon's number, link and position to the new revision with a new balloon ID that records its predecessor.

## Interfaces

- **Offers:** `report.add_drawing_balloon@1` (W2; imports a drawing revision for ballooning), `report.list_drawing_balloon@1` (R0); types `DrawingRevision`, `DrawingSheet`; the `SheetInfo` trait used by `RPT.BALLOON.REG`.
- **Consumes:** file decoding through IO readers and approved import roots (`IO.REG`), blob store (`CORE.STO`), project model (`CORE.PRJ`).

## Decomposition hints

IMPORT (drawing revision import), COORD (sheet coordinate system), CARRY (carry balloons forward).

## Open questions

- **Cross-domain:** no IO node names a PDF or raster-image page reader for drawings. IO should own it (format reading is out of RPT); raised here for IO.
- Command names `*_drawing_balloon` are chosen to stay inside the parent's `*_balloon` pattern; `report.import_drawing` would read better.

## Children

_None yet: status `open`._
