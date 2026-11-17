---
id: UNC.ORS.PROJ.EDGE
title: Face assignment near seams, edges and trim boundaries
level: 4
parent: UNC.ORS.PROJ
kind: task
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-UNC.ORS.PROJ-002, REQ-UNC.ORS.PROJ-003]
covers: []
depends_on: [UNC.ORS.TRACE, UNC.RDL.GEN]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# UNC.ORS.PROJ.EDGE: Face assignment near seams, edges and trim boundaries

## Purpose

Generates projection cases on face sets (bodies) where the nearest point is on a seam, a shared edge, a vertex or a trim boundary, or where two faces are equidistant. Expected face assignment follows geometry when one face is nearer by a margin, and the project's documented tie rule (lowest face ID, `GEO.DIST.FACE`) when faces are equidistant by construction.

## Scope

**In:** generator `ors_proj::edge`; boxes, filleted boxes, cylinders with seams, trimmed planar faces with holes; points whose nearest face wins by a margin of at least 1 µm, points exactly equidistant by symmetry, points projecting onto edges and vertices; expected face ID, foot point, signed distance and `on_boundary` flag.
**Out:** interior points on single faces (owned by `UNC.ORS.PROJ.ANALYTIC`, `UNC.ORS.PROJ.NURBS`).

## Requirements

- **REQ-UNC.ORS.PROJ.EDGE-001** The generator shall emit at least 20 margin cases, 10 exact-tie cases, 10 edge cases and 5 vertex cases, each stating the expected face ID, foot point and signed distance.
- **REQ-UNC.ORS.PROJ.EDGE-002** Exact-tie cases shall expect the lowest face ID and be tagged `tie_rule` so that a change of the GEO tie rule shows up as a suite version change, not a silent failure.
- **REQ-UNC.ORS.PROJ.EDGE-003** Every case shall state tolerances of 1e-12 m on distance and foot point.

## Acceptance criteria

- **AC-1** Given seed `0x50524A33`, then two runs give identical files and the suite check reports zero findings.
- **AC-2** Given every margin case, the 50-digit reference confirms the nearest face wins by ≥ 1 µm.
- **AC-3** Given the suite, it contains the counts in REQ-001 and every tie case carries `tie_rule`.

## Design

- **Registry command(s):** none; tooling generator (`ors-gen`), aim `geo.project_points@1`.
- **Algorithm:** construct bodies from analytic faces with known IDs; choose foot points on edges, vertices and trims; offset along bisector or face normals; compute nearest face at 50 digits; for ties, construct mirror-symmetric points.
- **Errors:** generator failures abort the build; no partial suite is written.
- **Determinism:** D0.
- **Performance budget:** generation under 30 s.
- **Crate:** `fid-ors` (Tooling; links no product crate).

## Test fixtures

`fixtures/ors/proj/`: the generated cases (project-authored, CC-BY-4.0); no standards text and no third-party CAD files.

## Build prompt

> In tooling crate `fid-ors`, add generator `ors_proj::edge` (`ors-gen proj-edge`), seeded: bodies (box, filleted box, seamed cylinder, trimmed plane with hole) with known face IDs; ≥ 20 margin cases (nearest face wins by ≥ 1 µm, verified at 50 digits), ≥ 10 exact ties by symmetry (expect lowest face ID, tag `tie_rule`), ≥ 10 edge and ≥ 5 vertex cases with `on_boundary`; tolerances 1e-12 m; aim `geo.project_points@1`. Tests AC-1..AC-3. Every case: `citations: none`, `basis: closed_form_math`, `dialect: none`; byte-reproducible per seed; the suite must pass `rdl.check_suite@1`. Tooling layer: link no product crate. No panic or `unwrap`.

## Open questions

- If `GEO.DIST.FACE` changes its tie rule, the `tie_rule` cases change expected values and need a new suite version (REQ-UNC.ORS-007).

## Children

_Leaf: none._
