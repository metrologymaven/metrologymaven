---
id: IO.MESH.OBJ
title: OBJ geometry reader and writer
level: 3
parent: IO.MESH
kind: feature
status: leaf
phase: P1b
size: S
owners: []
claimed_by:
satisfies: [REQ-IO.MESH-004, REQ-IO.MESH-007]
covers: [GOM.IMP.02, GOM.EXP.01, PW.IMP.02, PW.EXP.01]
depends_on: [IO.MESH.MBLD, IO.REG.ENV, IO.REG.WRITE]
side_effect: W2
determinism: D0
cli: [fid import mesh-obj, fid export mesh-obj]
mcp: [import_mesh_obj, export_mesh_obj]
journal: [import.mesh_obj@1, export.mesh_obj@1]
built_in: [fid-io-mesh]
updated: 2026-11-22
---

# IO.MESH.OBJ: OBJ geometry reader and writer

## Purpose

Imports the geometry part of Wavefront OBJ (vertices, normals, polygonal faces) into GEO meshes and exports meshes as OBJ. Materials, textures and free-form geometry are ignored and reported.

## Scope

**In:**
- `v`, `vn`, `f` (forms `i`, `i/t`, `i//n`, `i/t/n`, negative relative indices), `o` and `g` names as untrusted metadata
- Fan triangulation of n-gons, counted in the report
- Ignored statements counted by keyword (`vt`, `mtllib`, `usemtl`, `s`, `l`, `p`, `curv`, `surf`, ...)
- Export with optional normals
- `import.mesh_obj@1` (W2), `export.mesh_obj@1` (W1 write_root)

**Out:**
- Loading `.mtl` files or textures (never opened, so no second file access)
- Units: OBJ declares none, so `unit` is required

## Requirements

- **REQ-IO.MESH.OBJ-001** When an OBJ face has more than three vertices, the reader shall fan-triangulate it and report the number of faces triangulated.
- **REQ-IO.MESH.OBJ-002** The reader shall resolve negative (relative) indices, and if any index resolves outside the vertex list, then the import shall fail with `malformed` naming the line.
- **REQ-IO.MESH.OBJ-003** Statements the reader does not import shall be counted by keyword in the import report, and the reader shall never open a file referenced by `mtllib`.
- **REQ-IO.MESH.OBJ-004** OBJ coordinates shall be parsed as f64 with correct rounding, and the exporter shall write shortest round-trip decimal forms so that export and re-import is bit-identical.

## Acceptance criteria

- **AC-1** Given `fixtures/io/mesh/obj/cube_quads.obj` (8 `v`, 6 quad `f`) and `--unit mm`, then 12 triangles, report `triangulated_faces: 6`.
- **AC-2** Given `fixtures/io/mesh/obj/negative_idx.obj`, then faces equal those of an equivalent absolute-index file.
- **AC-3** Given `fixtures/io/mesh/obj/with_mtl.obj` referencing `missing.mtl` and using `usemtl`, `vt`, then import succeeds, no file-open of `missing.mtl` is recorded by the test's file-system mock, and the report counts `mtllib:1, usemtl:2, vt:24`.
- **AC-4** (error path) Given `fixtures/io/mesh/obj/bad_index.obj` (`f 1 2 99` with 8 vertices), then fails `malformed{line: 10}`.
- **AC-5** Given AC-1's mesh exported and re-imported, then coordinates are bit-identical.

## Design

- **Registry commands:** `import.mesh_obj@1` (W2; `source`, `unit`, `stage?`, `dry_run?`), `export.mesh_obj@1` (W1 write_root; `mesh`, `target`, `normals: bool`, `overwrite?`). Strongest class W2 named in frontmatter.
- **Algorithm:** line tokenizer, 4 KiB line cap, decimal parsing with a correctly rounded parser; export via shortest round-trip formatting.
- **Errors:** `malformed`, `limit_exceeded`, `unit_required`.
- **Determinism:** D0.
- **Performance budget:** 10^6 faces in < 1 s [unverified until measured].
- **Crate:** `fid-io-mesh`.

## Test fixtures

Hand-written OBJ files under `fixtures/io/mesh/obj/` (CC-BY-4.0).

## Build prompt

> In crate `fid-io-mesh`, implement OBJ import/export: `import.mesh_obj@1` (W2; CLI `fid import mesh-obj`; MCP `import_mesh_obj`) and `export.mesh_obj@1` (W1, `fs_effect: write_root`; CLI `fid export mesh-obj`; MCP `export_mesh_obj`) on the `fid-io-reg` skeleton. Read `v`, `vn`, `f` in all index forms including negative indices; fan-triangulate n-gons and count; out-of-range index → `malformed{line}`; count all other statements by keyword; never open `mtllib` files. Parse f64 with correct rounding; export shortest round-trip decimals. Unit is required. Feed `MeshBuilder`. CONSTITUTION §8: bounded lines, no panics, no unwrap. Tests AC-1..AC-5.

## Open questions

- None.

## Children

_Leaf: none._
