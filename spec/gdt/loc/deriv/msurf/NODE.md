---
id: GDT.LOC.DERIV.MSURF
title: Derived median surface and median points
level: 4
parent: GDT.LOC.DERIV
kind: feature
status: open
phase: P1b
size: L
owners: []
claimed_by:
satisfies: [REQ-GDT.LOC.DERIV-001, REQ-GDT.LOC.DERIV-002, REQ-GDT.LOC.DERIV-003]
covers: []
depends_on: [GDT.LOC.DERIV.TYPES, KRN.LSQ]
uses_artifacts: [GDT.RES.ROWASME, GDT.RES.ROWISO, UNC.ORS.GDT]
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# GDT.LOC.DERIV.MSURF: Derived median surface and median points

## Purpose

Produces the derived median surface of a width feature (midpoints of pairs of opposite points on its two faces) and the median points used by symmetry and legacy concentricity evaluations. How opposite points are paired (along which direction, against which reference), and how median points of a revolute feature are formed, are interpretation content of the DMT rows (D-009, D-016). Symmetry (`GDT.LOC.SYM`) and concentricity (`GDT.LOC.COAX`) consume it, both P1b.

## Scope

**In:**
- Opposite-point pairing per the chain for width features and revolute features
- `MedianSurface{points}` and `MedianPoints{points}` with algorithm IDs
- `derivation_failed{no_opposite_point}` and `too_few_points` handling

**Out** (and which node owns it instead):
- Axes and median lines (owned by `GDT.LOC.DERIV.AXIS`, `GDT.LOC.DERIV.MLINE`)
- Definitions as content (owned by DMT rows via `GDT.RES`)

## Requirements

- **REQ-GDT.LOC.DERIV.MSURF-001** The system shall pair opposite points exactly as the resolved chain names and return the median points with the algorithm IDs used.
- **REQ-GDT.LOC.DERIV.MSURF-002** If fewer pairs than the chain's minimum can be formed, then the system shall return `derivation_failed{reason: too_few_points}`, counting unpaired points.
- **REQ-GDT.LOC.DERIV.MSURF-003** Median surfaces and points shall match every ORS-GDT median-surface and median-point case within its stated tolerance.

## Interfaces

- **Offers:** `derive()` implementations producing `DerivedElement::MedianSurface` and `DerivedElement::MedianPoints`.
- **Consumes:** type and registry (GDT.LOC.DERIV.TYPES); reference fits (KRN.LSQ); rows and ORS-GDT cases (read as data).

## Decomposition hints

WIDTH (median surface of a width feature), REVOL (median points of a revolute feature); possibly shared pairing mechanics with `GDT.DIM.SIZEASME.LOCAL` when rows name the same operator.

## Open questions

- Not a leaf: pairing rules are DMT content (D-009, D-016). Consider sharing the pairing operators with the two-point size modules once rows exist.

## Children

_None yet: status `open`._
