---
id: GEO.CAD.TESS.BODY
title: Body tessellation driver and watertight assembly
level: 4
parent: GEO.CAD.TESS
kind: task
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-GEO.CAD.TESS-002, REQ-GEO.CAD.TESS-003, REQ-GEO.CAD.TESS-005, REQ-GEO.CAD.TESS-006]
covers: []
depends_on: [GEO.CAD.TESS.EDGE, GEO.CAD.TESS.FACE, GEO.CAD.TESS.REFINE, GEO.TYPES.MESH]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# GEO.CAD.TESS.BODY: Body tessellation driver and watertight assembly

## Purpose

The `Tessellator` entry point: samples all edges once, meshes and refines faces in parallel, and assembles one GEO.TYPES `Mesh` with shared boundary vertices and a per-triangle face ID table, in a fixed order so the output does not depend on the thread count. Faces that fail are listed and skipped rather than failing the body.

## Scope

**In:**
- `tessellate(body, TessTolerance, threads) -> Tessellation { mesh, face_of_triangle, failed_faces, edge_reports, achieved }`
- Parallel face meshing and refinement with results stored by face index, then assembly in ascending face ID order
- Vertex numbering: shared edge vertices first (GEO.CAD.TESS.EDGE order), then interior vertices face by face
- Watertightness check through GEO.TYPES.MESH topology facts
- Tolerance validation: chordal in [1e-7 m, 1e-1 m], angular in [0.01 rad, 1.0 rad] [unverified ranges]
- Instance bodies: tessellated in the definition frame; the mesh carries the instance's frame reference

**Out** (and which node owns it instead):
- Commands, journaling and dependents (owned by `GEO.CAD.TESS.CMD`)
- GPU display (owned by `VIZ`)
- Mesh BVH (owned by `GEO.INDEX.BVH`)

## Requirements

- **REQ-GEO.CAD.TESS.BODY-001** When a valid closed body (empty GEO.CAD.TOPO.VALID report) is tessellated and no face fails, the output mesh shall report `closed = true` and `oriented = true` in GEO.TYPES.MESH topology facts.
- **REQ-GEO.CAD.TESS.BODY-002** Every output triangle shall have an entry in `face_of_triangle` naming its source face's `TopologyId`.
- **REQ-GEO.CAD.TESS.BODY-003** The output shall be bit-identical for any thread count from 1 to 64 and on every Tier 1 platform (class D1 required by the parent; the design targets D0).
- **REQ-GEO.CAD.TESS.BODY-004** If faces fail (`FaceFailed` or `FaceTolerance`), then the driver shall list them with reasons in `failed_faces` or `achieved`, include all other faces, and still return a mesh.
- **REQ-GEO.CAD.TESS.BODY-005** If the tolerance pair is outside the allowed ranges or the result would exceed 5 × 10⁷ triangles, then the driver shall fail with `BadTolerance` or `TooManyTriangles` before producing output [unverified limit].
- **REQ-GEO.CAD.TESS.BODY-006** The crate shall build and pass its tests for `wasm32-unknown-unknown`, so the web viewer tessellates locally (D-024 option C).

## Acceptance criteria

- **AC-1** Given fixture `fixtures/geo/cad/tess/analytic_set` at t = 1e-5 m, a = 0.2 rad, when tessellated, then each mesh reports `closed = true`, `oriented = true`, `boundary_edges = 0`, and every triangle has a face ID.
- **AC-2** Given the same input with 1, 2, 8 and 64 threads, then mesh digests (GEO.TYPES.DIGEST) are identical, and identical on Linux, Windows, macOS and wasm32 CI.
- **AC-3** Given fixture `fixtures/geo/cad/tess/one_bad_face.json` (a cube with one bow-tie loop), then five faces are meshed, the bad face is in `failed_faces`, and `closed = false`.
- **AC-4** (error path) Given t = 0, then `BadTolerance`.
- **AC-5** Given fixture `fixtures/geo/cad/tess/step_sample` (a real-world body converted from a public STEP sample from IO.CORP [unverified availability]), then tessellation completes and the dense check of GEO.CAD.TESS.REFINE AC-1 passes on non-failed faces.

## Design

- **Registry command(s):** none; library leaf.
- **Data structures:** `TessTolerance { chordal_m, angular_rad }`, `Tessellation` as in Scope, `FaceIssue { face, reason, achieved_chordal_m?, achieved_angular_rad? }`.
- **Algorithm:** EDGE once (sequential, D0); faces as independent jobs in a work queue writing into pre-allocated slots by face index; assemble by ascending face ID; renumber interior vertices by (face order, local index); compute topology facts.
- **Errors:** `BadTolerance`, `TooManyTriangles`, `Cancelled`.
- **Determinism:** D0 by design (D1 required): scheduling affects only which thread fills a slot.
- **Performance budget:** a 10⁴-face body with 10⁶ output triangles under 5 s on 8 threads [unverified].
- **Crate:** `fid-geo-cad` (L0, KNP-1 applies; the cancellation token is a plain atomic flag from the caller).

## Test fixtures

- `analytic_set`, `one_bad_face.json` (hand-written), `step_sample` (converted from an IO.CORP public STEP file under its source licence). CC-BY-4.0 for hand-written fixtures.

## Build prompt

> In crate `fid-geo-cad` (L0, no I/O, KNP-1, wasm32-buildable), add module `tess::body` with `tessellate(&Body, TessTolerance, threads, cancel) -> Result<Tessellation, TessError>`. Validate tolerances (chordal 1e-7..1e-1 m, angular 0.01..1.0 rad). Run `tess::edge` once, then `tess::face` plus `tess::refine` per face in parallel writing into slots by face index; assemble in ascending face ID order (shared edge vertices first, then interior vertices per face) into a `fid-geo-types` `Mesh` with `face_of_triangle`; collect `FaceFailed`/`FaceTolerance` into the report and continue; fail with `TooManyTriangles` above 5 × 10⁷. Acceptance: AC-1 closed and oriented meshes with face IDs; AC-2 identical digests for 1/2/8/64 threads and on Linux, Windows, macOS, wasm32; AC-3 one bad face skipped; AC-4 `BadTolerance`; AC-5 real STEP sample. Constraints: CONSTITUTION §8 (no panic, no `unsafe`), KNP-1, C2 (CPU only). Files: `crates/fid-geo-cad/src/tess/body.rs`, `tests/tess_body.rs`, `fixtures/geo/cad/tess/`.

## Open questions

- The STEP sample in AC-5 depends on IO.CORP publishing a corpus and on IO.CAD.STEP building bodies; until then AC-5 uses a body description committed by hand.

## Children

_Leaf: none._
