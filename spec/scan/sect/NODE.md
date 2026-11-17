---
id: SCAN.SECT
title: Sections, section comparison and cloud slicing
level: 2
parent: SCAN
kind: subsystem
status: split
phase: P1b
size: L
owners: []
claimed_by:
satisfies: [REQ-SCAN-001, REQ-SCAN-002, REQ-SCAN-004]
covers: [GOM.SRF.04, GOM.SRF.05, PW.CMP.02, SA.SCN.03]
depends_on: [SCAN.CMP, GEO, KRN]
cli: [fid compare section-*]
mcp: [compare_section_*]
journal: [compare.section_*]
updated: 2026-11-16
---

# SCAN.SECT: Sections, section comparison and cloud slicing

## Purpose

Cuts actual data (cloud or mesh) and nominal CAD with planes or other section surfaces, producing section curves; compares actual against nominal sections and reports whiskers and min/max/RMS statistics. It also slices point clouds into bands for fitting 2D features, the SpatialAnalyzer-style "slice and fit" workflow. It is a separate unit because sections are a 2D derived element with their own creation parameters, statistics and reporting, even though their deviation values come from SCAN.CMP's primitive.

## Scope

**In:**
- Section definition: single plane, parallel plane series with spacing, radial planes about an axis, and planes from CAD or construction elements
- Section curves through meshes (polyline) and through CAD (exact curve, tessellated within a stated tolerance)
- Section of a point cloud as a slab of stated thickness projected to the plane
- Section comparison: deviation of the actual section against the nominal section, in plane
- Whisker values at a stated spacing along the nominal section, and min, max, mean and RMS statistics per section
- Hand-off of slice points to KRN for 2D fitting (line, circle, and curve fits)

**Out** (and which node owns it instead):
- Drawing whiskers and section views (owned by `VIZ`)
- The deviation primitive itself (owned by `SCAN.CMP`)
- 2D fitting algorithms (owned by `KRN`)
- Line-profile tolerance evaluation (owned by `GDT`)
- Airfoil section analysis (owned by `IND`)

## Requirements

- **REQ-SCAN.SECT-001** When a section is created, the system shall create a section element version holding its definition, the digests of the cut source elements and the resulting curves, without modifying the sources.
- **REQ-SCAN.SECT-002** The system shall create sections from a single plane, a parallel series with spacing in metres, and a radial series with angular step about an axis.
- **REQ-SCAN.SECT-003** A CAD section curve shall lie within a stated chordal tolerance in metres of the exact intersection of the plane and the B-rep.
- **REQ-SCAN.SECT-004** A cloud section shall include exactly the points within half the stated slab thickness of the plane, projected onto it, and shall report the point count.
- **REQ-SCAN.SECT-005** Section comparison shall compute in-plane signed deviations of actual against nominal on the CPU, using SCAN.CMP's deviation primitive restricted to the section plane.
- **REQ-SCAN.SECT-006** For each compared section, the system shall report whisker values at a stated spacing along the nominal curve, and the minimum, maximum, mean and RMS deviation, with `NO_MATCH` stations excluded and counted.
- **REQ-SCAN.SECT-007** When a slice is fitted, the system shall pass the slice points to a KRN fit command and link the fitted feature to the slice element so it recomputes when the slice changes.

## Interfaces

- **Offers:** `compare.section_*` commands (for example `compare.section_create@1`, `compare.section_compare@1`); section and section-comparison elements; whisker arrays for VIZ and RPT.
- **Consumes:** SCAN.CMP (deviation primitive), GEO (plane intersection with mesh and CAD, tessellation), KRN (2D fits).

## Decomposition hints

DEF (section definitions and series), CUT (mesh, CAD and cloud intersection), WHISK (section comparison, whiskers and statistics), SLICE (cloud slab slicing and fit hand-off).

## Open questions

- Should section surfaces other than planes (cylindrical, curve-normal) be in P1b scope? Proposed: planes only for P1b; others later.

## Split rationale

The split follows the **section pipeline**: define the planes (DEF), cut sources into section curves (CUT), compare actual against nominal with whiskers and statistics (WHISK), and the cloud-specific slab slicing with its fit hand-off (SLICE). Nominal CAD section curves are KRN.CONS constructions (boundary agreed at review); CUT requests and stores them and owns only the actual-data cuts. All four children are leaves in `fid-scan-sect`; DEF is an internal task with no command of its own.

Alternatives considered:
- **One leaf for create and compare.** Rejected: the section element is reused by reports, slicing and fits without comparison, and the compare step has its own parameters and statistics.
- **Put cloud slab extraction in CUT.** Rejected: slicing for fits is a separate workflow (SA.SCN.03) with its own 2D frame and recompute link; CUT reuses SLICE's function instead.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| SCAN.SECT.DEF | Section plane definitions and series | 001 002 | PW.CMP.02 |
| SCAN.SECT.CUT | Section creation through meshes, clouds and CAD | 001 003 | GOM.SRF.05, PW.CMP.02 |
| SCAN.SECT.WHISK | Section comparison, whiskers and statistics | 005 006 | GOM.SRF.04, GOM.SRF.05 |
| SCAN.SECT.SLICE | Cloud slab slicing and fit hand-off | 004 007 | SA.SCN.03 |
