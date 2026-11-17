---
id: IO.MESH.MBLD
title: Shared mesh builder, import report and precision report
level: 3
parent: IO.MESH
kind: task
status: leaf
phase: P1b
size: S
owners: []
claimed_by:
satisfies: [REQ-IO.MESH-006, REQ-IO.MESH-009]
covers: []
depends_on: [IO.REG.LIMITS, GEO.TYPES]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# IO.MESH.MBLD: Shared mesh builder, import report and precision report

## Purpose

The one builder all four mesh readers feed: it accepts vertices and faces as read, builds a GEO triangle mesh (or point set) without repairing anything, counts degenerate and non-manifold elements for the import report, and computes the rounding error that a single-precision export would introduce.

## Scope

**In:**
- `MeshBuilder`: append vertices (f64 or f32 as declared), faces, optional per-vertex attributes, budgeted through `IO.REG.LIMITS`
- Import-report counts: vertices, faces, degenerate faces (repeated index or zero area), non-manifold edges (shared by more than two faces), unreferenced vertices, out-of-range indices
- `f32_rounding_report(mesh)`: maximum absolute coordinate change when rounding to f32, in metres

**Out:**
- Format parsing (STL, PLY, OBJ, TMF siblings)
- Mesh repair (SCAN)

## Requirements

- **REQ-IO.MESH.MBLD-001** The builder shall keep every vertex and face as read, including degenerate faces and non-manifold edges, and shall not merge, reorder or drop elements.
- **REQ-IO.MESH.MBLD-002** The import report shall count degenerate faces, non-manifold edges and unreferenced vertices exactly.
- **REQ-IO.MESH.MBLD-003** If a face references a vertex index outside the vertex range, then the build shall fail with `malformed` naming the face index.
- **REQ-IO.MESH.MBLD-004** The precision report shall state the maximum absolute coordinate change in metres caused by rounding each coordinate to the nearest f32, computed in f64.

## Acceptance criteria

- **AC-1** Given `fixtures/io/mesh/mbld/tetra_plus_degenerate.json` (a tetrahedron plus one face with a repeated index and one extra face sharing an edge with two others), when built, then counts are 5 faces, 1 degenerate face, 1 non-manifold edge, 0 unreferenced vertices, and the face list equals the input order.
- **AC-2** (error path) Given a face with index 9 on a 4-vertex mesh, then the build fails `malformed{face: 4}`.
- **AC-3** Given a vertex at (5.000000123, 0, 0) m, when the precision report runs, then the reported maximum change equals |f32(5.000000123) − 5.000000123| computed in f64.
- **AC-4** Given a 10^6-face synthetic mesh (seed 7), when built with an element limit of 10^5, then it fails `limit_exceeded{limit: elements}`.

## Design

- **Registry commands:** none (library).
- **Data structures:** `MeshBuilder { positions: Vec<[f64;3]>, source_precision: F32|F64, faces: Vec<[u32;3]>, attrs: AttrTable }`; `MeshImportCounts`; `PrecisionReport { max_abs_m: f64 }`.
- **Algorithm:** non-manifold edge count by sorting undirected edge keys (`(min,max)`) and counting runs > 2 (no HashMap, KNP-style); degenerate if any two indices equal or the f64 cross product is exactly zero.
- **Errors:** `malformed`, `limit_exceeded`.
- **Determinism:** D0.
- **Performance budget:** 10^7 faces counted in < 1.5 s [unverified until measured].
- **Crate:** `fid-io-mesh`.

## Test fixtures

Small JSON meshes under `fixtures/io/mesh/mbld/` and a seeded synthetic grid generator; CC-BY-4.0.

## Build prompt

> In crate `fid-io-mesh`, implement `MeshBuilder` (positions f64 with recorded source precision, triangle faces, per-vertex attribute table), budgeted via `fid-io-reg` `Budget`. Never repair: keep order, degenerate faces and non-manifold edges. Report counts: vertices, faces, degenerate (repeated index or exactly zero cross product), non-manifold edges (sort undirected edge keys, count runs > 2), unreferenced vertices. Out-of-range index fails `malformed{face}`. Add `f32_rounding_report` returning the max absolute coordinate change in metres. No std HashMap, no panics, no unwrap (CONSTITUTION §8). Output is a GEO triangle mesh or point set. Tests AC-1..AC-4.

## Open questions

- None.

## Children

_Leaf: none._
