---
id: IND.SHEET.HOLE
title: Sheet-metal holes and slots
level: 3
parent: IND.SHEET
kind: module
status: open
phase: P3
size: L
owners: []
claimed_by:
satisfies: [REQ-IND.SHEET-003, REQ-IND.SHEET-005]
covers: [GOM.SPE.01]
depends_on: [IND.SHEET.CTX, SCAN, KRN]
cli: [fid pack create-sheet-hole, fid pack evaluate-sheet-hole]
mcp: [pack_create_sheet_hole, pack_evaluate_sheet_hole]
journal: [pack.create_sheet_hole@1, pack.evaluate_sheet_hole@1]
updated: 2026-11-16
---

# IND.SHEET.HOLE: Sheet-metal holes and slots

## Purpose

Round holes, slots and rectangular holes extracted from scan data on thin sheet: the edge of the cut-out is found in a search region around the nominal, fitted in the plane of the reference surface, and reported as centre position and size with thickness compensation when the scanned skin is not the reference skin.

## Scope

**In:**
- Element types `SheetHole`, `SheetSlot`, `SheetRectHole` referencing a `SheetContext`
- Edge-point extraction around the nominal in a declared search region, with a declared minimum point count
- In-plane fits (circle, slot, rectangle with corner radius) after projection onto the local reference plane
- Thickness compensation through `IND.SHEET.CTX`
- Reporting centre (in-plane and normal components), diameter or length and width, and orientation

**Out** (and which node owns it instead):
- 2D fits (owned by `KRN`)
- Boundary and edge detection in meshes (owned by `SCAN`)
- Position tolerances (owned by `GDT`)
- Batch creation of many holes (owned by `IND.PATTERN`)

## Requirements

- **REQ-IND.SHEET.HOLE-001** The pack shall refuse to create a hole, slot or rectangular hole without a valid `SheetContext` (`E_SHEET_NO_CONTEXT`).
- **REQ-IND.SHEET.HOLE-002** When a hole element is evaluated, the pack shall extract edge points within the declared search region, project them onto the local reference plane and fit the declared shape in that plane.
- **REQ-IND.SHEET.HOLE-003** The pack shall report centre position in the reference-plane frame (two in-plane components and one normal component) and size (diameter, or length and width), applying thickness compensation when the scanned side differs from the reference side.
- **REQ-IND.SHEET.HOLE-004** If fewer edge points than the declared minimum lie within the search region, then the pack shall mark the element not measured and its characteristics `NOT_DECIDABLE`.
- **REQ-IND.SHEET.HOLE-005** On a synthetic hole of known geometry sampled without noise, the reported centre and diameter shall match the true values within 1e-6 m [unverified].

## Interfaces

- **Offers:** element types above; the two commands (create W2, evaluate R1); `SheetHoleResult`.
- **Consumes:** `IND.SHEET.CTX` context and compensation; SCAN edge and boundary extraction; KRN 2D fits.

## Decomposition hints

ELEM (element types and create), EXTRACT (edge points in the search region), FIT (shape fits and reporting).

## Open questions

- Whether to support hole edges detected by the "virtual edge" from surface curvature when the scan has no clean boundary [unverified incumbent behaviour].

## Children

_None yet: status `open`._
