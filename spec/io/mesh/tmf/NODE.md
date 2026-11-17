---
id: IO.MESH.TMF
title: 3MF core mesh reader and writer
level: 3
parent: IO.MESH
kind: feature
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-IO.MESH-005, REQ-IO.MESH-007]
covers: [GOM.IMP.02, GOM.EXP.01, PW.IMP.02, PW.EXP.01]
depends_on: [IO.MESH.MBLD, IO.REG.ENV, IO.REG.WRITE]
side_effect: W2
determinism: D0
cli: [fid import mesh-3mf, fid export mesh-3mf]
mcp: [import_mesh_3mf, export_mesh_3mf]
journal: [import.mesh_3mf@1, export.mesh_3mf@1]
updated: 2026-11-16
---

# IO.MESH.TMF: 3MF core mesh reader and writer

## Purpose

Imports the mesh objects of a 3MF package (ZIP with OPC parts and a 3D model XML part) using the package's declared unit, and exports GEO meshes as minimal 3MF core packages. The ZIP container needs its own bomb defences inside the `IO.REG` limits.

## Scope

**In:**
- ZIP reading with limits on entry count, total uncompressed size and compression ratio
- OPC relationship lookup of the 3D model part; model XML with DTDs and external entities rejected
- `model@unit` (micron, millimeter, centimeter, inch, foot, meter), `object` meshes, `build` items with transforms recorded as frames (not baked)
- Export of one or more meshes as a 3MF core package
- `import.mesh_3mf@1` (W2), `export.mesh_3mf@1` (W1 write_root)

**Out:**
- 3MF extensions (materials, beam lattice, slices, production) other than counting them in the report
- Components assembly flattening beyond recording component transforms as frames

## Requirements

- **REQ-IO.MESH.TMF-001** When a 3MF file is imported, the reader shall use the model's declared unit (default millimetre when absent, per the 3MF core specification [unverified]) and shall not require a `unit` argument.
- **REQ-IO.MESH.TMF-002** If the ZIP has more than 10,000 entries, an uncompressed total above the byte limit, or any entry with compression ratio above 200:1, then the import shall fail with `limit_exceeded` naming the limit [unverified: thresholds to tune].
- **REQ-IO.MESH.TMF-003** If the model XML contains a DOCTYPE or external entity, then the import shall fail with `xml_forbidden_construct`.
- **REQ-IO.MESH.TMF-004** Build-item and component transforms shall be recorded as frames on the imported meshes and shall not be baked into coordinates.
- **REQ-IO.MESH.TMF-005** When a mesh is exported, the writer shall declare the unit `millimeter` unless the caller names another, and re-import shall reproduce coordinates within one ulp of the declared-unit decimal values written.

## Acceptance criteria

- **AC-1** Given `fixtures/io/mesh/tmf/cube_mm.3mf` (unit millimeter, 8 vertices, 12 triangles), when imported with no `--unit`, then 12 triangles, provenance unit `mm`.
- **AC-2** Given `fixtures/io/mesh/tmf/cube_inch.3mf`, then provenance unit `in` and a vertex at 1 inch becomes 0.0254 m.
- **AC-3** (error path) Given `fixtures/io/mesh/tmf/bomb.3mf` (one entry with ratio 1000:1), then fails `limit_exceeded{limit: compression_ratio}` after reading no more than 1 MiB of decompressed output.
- **AC-4** (error path) Given `fixtures/io/mesh/tmf/doctype.3mf`, then fails `xml_forbidden_construct`.
- **AC-5** Given `fixtures/io/mesh/tmf/two_items_transformed.3mf`, then two meshes are imported, each with a frame equal to its build transform, and raw vertex coordinates equal the file's.
- **AC-6** Given AC-1's mesh exported and re-imported, then coordinates match within one ulp.

## Design

- **Registry commands:** `import.mesh_3mf@1` (W2; `source`, `unit?` with `override_unit`, `stage?`, `dry_run?`), `export.mesh_3mf@1` (W1 write_root; `meshes[]`, `target`, `unit?`, `overwrite?`). Strongest class W2 named in frontmatter.
- **Algorithm:** ZIP central directory read with entry-count cap; streaming inflate with ratio check per 64 KiB; XML via the hardened parser configuration shared with `IO.QIF.XML` (no DTD, no entities).
- **Errors:** `limit_exceeded`, `xml_forbidden_construct`, `malformed`, `unit_conflict`.
- **Determinism:** D0 (writer uses fixed ZIP timestamps and entry order).
- **Performance budget:** 10^6 triangles in < 3 s [unverified until measured].
- **Crate:** `fid-io-mesh`. ZIP and XML crates must be on the D-001 dependency allowlist.

## Test fixtures

Script-generated 3MF packages under `fixtures/io/mesh/tmf/` (CC-BY-4.0); 3MF Consortium sample files via the `IO.CORP` manifest [unverified: licence].

## Build prompt

> In crate `fid-io-mesh`, implement 3MF core import/export: `import.mesh_3mf@1` (W2; CLI `fid import mesh-3mf`; MCP `import_mesh_3mf`) and `export.mesh_3mf@1` (W1, `fs_effect: write_root`; CLI `fid export mesh-3mf`; MCP `export_mesh_3mf`) on the `fid-io-reg` skeleton. ZIP limits: ≤ 10,000 entries, total uncompressed ≤ byte limit, per-entry ratio ≤ 200:1 checked while streaming (`limit_exceeded`). Locate the model part via OPC relationships; parse XML with DTD and entities rejected (`xml_forbidden_construct`). Use `model@unit` (no unit argument needed; caller unit conflicts → `unit_conflict` unless `override_unit`). Record build-item and component transforms as frames, never bake them. Export writes a minimal core package with fixed timestamps and entry order. Feed `MeshBuilder`. CONSTITUTION §8: no panics, no unwrap. Tests AC-1..AC-6.

## Open questions

- None.

## Children

_Leaf: none._
