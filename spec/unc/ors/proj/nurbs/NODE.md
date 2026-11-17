---
id: UNC.ORS.PROJ.NURBS
title: Projection cases on trimmed NURBS faces
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

# UNC.ORS.PROJ.NURBS: Projection cases on trimmed NURBS faces

## Purpose

Generates projection cases on NURBS faces whose geometry is known exactly: rational quadratic surfaces that represent cylinders, spheres and tori exactly, and polynomial Bézier and B-spline patches of degree 2 and 3 where the foot point is chosen first and the point is offset along the exact normal within the local curvature limit, then checked at 50 digits to be the global nearest point on the trimmed face.

## Scope

**In:** generator `ors_proj::nurbs`; exact-representation NURBS faces and freeform patches with simple trimming loops; ≥ 50 points per surface type; expected foot point, (u, v) and signed distance; tolerances 1e-10 m on distance and foot point, 1e-10 on parameters [unverified: adequacy for Newton-based projection].
**Out:** analytic faces (owned by `UNC.ORS.PROJ.ANALYTIC`); points near trims and seams (owned by `UNC.ORS.PROJ.EDGE`).

## Requirements

- **REQ-UNC.ORS.PROJ.NURBS-001** For each surface type in Scope, the generator shall emit at least 50 points whose expected foot point is confirmed at 50 digits as the global nearest point on the trimmed face.
- **REQ-UNC.ORS.PROJ.NURBS-002** Every foot point shall lie at least 1 mm inside the trimming loop, so that face assignment is not at issue.
- **REQ-UNC.ORS.PROJ.NURBS-003** Every case shall state tolerances of 1e-10 m on distance and foot point and 1e-10 on parameters.

## Acceptance criteria

- **AC-1** Given seed `0x50524A32`, then two runs give identical files and the suite check reports zero findings.
- **AC-2** Given every freeform case, a 50-digit global search on a 10^4-sample grid plus local refinement finds no nearer point on the face.
- **AC-3** Given the exact-cylinder NURBS cases, the expected distances equal the analytic cylinder distance within 1e-14 m.

## Design

- **Registry command(s):** none; tooling generator (`ors-gen`), aim `geo.project_points@1`.
- **Algorithm:** build surfaces with exact rational control nets; choose (u, v) inside the trim with 1 mm margin; offset along the exact normal; verify global nearest point at 50 digits; round and recompute.
- **Errors:** generator failures abort the build; no partial suite is written.
- **Determinism:** D0.
- **Performance budget:** generation under 30 s.
- **Crate:** `fid-ors` (Tooling; links no product crate).

## Test fixtures

`fixtures/ors/proj/`: the generated cases (project-authored, CC-BY-4.0); no standards text and no third-party CAD files.

## Build prompt

> In tooling crate `fid-ors`, add generator `ors_proj::nurbs` (`ors-gen proj-nurbs`), seeded: exact rational quadratic cylinders, spheres and tori and degree 2 and 3 freeform patches with simple trims; ≥ 50 points per type with foot points ≥ 1 mm inside the trim, offset along the exact normal within the curvature limit, verified as global nearest at 50 digits; tolerances 1e-10 m and 1e-10; aim `geo.project_points@1`. Tests AC-1..AC-3. Every case: `citations: none`, `basis: closed_form_math`, `dialect: none`; byte-reproducible per seed; the suite must pass `rdl.check_suite@1`. Tooling layer: link no product crate. No panic or `unwrap`.

## Open questions

- Surfaces are written in the project's neutral NURBS JSON; whether cases should also ship as STEP files for IO round-trip tests is for IO's owners.

## Children

_Leaf: none._
