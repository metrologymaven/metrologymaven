---
id: IO.MESH
title: Mesh formats: STL, PLY, OBJ, 3MF
level: 2
parent: IO
kind: subsystem
status: open
phase: P1b
size: L
owners: []
claimed_by:
satisfies: [REQ-IO-001, REQ-IO-003, REQ-IO-006]
covers: [GOM.IMP.01, GOM.IMP.02, GOM.EXP.01, PW.IMP.02, PW.EXP.01]
depends_on: [IO.REG, GEO]
cli: [fid import mesh*, fid export mesh*]
mcp: [import_mesh*, export_mesh*]
journal: [import.mesh*, export.mesh*]
updated: 2026-11-16
---

# IO.MESH: Mesh formats

## Purpose

Readers and writers for triangle and polygon mesh files, producing and consuming GEO triangle meshes (and point sets for vertex-only PLY files). Scan-inspection users bring most of their data as meshes, so this is the first scan-data path.

## Scope

**In:**
- STL, ASCII and binary
- PLY, ASCII and binary (both byte orders), including vertex-only PLY files read as point sets, with standard and custom vertex properties
- OBJ geometry (vertices, normals, faces); materials and textures ignored and reported
- 3MF core mesh (with its declared unit)
- Export of GEO meshes (and point sets to PLY) in all four formats
- Import reports: counts of vertices, faces, degenerate and non-manifold elements, dropped items

**Out** (and which node owns it instead):
- Mesh repair, hole filling, smoothing and other processing (SCAN)
- Mesh types, units and frames themselves (GEO)
- Point-cloud formats other than PLY (`IO.CLOUD`)
- CAD tessellation (`IO.CAD` and GEO)
- Limits, roots, provenance and unit-required rule (`IO.REG`)

## Requirements

- **REQ-IO.MESH-001** When an STL file is imported, the system shall detect ASCII or binary encoding from file size consistency with the declared triangle count, not from the leading `solid` keyword.
- **REQ-IO.MESH-002** If a binary STL's declared triangle count does not match its byte length, then the import shall fail with error `truncated_file`.
- **REQ-IO.MESH-003** When a PLY file is imported, the system shall keep every vertex property (coordinates, normals, colour and custom scalars) with its declared type, and double-precision coordinates shall be kept without loss.
- **REQ-IO.MESH-004** When an OBJ file is imported, faces with more than three vertices shall be triangulated and the number of triangulated faces shall appear in the import report.
- **REQ-IO.MESH-005** When a 3MF file is imported, the system shall use its declared unit and shall not require a `unit` argument.
- **REQ-IO.MESH-006** The mesh readers shall import degenerate and non-manifold geometry unchanged and report its counts, leaving repair to SCAN.
- **REQ-IO.MESH-007** The system shall export a GEO mesh to STL (ASCII and binary), PLY (ASCII and binary), OBJ and 3MF.
- **REQ-IO.MESH-008** When a mesh is exported to PLY with double-precision properties and re-imported, the vertex coordinates shall be bit-identical.
- **REQ-IO.MESH-009** When a mesh is exported to a single-precision format (STL, or PLY with float properties), the export report shall state the maximum coordinate rounding error introduced.
- **REQ-IO.MESH-010** The system shall import a 10-million-triangle binary STL in under 5 s on the reference hardware [unverified until measured].

## Interfaces

- **Offers:** `import.mesh_stl@1`, `import.mesh_ply@1`, `import.mesh_obj@1`, `import.mesh_3mf@1` and the matching `export.mesh_*@1` commands, all registered with `IO.REG`.
- **Consumes:** `IO.REG` harness and provenance; GEO triangle mesh and point-set types.

## Decomposition hints

STL, PLY, OBJ, TMF (3MF), one module each, sharing a GEO mesh builder; a small EXPORT module for precision reporting.

## Open questions

- 3MF is a ZIP container with XML parts; the ZIP reader needs its own bomb limits (compression ratio, entry count) inside the `IO.REG` limits.

## Children

_None yet: status `open`._
