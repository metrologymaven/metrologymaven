---
id: IND.SHEET
title: Sheet metal pack
level: 2
parent: IND
kind: subsystem
status: split
phase: P3
size: XL
owners: []
claimed_by:
satisfies: [REQ-IND-001, REQ-IND-002, REQ-IND-003, REQ-IND-004]
covers: [GOM.SPE.01, GOM.SPE.02, GOM.SPE.03, PW.IND.03]
depends_on: [IND.CONTRACT, SCAN, ALN, GDT]
cli: [fid pack *-sheet*]
mcp: [pack_*_sheet*]
journal: [pack.*_sheet*]
updated: 2026-11-16
---

# IND.SHEET: Sheet metal pack

## Purpose

Inspection elements that only make sense for thin formed parts: holes and slots measured on the sheet with thickness and material-side awareness, discrete trim and edge points with in-plane and out-of-plane components, springback assessment of formed panels, and hem geometry. These are what body-in-white and stamping users expect from scan inspection.

## Scope

**In:**
- Sheet context: material side, nominal thickness and the reference surface (CAD side) used by every element
- Sheet-metal hole, slot and rectangular-hole elements from scan data, projected onto the reference surface
- Discrete trim and edge points with in-plane (along the surface) and normal deviation components
- Surface points with sheet-thickness compensation
- Springback assessment: deviation of the formed part against nominal under a declared clamping alignment, with per-region summaries
- Hem elements: hem height, hem width, hem radius, and the gap between hem flange and inner panel
- Sheet metal report template

**Out** (and which node owns it instead):
- Continuous edge and trim-line deviation computation (owned by `SCAN`; this node consumes it)
- Generic surface comparison (owned by `SCAN`)
- Clamping and RPS alignments (owned by `ALN`)
- Position tolerances on holes (owned by `GDT`)
- Batch recognition and instantiation of repeated holes (owned by `IND.PATTERN`)
- Gap and flush between two panels (owned by `IND.GAPFL`)
- Die-face compensation or morphing of CAD from springback results (not covered; see Open questions)

## Requirements

- **REQ-IND.SHEET-001** The sheet metal pack shall register its commands, element types, checks and report templates only through the `IND.CONTRACT` manifest.
- **REQ-IND.SHEET-002** Every sheet metal element shall reference a sheet context that names the material side and nominal thickness, and the pack shall refuse to create an element without one.
- **REQ-IND.SHEET-003** When a hole, slot or rectangular hole is extracted from scan data, the sheet metal pack shall report its centre position and size in the plane of the reference surface, compensating for nominal thickness when the scanned side differs from the reference side.
- **REQ-IND.SHEET-004** When a trim or edge point is evaluated, the sheet metal pack shall report its deviation as two components, one in the tangent plane perpendicular to the nominal edge and one along the surface normal.
- **REQ-IND.SHEET-005** If fewer scan points than the declared minimum lie within the search region of a sheet metal element, then the pack shall mark the element as not measured and its characteristics as `NOT_DECIDABLE`.
- **REQ-IND.SHEET-006** When springback is assessed, the sheet metal pack shall report deviation statistics per declared region under a named, journaled clamping alignment, and shall record which alignment was used in each ResultRecord.
- **REQ-IND.SHEET-007** When a hem is evaluated from a section through the hem, the sheet metal pack shall report hem height, hem width, outer radius and flange-to-panel gap.
- **REQ-IND.SHEET-008** The sheet metal pack manifest shall declare its element definitions as versioned method IDs, citing any OEM or national standard by designator and edition only, and shall name a maintainer.
- **REQ-IND.SHEET-009** The sheet metal pack shall ship reference datasets covering holes, slots, trim points, a springback panel and a hem section, with synthetic data whose expected values are known to within 1e-6 m [unverified].

## Interfaces

- **Offers:** element types `SheetContext`, `SheetHole`, `SheetSlot`, `TrimPoint`, `SheetSurfacePoint`, `Hem`; commands such as `pack.create_sheet_hole@1` (W2), `pack.evaluate_sheet_springback@1` (R1); sheet metal report template.
- **Consumes:** SCAN deviations, sections and edge deviations; ALN clamping/RPS alignments; GDT position evaluation; IND.CONTRACT manifest and provenance.

## Decomposition hints

CTX (sheet context), HOLE (holes and slots), TRIM (trim and edge points), SPRING (springback), HEM (hem), DATA, TPL.

## Split rationale

The split is **one child per sheet element family**, plus the shared sheet context and the pack's declarations and template. The context (CTX) is its own child because every element needs the same material side, thickness and compensation rule; making it a dependency avoids each element re-implementing compensation. Holes (HOLE) and trim/surface points (TRIM) are separate because holes need shape fits while points need only local deviation decomposition. Springback (SPRING) is a region-statistics evaluation on top of SCAN and ALN, and hems (HEM) are section-based.

Alternatives considered:
- **Split by data source** (scan elements vs section elements). Rejected: it would put holes and hems together although they share no algorithm.
- **Fold CTX into each element.** Rejected: duplicated compensation logic and a risk of inconsistent sign conventions.

## Open questions

- Die compensation (morphing CAD to counter springback) is offered by some incumbents [unverified] but borders on reverse-engineering surface modelling, which FID puts out of scope. Opened as decision **D-022**; `IND.SHEET.SPRING` excludes it until decided.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| IND.SHEET.CTX | Sheet context element (leaf) | 001 002 | GOM.SPE.01, PW.IND.03 |
| IND.SHEET.HOLE | Sheet-metal holes and slots | 003 005 | GOM.SPE.01 |
| IND.SHEET.TRIM | Trim, edge and surface points | 004 005 | GOM.SPE.01, PW.IND.03 |
| IND.SHEET.SPRING | Springback assessment | 006 | GOM.SPE.02 |
| IND.SHEET.HEM | Hem evaluation | 005 007 | GOM.SPE.03, PW.IND.03 |
| IND.SHEET.DATA | Sheet metal method declarations and reference datasets | 008 009 | none |
| IND.SHEET.TPL | Sheet metal report template | 001 | none |
