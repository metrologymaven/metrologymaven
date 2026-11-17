---
id: UNC.ORS.PROJ
title: ORS-PROJ: projection onto CAD faces
level: 3
parent: UNC.ORS
kind: module
status: open
phase: P1b
size: L
owners: []
claimed_by:
satisfies: [REQ-UNC.ORS-002, REQ-UNC.ORS-003]
covers: []
depends_on: [UNC.ORS.TRACE]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# UNC.ORS.PROJ: ORS-PROJ: projection onto CAD faces

## Purpose

Cases for projecting measured points onto nominal geometry: analytic faces, trimmed NURBS faces, seams and edges, with expected foot points and signed distances known in closed form. Surface comparison and profile evaluation depend on projection being right.

## Scope

**In:**
- Analytic faces (plane, cylinder, sphere, cone, torus)
- Trimmed NURBS faces with known parameterisation
- Points near seams, edges and trim boundaries

**Out:**
- Case format, citation checks and versioning (owned by `UNC.ORS.TRACE`)
- Running the suite (owned by `UNC.ORS.RUN`)
- The evaluation under test (owned by `GDT`, `ALN`, `SCAN` or `GEO` as applicable)

## Requirements

- **REQ-UNC.ORS.PROJ-001** Each nominal face type in the P1b geometry scope shall have at least one projection case with a closed-form expected foot point and signed distance.
- **REQ-UNC.ORS.PROJ-002** Cases near seams and trim boundaries shall state the expected face assignment.
- **REQ-UNC.ORS.PROJ-003** Cases shall state tolerances on distance (m) and foot-point parameters.

## Interfaces

- **Offers:** suite `ors-proj@<semver>` in `ors-case/v1` format.
- **Consumes:** case format and checker (UNC.ORS.TRACE); DMT rows (GDT.RES) where cases cite them; generators from `UNC.RDL.GEN` for constructed geometry.

## Decomposition hints

ANALYTIC, NURBS, EDGE. Each sub-suite becomes a content task once its cases have expert authors.

## Open questions

- Interpretation content (expected results that depend on reading a standard, and every DMT row citation) is authored by paid or credentialed GD&T experts and signed off by two reviewers, per D-009; agents may draft structure, generators and closed-form mathematics only.
- Expected results that cannot be derived in closed form need an independent oracle result recorded as a cross-check [unverified: which cases].

## Children

_None yet: status `open`._
