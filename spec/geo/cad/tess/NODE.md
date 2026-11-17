---
id: GEO.CAD.TESS
title: Controlled, watertight tessellation of CAD bodies
level: 3
parent: GEO.CAD
kind: module
status: open
phase: P1b
size: L
owners: []
claimed_by:
satisfies: [REQ-GEO.CAD-003, REQ-GEO.CAD-004, REQ-GEO.CAD-005, REQ-GEO.CAD-006]
covers: [GOM.CAD.04]
depends_on: [GEO.CAD.TOPO, GEO.NURBS.EVAL, GEO.TYPES.MESH]
cli: [fid geo tessellate-body, fid geo set-tessellation-tolerance]
mcp: [geo_tessellate_body, geo_set_tessellation_tolerance]
journal: [geo.tessellate_body@1, geo.set_tessellation_tolerance@1]
updated: 2026-11-17
---

# GEO.CAD.TESS: Controlled, watertight tessellation of CAD bodies

## Purpose

Turns CAD bodies into triangle meshes for display, spatial indexing and candidate search, under user-controlled chordal and angular tolerances, sharing vertices along common edges so closed bodies tessellate watertight. Tessellation is never used for values of record (projection uses exact geometry), but its quality and determinism matter for display, picking and reproducible selections.

## Scope

**In:**
- Edge discretisation shared by both adjacent faces, then face meshing in parameter space inside the trimming loops
- Chordal-deviation and angular tolerances; per-body and per-document defaults
- Vertex-on-surface guarantee and per-triangle deviation bound
- Output as a GEO.TYPES `Mesh` with per-triangle face IDs
- `geo.set_tessellation_tolerance@1` (W2) with recompute of dependents through CORE.DEP, and `geo.tessellate_body@1`
- GOM.CAD.04 (control CAD tessellation quality)

**Out** (and which node owns it instead):
- Topology and trimming model (owned by `GEO.CAD.TOPO`)
- GPU display of meshes (owned by `VIZ`)
- Mesh indexes (owned by `GEO.INDEX.BVH`)

## Requirements

- **REQ-GEO.CAD.TESS-001** When a body is tessellated with chordal tolerance t and angular tolerance a, every mesh vertex shall lie on its source surface within 1e-9 m [unverified] and every triangle shall deviate from its surface by at most t.
- **REQ-GEO.CAD.TESS-002** Adjacent faces shall share every vertex along their common edge, so the mesh of a valid closed body reports `closed = true` in GEO.TYPES topology facts.
- **REQ-GEO.CAD.TESS-003** Tessellation shall be deterministic class D1 for a given body, tolerance pair and engine version, independent of thread count.
- **REQ-GEO.CAD.TESS-004** When the tessellation tolerance changes, the system shall journal one W2 `geo.set_tessellation_tolerance@1` entry and recompute dependents through the CORE dependency graph.
- **REQ-GEO.CAD.TESS-005** Each output triangle shall carry the ID of its source face.
- **REQ-GEO.CAD.TESS-006** If a face cannot be tessellated within tolerance (for example a defective trimming loop), then the system shall report the face ID and continue with the other faces rather than fail the whole body.

## Interfaces

- **Offers:** `Tessellator`, `TessTolerance { chordal_m, angular_rad }`, `Tessellation { mesh, face_of_triangle }`; commands `geo.tessellate_body@1` (W2: creates a derived mesh element), `geo.set_tessellation_tolerance@1` (W2).
- **Consumes:** GEO.CAD.TOPO bodies and trimming; GEO.NURBS evaluators; GEO.TYPES `Mesh`; CORE.DEP.

## Decomposition hints

EDGE (shared edge discretisation), FACE (parameter-space meshing inside loops), REFINE (tolerance-driven refinement and checks), CMD (commands). The engine behind FACE and REFINE depends on D-024.

## Open questions

- **Unblocked (2026-11-17):** D-024 (B-rep kernel - own Rust crate or OCCT out of process) is accepted. Apply its Decision section (`decisions/D-024-*.md`) when this node is expanded.
- **Blocked on D-024** (B-rep kernel: own Rust crate or OCCT out of process). Under options A, C and D the tessellator is Rust code in `fid-geo-cad` and can reach D1 or D0; under option B it is the OCCT mesher in a worker process, which changes the determinism class (likely D1 per pinned build or N), rules out a WASM tessellator and moves the leaf's crate boundary. Splitting before the decision would be guessing.
- REQ-GEO.CAD-006 calls the tolerance change W2; whether per-view display tolerances (not stored in the document) also need a command is a VIZ question.

## Children

_None yet: status `decision`._
