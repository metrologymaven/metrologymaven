---
id: UNC.ORS.PROJ.ANALYTIC
title: Projection cases on analytic faces
level: 4
parent: UNC.ORS.PROJ
kind: task
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-UNC.ORS.PROJ-001, REQ-UNC.ORS.PROJ-003]
covers: []
depends_on: [UNC.ORS.TRACE, UNC.RDL.GEN]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# UNC.ORS.PROJ.ANALYTIC: Projection cases on analytic faces

## Purpose

Generates projection cases on untrimmed or simply trimmed analytic faces (plane, cylinder, sphere, cone, torus): choose a foot point and the outward normal there, offset along the normal by a signed distance smaller than the local minimum radius of curvature, so the expected foot point, face parameters and signed distance are exact.

## Scope

**In:** generator `ors_proj::analytic`; per face type ≥ 50 points with signed distances in ±10 mm, including points inside and outside, near the axis of cylinders and cones (but not on it) and near torus inner equator; expected foot point (m), face parameters (u, v) and signed distance (m) with tolerances 1e-12 m and 1e-12 in parameters.
**Out:** NURBS faces (owned by `UNC.ORS.PROJ.NURBS`); seams, edges and trim boundaries (owned by `UNC.ORS.PROJ.EDGE`).

## Requirements

- **REQ-UNC.ORS.PROJ.ANALYTIC-001** For each of plane, cylinder, sphere, cone and torus, the generator shall emit at least 50 points with exact expected foot point, parameters and signed distance.
- **REQ-UNC.ORS.PROJ.ANALYTIC-002** Every offset shall be smaller in magnitude than the minimum principal radius of curvature at the foot point by at least 1 %, so that the foot point is the unique nearest point on the face.
- **REQ-UNC.ORS.PROJ.ANALYTIC-003** Every case shall state tolerances of 1e-12 m on distance and foot point and 1e-12 on parameters.

## Acceptance criteria

- **AC-1** Given seed `0x50524A31`, then two runs give identical files and the suite check reports zero findings.
- **AC-2** Given every case, the 50-digit reference script confirms that the foot point lies on the face and the offset vector is parallel to the normal within 1e-30.
- **AC-3** Given the torus cases, none lies within 1 % of the inner-equator curvature limit.

## Design

- **Registry command(s):** none; tooling generator (`ors-gen`), aim `geo.project_points@1`.
- **Algorithm:** pick (u, v) and signed distance from the seed; compute foot point and normal in closed form; offset; round input to 16 digits and recompute the exact foot point for the rounded point at 50 digits.
- **Errors:** generator failures abort the build; no partial suite is written.
- **Determinism:** D0.
- **Performance budget:** generation under 30 s.
- **Crate:** `fid-ors` (Tooling; links no product crate).

## Test fixtures

`fixtures/ors/proj/`: the generated cases (project-authored, CC-BY-4.0); no standards text and no third-party CAD files.

## Build prompt

> In tooling crate `fid-ors`, add generator `ors_proj::analytic` (`ors-gen proj-analytic`), seeded: for plane, cylinder, sphere, cone and torus, ≥ 50 points each from a chosen (u, v) and signed offset (|offset| < 0.99 × minimum principal radius), rounded to 16 digits, with the foot point, parameters and signed distance recomputed at 50 digits; tolerances 1e-12 m and 1e-12 in parameters; aim `geo.project_points@1`. Tests AC-1..AC-3. Every case: `citations: none`, `basis: closed_form_math`, `dialect: none`; byte-reproducible per seed; the suite must pass `rdl.check_suite@1`. Tooling layer: link no product crate. No panic or `unwrap`.

## Open questions

- Parameterisation conventions (u, v ranges and seam location) must match `GEO.NURBS.ANALYTIC` when that leaf is built.

## Children

_Leaf: none._
