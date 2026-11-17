---
id: GDT.LOC.DERIV
title: Derived-feature extraction
level: 3
parent: GDT.LOC
kind: module
status: open
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-GDT.LOC-005]
covers: []
depends_on: [KRN.LSQ, KRN.CONS, GDT.RES.RESOLVE]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# GDT.LOC.DERIV: Derived-feature extraction

## Purpose

Produces derived toleranced elements from measured surfaces: axes, derived median lines, median surfaces (median planes) and median points, using the reduction operator the resolved chain names (for example the axis of an associated cylinder, or the locus of section centres for an ISO 1101 derived median line). Orientation, position, coaxiality, symmetry and derived-line straightness all consume these elements, so the derivation rules live in one place.

## Scope

**In:**
- Axis of an associated feature (cylinder, cone) with the association named in the chain
- Derived median line (section centres along an axis) and median surface (pairs of opposite points)
- Median points for symmetry-type evaluations
- Recording the reduction operator's algorithm ID

**Out** (and which node owns it instead):
- Fits (owned by `KRN`)
- Tolerance evaluation of the derived element (owned by the sibling symbol children and `GDT.FORM.STRAIGHT`)

## Requirements

- **REQ-GDT.LOC.DERIV-001** The system shall derive the toleranced element with the reduction operator the resolved chain names and record that operator's algorithm ID in the ResultRecord.
- **REQ-GDT.LOC.DERIV-002** If the chain names a derived element that the point data cannot support (for example too few sections), then the system shall return `derivation_failed` with the reason and compute no tolerance value.
- **REQ-GDT.LOC.DERIV-003** Derived elements shall match every ORS-GDT derived-feature case within its stated tolerance.

## Interfaces

- **Offers:** `derive(element_kind, points, chain) -> DerivedElement` to ORIENT, POS, COAX, SYM and `GDT.FORM.STRAIGHT`.
- **Consumes:** fits and constructions (KRN); chain (GDT.RES.RESOLVE).

## Decomposition hints

AXIS (axis of associated feature; likely a leaf), MLINE (derived median line), MSURF (median surface and points).

## Open questions

- Derived median line and median surface definitions differ by dialect and edition (ISO 1101 versus ASME) and are DMT content (D-009, D-016).

## Children

_None yet: status `open`._
