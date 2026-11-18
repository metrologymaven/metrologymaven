---
id: UNC.RDL.GEN.GEOM
title: Partial-coverage and extreme-aspect-ratio families
level: 4
parent: UNC.RDL.GEN
kind: task
status: leaf
phase: P0
size: S
owners: []
claimed_by:
satisfies: [REQ-UNC.RDL.GEN-004]
covers: []
depends_on: [UNC.RDL.GEN.LSNULL]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# UNC.RDL.GEN.GEOM: Partial-coverage and extreme-aspect-ratio families

## Purpose

Least-squares null-space cases whose sampling geometry stresses the fits: partial arcs and caps (small angular coverage), and extreme aspect ratios (long thin cylinders, short wide cylinders, cones with very small or very large half-angles). They reuse the LSNULL construction with different sampling parameters, so the references remain exact by construction.

## Scope

**In:**
- Families `ls.circle.partial`, `ls.sphere.cap`, `ls.cylinder.partial` (coverage 5 to 180 degrees), `ls.cylinder.aspect` (length/diameter 0.05 to 50), `ls.cone.angle` (half-angle 1 to 80 degrees)
- Recording coverage and aspect in each record's case metadata

**Out:**
- The null-space construction (owned by `UNC.RDL.GEN.LSNULL`)

## Requirements

- **REQ-UNC.RDL.GEN.GEOM-001** Each family shall generate cases over its declared parameter grid using the LSNULL construction, and record the coverage angle or aspect ratio in the case metadata.
- **REQ-UNC.RDL.GEN.GEOM-002** If the high-precision refinement for a grid point is ill-conditioned (condition above 1e40), then the family shall skip it and list it as `skipped_ill_posed` in the generation report.

## Acceptance criteria

- **AC-1** Given seed 2, when `ls.circle.partial` is generated, then cases exist at coverages 5, 10, 30, 90 and 180 degrees, each with the coverage in its metadata, and pass format validation.
- **AC-2** Given the mpmath oracle file `fixtures/rdl/gen/geom/oracle_seed2.json`, then references agree to 1e-40 relative.
- **AC-3** Given seed 2 twice, then byte-identical records.

## Design

- **Registry command(s):** none; families registered for `UNC.RDL.GEN.CMD`.
- **Data structures:** `GridSpec` per family.
- **Algorithm:** call the LSNULL builder with restricted sampling domains.
- **Errors:** as LSNULL; skips reported.
- **Determinism:** D0.
- **Performance budget:** each family in under 60 s [unverified until measured].
- **Crate:** `fid-rdl-gen` (module `geom`).

## Test fixtures

`fixtures/rdl/gen/geom/` oracle file for seed 2 (CC-BY-4.0).

## Build prompt

> In crate `fid-rdl-gen`, module `geom`, add families `ls.circle.partial`, `ls.sphere.cap`, `ls.cylinder.partial` (coverage 5 to 180 degrees), `ls.cylinder.aspect` (L/D 0.05 to 50) and `ls.cone.angle` (half-angle 1 to 80 degrees) on top of the LSNULL builder with restricted sampling domains; record coverage or aspect in case metadata; skip grid points whose refinement condition exceeds 1e40 and list them as `skipped_ill_posed`. Tests AC-1..AC-3. CONSTITUTION 8.

## Open questions

- **Gap from research hypotheses (2026-11-17):** add a construction family (intersections, projections, offsets with known exact results) for Jacobian tests in UNC.PROP.LPU.TCONS.
- Grid values are initial choices for review [unverified].

## Children

_Leaf: none._
