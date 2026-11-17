---
id: SCAN.THK
title: Wall and material thickness
level: 2
parent: SCAN
kind: subsystem
status: split
phase: P1b
size: L
owners: []
claimed_by:
satisfies: [REQ-SCAN-001, REQ-SCAN-002, REQ-SCAN-005]
covers: [GOM.SRF.07, GOM.SRF.08, PW.CMP.05]
depends_on: [SCAN.CMP, GEO]
cli: [fid compare thickness-*]
mcp: [compare_thickness_*]
journal: [compare.thickness_*]
updated: 2026-11-16
---

# SCAN.THK: Wall and material thickness

## Purpose

Computes thickness from scan data: wall thickness of a closed or two-sided body measured from one surface to the opposite surface, and material thickness of sheet parts measured from a single scanned side against the nominal opposite side. It is separate because thickness is a per-point field defined by ray or sphere queries through the material, not by actual-to-nominal deviation, and has its own method choices and failure modes.

## Scope

**In:**
- Wall thickness on a mesh or cloud that captures both sides, by ray casting along the inward normal to the opposite surface
- Wall thickness by maximum inscribed sphere at each point
- Material thickness from one measured side to the nominal opposite side
- Minimum and maximum thickness limits for classification
- Thickness field element and min/max/mean statistics per region
- Thickness comparison points that produce ResultRecords via SCAN.CMP's comparison-point model

**Out** (and which node owns it instead):
- Actual-to-nominal surface deviation (owned by `SCAN.CMP`)
- CT-based wall thickness (out of scope; see `seeds/out-of-scope.md`)
- Rendering thickness color maps (owned by `VIZ`)

## Requirements

- **REQ-SCAN.THK-001** When a thickness analysis runs, the system shall create a thickness field element recording the method, parameters, source digests and alignment ID, without modifying any source.
- **REQ-SCAN.THK-002** The system shall compute wall thickness per point by ray casting along the inward normal to the first opposite surface hit, within a stated maximum thickness in metres and a stated maximum angle between the ray and the opposite surface normal.
- **REQ-SCAN.THK-003** The system shall compute wall thickness per point as the diameter of the largest inscribed sphere touching that point, as an alternative method selected by parameter.
- **REQ-SCAN.THK-004** The system shall compute material thickness from measured points on one side to the nominal opposite side, in a stated alignment, using SCAN.CMP's deviation primitive.
- **REQ-SCAN.THK-005** If no opposite surface is found within the maximum thickness, then the system shall mark that point `NO_MATCH` and shall not report a value for it.
- **REQ-SCAN.THK-006** Thickness values of record shall be computed on the CPU with determinism class D0, and each method shall be documented with its definition of thickness.
- **REQ-SCAN.THK-007** On a synthetic fixture of a plate with a known stepped thickness, each method shall report the known thicknesses within a published tolerance [unverified].

## Interfaces

- **Offers:** `compare.thickness_*` commands (for example `compare.thickness_wall@1`, `compare.thickness_material@1`); thickness field elements; thickness comparison points.
- **Consumes:** SCAN.CMP (deviation primitive, comparison-point model), GEO (ray and closest-point queries, spatial index).

## Decomposition hints

RAY (ray-cast wall thickness), SPHERE (inscribed-sphere wall thickness), MAT (material thickness to nominal), TSTAT (thickness classification and statistics).

## Open questions

- None.

## Split rationale

The split is **one child per definition of thickness**, plus one child for the field they all write. RAY (ray to the opposite surface; owns `compare.thickness_wall@1`), SPHERE (largest inscribed sphere; a method of the same command, selected by parameter as REQ-SCAN.THK-003 requires), and MAT (one measured side against the nominal opposite side, via SCAN.CMP's primitive) each document their own definition ID. TFIELD owns the `ThicknessField` element, min/max limit classification, statistics and thickness comparison points, so the three methods report identically. All four are leaves in `fid-scan-thk`.

Alternatives considered:
- **One leaf for both wall-thickness methods.** Rejected: the inscribed-sphere iteration has its own convergence and failure modes and is independently testable.
- **Put limit classification in SCAN.CMP.BAND.** Partly adopted: colour bands for thickness maps use BAND, while the below/within/above limit counts and region statistics stay with the field here.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| SCAN.THK.TFIELD | Thickness field, limits, statistics and thickness points | 001 006 | PW.CMP.05 |
| SCAN.THK.RAY | Ray-cast wall thickness | 001 002 005 006 007 | GOM.SRF.07, PW.CMP.05 |
| SCAN.THK.SPHERE | Inscribed-sphere wall thickness method | 003 005 006 007 | GOM.SRF.07 |
| SCAN.THK.MAT | Material thickness to the nominal opposite side | 001 004 005 006 007 | GOM.SRF.08, PW.CMP.05 |
