---
id: IO.MESH.PLY
title: PLY reader and writer (ASCII and binary, meshes and point sets)
level: 3
parent: IO.MESH
kind: feature
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-IO.MESH-003, REQ-IO.MESH-007, REQ-IO.MESH-008]
covers: [GOM.IMP.02, GOM.EXP.01, PW.IMP.02, PW.EXP.01]
depends_on: [IO.MESH.MBLD, IO.REG.ENV, IO.REG.WRITE]
side_effect: W2
determinism: D0
cli: [fid import mesh-ply, fid export mesh-ply]
mcp: [import_mesh_ply, export_mesh_ply]
journal: [import.mesh_ply@1, export.mesh_ply@1]
built_in: [fid-io-mesh]
updated: 2026-11-22
---

# IO.MESH.PLY: PLY reader and writer

## Purpose

Imports PLY files in all three encodings (ASCII, binary little-endian, binary big-endian) into GEO meshes, or into point sets when the file has no faces, keeping every vertex property with its declared type. Exports meshes and point sets to PLY, with double precision preserved bit for bit.

## Scope

**In:**
- Header parsing: `format`, `element`, `property` (scalar and `list`), `comment`, `obj_info`
- All PLY scalar types (`char`…`double` and their `int8`…`float64` aliases)
- Vertex properties: x, y, z, normals, colour, custom scalars; face lists triangulated by fan when n > 3 (counted in the report)
- Vertex-only files as point sets
- Export of meshes and point sets, ASCII or binary little-endian, with chosen property set and precision
- `import.mesh_ply@1` (W2), `export.mesh_ply@1` (W1 write_root)

**Out:**
- PLY elements other than `vertex` and `face` (reported as dropped with counts)
- Units: PLY declares none, so `unit` is required

## Requirements

- **REQ-IO.MESH.PLY-001** The reader shall accept ASCII, binary little-endian and binary big-endian PLY and shall keep every vertex property with its declared type.
- **REQ-IO.MESH.PLY-002** Double-precision coordinates shall be imported without loss, and float coordinates shall be widened exactly with source precision `f32` recorded.
- **REQ-IO.MESH.PLY-003** When a PLY file has no `face` element or zero faces, the reader shall create a point set instead of a mesh.
- **REQ-IO.MESH.PLY-004** If the header declares more elements than the body holds, then the import shall fail with `truncated_file`.
- **REQ-IO.MESH.PLY-005** When a mesh or point set with f64 coordinates is exported to binary PLY with `precision=double` and re-imported, the coordinates shall be bit-identical.
- **REQ-IO.MESH.PLY-006** Elements other than `vertex` and `face` shall be skipped and listed in the import report with their counts.

## Acceptance criteria

- **AC-1** Given `fixtures/io/mesh/ply/cube_ascii.ply`, `cube_bin_le.ply`, `cube_bin_be.ply` (same cube, 8 vertices, 6 quads, float), when each is imported with `--unit mm`, then each yields 8 vertices, 12 triangles, report `triangulated_faces: 6`, identical coordinates.
- **AC-2** Given `fixtures/io/mesh/ply/points_double_custom.ply` (1,000 vertices, double xyz, `float quality`, `uchar red/green/blue`, no faces), then a point set is created with attributes `quality` (f32) and colour (u8×3), and coordinates equal the file's doubles bit for bit.
- **AC-3** Given AC-2's point set exported with `precision=double` and re-imported, then coordinates and attributes are bit-identical.
- **AC-4** (error path) Given `fixtures/io/mesh/ply/truncated.ply` (header says 8 vertices, body has 5), then the import fails `truncated_file`.
- **AC-5** Given `fixtures/io/mesh/ply/with_edges.ply` (extra `edge` element), then import succeeds and the report lists `dropped: edge × 12`.
- **AC-6** (error path) Given a header with `element vertex 99999999999`, then the import fails `limit_exceeded` before allocating.
- **AC-7** (replay) Given the journal of AC-2, when replayed exactly, then `IDENTICAL`.

## Design

- **Registry commands:** `import.mesh_ply@1` (W2; `source`, `unit`, `stage?`, `dry_run?`), `export.mesh_ply@1` (W1 write_root; `entity` ref, `target`, `encoding: ascii|binary_le`, `precision: float|double`, `properties[]`, `overwrite?`). Strongest class W2 named in frontmatter.
- **Data structures:** `PlyHeader { format, elements: Vec<ElementDecl> }`.
- **Algorithm:** header bounded to 64 KiB and 256 properties; element counts charged to the budget before allocation; fan triangulation `(0,i,i+1)`.
- **Errors:** `truncated_file`, `malformed`, `limit_exceeded`, `unit_required`.
- **Determinism:** D0.
- **Performance budget:** 10^7 binary vertices in < 2 s [unverified until measured].
- **Crate:** `fid-io-mesh`.

## Test fixtures

Script-generated PLY files under `fixtures/io/mesh/ply/` (CC-BY-4.0); public samples (e.g. Stanford scan repository [unverified: licence]) via the `IO.CORP` manifest.

## Build prompt

> In crate `fid-io-mesh`, implement PLY import/export: `import.mesh_ply@1` (W2; CLI `fid import mesh-ply`; MCP `import_mesh_ply`) and `export.mesh_ply@1` (W1, `fs_effect: write_root`; CLI `fid export mesh-ply`; MCP `export_mesh_ply`) on the `fid-io-reg` skeleton. Read ASCII, binary LE and BE; all scalar types and aliases; keep every vertex property with its type; doubles lossless, floats widened exactly with source precision recorded; no faces → point set; fan-triangulate polygons and count them; skip other elements and report counts; header element counts are charged to the budget before allocation (`limit_exceeded`); short body → `truncated_file`. Export ASCII or binary LE with `precision` float or double; double round-trip is bit-identical. Feed `MeshBuilder`; unit is required. CONSTITUTION §8: no panics, no unwrap. Tests AC-1..AC-7.

## Open questions

- None.

## Children

_Leaf: none._
