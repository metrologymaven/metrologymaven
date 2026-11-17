---
id: GEO.CAD.NOMINAL
title: Nominal element generation from CAD faces and edges
level: 3
parent: GEO.CAD
kind: module
status: open
phase: P1b
size: L
owners: []
claimed_by:
satisfies: [REQ-GEO.CAD-008, REQ-GEO.CAD-009]
covers: [GOM.CAD.03]
depends_on: [GEO.CAD.TOPO, GEO.NURBS.ANALYTIC, GEO.NURBS.INVERT]
cli: [fid geo extract-nominal]
mcp: [geo_extract_nominal]
journal: [geo.extract_nominal@1]
updated: 2026-11-16
---

# GEO.CAD.NOMINAL: Nominal element generation from CAD faces and edges

## Purpose

Produces typed nominal geometric elements (plane, cylinder, cone, sphere, torus, line, circle, point) from CAD faces and edges, with exact parameters, so that inspection features, alignments and GD&T checks reference the designer's intent rather than a fit to tessellation. It recognises analytic shapes stored as analytic surfaces directly, recognises analytic shapes encoded as NURBS within a recognition tolerance, merges faces that belong to one primitive, and says plainly when a face is freeform.

## Scope

**In:**
- Direct mapping of analytic faces and edges to nominal elements, with placement in the body's frame
- Recognition of NURBS-encoded planes, cylinders, cones, spheres, tori, lines and circles within a recognition tolerance, recording the maximum deviation found
- Merging several faces of one primitive (for example a cylinder split into two half faces) into one nominal element
- Typed `NotRecognised` results for freeform faces
- `geo.extract_nominal@1` (W2): create nominal elements registered as derived from their faces
- GOM.CAD.03 (generate nominal elements from CAD faces)

**Out** (and which node owns it instead):
- Topology and IDs (owned by `GEO.CAD.TOPO`)
- Fitting features to measured points (owned by `KRN`)
- Inspection feature definitions and characteristics (owned by `GDT` and `CMM`)

## Requirements

- **REQ-GEO.CAD.NOMINAL-001** When a face or edge carries analytic geometry, the system shall produce the matching nominal element with parameters copied exactly from the CAD definition (no refit).
- **REQ-GEO.CAD.NOMINAL-002** When a face carries a NURBS surface that matches an analytic primitive within the recognition tolerance (default 1e-7 m [unverified]), the system shall produce that primitive and record the maximum deviation measured on a stated sample grid.
- **REQ-GEO.CAD.NOMINAL-003** If no analytic primitive matches within the recognition tolerance, then the system shall return `NotRecognised { face }` and shall not return an approximate primitive.
- **REQ-GEO.CAD.NOMINAL-004** When several selected faces describe one primitive within tolerance (same axis, radius or plane), the system shall offer one merged nominal element referencing all of them.
- **REQ-GEO.CAD.NOMINAL-005** `geo.extract_nominal@1` shall be class W2, produce one journal entry, and register each nominal element as derived from its source faces so it recomputes when the CAD is replaced.
- **REQ-GEO.CAD.NOMINAL-006** Recognition shall be deterministic class D0 or D1.

## Interfaces

- **Offers:** `NominalElement` (or the shared feature type, see Open questions), `recognise(face, tol) -> Recognition`, command `geo.extract_nominal@1` (W2).
- **Consumes:** GEO.CAD.TOPO faces and edges; GEO.NURBS analytic types and evaluation; GEO.SEL selections of faces (as input parameters).

## Decomposition hints

DIRECT (analytic faces and edges), RECOG (NURBS-encoded recognition and deviation), MERGE (multi-face primitives), CMD (command).

## Open questions

- Where the typed nominal element types live: GEO, or a shared feature model owned by KRN or GDT that both nominal generation and fitting use. A single type is preferable; the owner should be agreed with KRN.
- The recognition tolerance default and sample grid need evidence from real STEP corpora (IO.CORP).

## Children

_None yet: status `open`._
