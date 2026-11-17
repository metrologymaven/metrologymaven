---
id: IND.SHEET.TRIM
title: Trim, edge and surface points
level: 3
parent: IND.SHEET
kind: module
status: open
phase: P3
size: L
owners: []
claimed_by:
satisfies: [REQ-IND.SHEET-004, REQ-IND.SHEET-005]
covers: [GOM.SPE.01, PW.IND.03]
depends_on: [IND.SHEET.CTX, SCAN]
cli: [fid pack create-sheet-trim-point, fid pack evaluate-sheet-trim-point]
mcp: [pack_create_sheet_trim_point, pack_evaluate_sheet_trim_point]
journal: [pack.create_sheet_trim_point@1, pack.evaluate_sheet_trim_point@1]
updated: 2026-11-16
---

# IND.SHEET.TRIM: Trim, edge and surface points

## Purpose

Discrete inspection points on sheet parts: trim and edge points whose deviation is split into an in-plane component (across the edge, in the tangent plane) and a normal component, and surface points whose normal deviation is compensated for sheet thickness. These are the most numerous elements on a body-in-white inspection plan.

## Scope

**In:**
- Element types `TrimPoint` and `SheetSurfacePoint` referencing a `SheetContext`
- Trim point: nominal edge point, edge tangent and surface normal; deviation decomposed into in-plane (perpendicular to the edge in the tangent plane) and normal components
- Surface point: normal deviation with thickness compensation
- Search region and minimum point count per element

**Out** (and which node owns it instead):
- Continuous edge and trim-line deviation (owned by `SCAN`; this node consumes it)
- Gap and flush between panels (owned by `IND.GAPFL`)

## Requirements

- **REQ-IND.SHEET.TRIM-001** The pack shall refuse to create a trim or surface point without a valid `SheetContext`.
- **REQ-IND.SHEET.TRIM-002** When a trim point is evaluated, the pack shall report its deviation as an in-plane component perpendicular to the nominal edge within the tangent plane and a component along the surface normal, with signs defined by the nominal edge's outward direction and the reference normal.
- **REQ-IND.SHEET.TRIM-003** When a surface point is evaluated, the pack shall report the normal deviation relative to the reference surface after thickness compensation.
- **REQ-IND.SHEET.TRIM-004** If fewer scan points than the declared minimum lie within the search region, then the pack shall mark the element not measured and its characteristics `NOT_DECIDABLE`.
- **REQ-IND.SHEET.TRIM-005** On a synthetic plate with an edge offset by a known in-plane and normal amount, the two reported components shall match within 1e-6 m [unverified].

## Interfaces

- **Offers:** element types above; create (W2) and evaluate (R1) commands.
- **Consumes:** `IND.SHEET.CTX`; SCAN edge deviations and local point neighbourhoods.

## Decomposition hints

TRIMPT (trim and edge points), SURFPT (surface points). Both are leaf candidates once SCAN's edge interface is fixed.

## Open questions

- Edge detection on the scan is SCAN's; whether trim points also need a local "edge from section" fallback should be settled with SCAN.

## Children

_None yet: status `open`._
