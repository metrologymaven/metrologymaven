---
id: IO.CAD.EXPORT
title: STEP and IGES export of geometric elements
level: 3
parent: IO.CAD
kind: module
status: split
phase: P1b
size: L
owners: []
claimed_by:
satisfies: [REQ-IO.CAD-009]
covers: [GOM.EXP.07, PW.EXP.03]
depends_on: [IO.CAD.STEP.P21, IO.CAD.IGES.SECT, IO.REG.ENV, IO.REG.WRITE, GEO.TYPES]
cli: [fid export cad-step, fid export cad-iges]
mcp: [export_cad_step, export_cad_iges]
journal: [export.cad_step@1, export.cad_iges@1]
updated: 2026-11-17
---

# IO.CAD.EXPORT: STEP and IGES export of geometric elements

## Purpose

Writes selected geometric elements (points, lines, planes, circles, cylinders, cones, spheres, curves, surfaces), nominal or measured, to STEP and IGES so they can go back into a CAD system, with their unit and frame stated.

## Scope

**In:**
- Element-to-entity mapping for STEP (geometry-only AP242 or AP214) and IGES
- Bounded elements: infinite primitives (planes, lines, cylinders) written with a caller-given or fitted extent
- Frame choice (world or a named frame) and unit choice
- `export.cad_step@1` and `export.cad_iges@1` (W1, `fs_effect: write_root`)

**Out:**
- PMI export (not required by IO.CAD)
- Mesh export (`IO.MESH`)

## Requirements

- **REQ-IO.CAD.EXPORT-001** The system shall export selected geometric elements to STEP and to IGES in a caller-chosen frame and length unit, both recorded in the file and in the export report.
- **REQ-IO.CAD.EXPORT-002** When an element is unbounded, the exporter shall bound it with the caller's extent or, if none is given, with the extent of the element's fitted points, and shall report the extent used.
- **REQ-IO.CAD.EXPORT-003** If a selected element type has no mapping for the target format, then the export shall fail with `element_unsupported` naming it, unless `skip_unsupported` is set, in which case it shall be listed in the report.
- **REQ-IO.CAD.EXPORT-004** When an exported file is re-imported, each element's defining parameters shall match the originals within 1e-9 m and 1e-9 rad [unverified: tolerance].
- **REQ-IO.CAD.EXPORT-005** Element names shall be written as entity names or labels so they survive into the CAD system.

## Interfaces

- **Offers:** `export.cad_step@1`, `export.cad_iges@1` (W1 write_root).
- **Consumes:** GEO element types, `IO.REG` writer, `IO.CAD.WORKER` if the kernel writes the files.

## Decomposition hints

MAP (element mapping), BOUND (extents), STEPW, IGESW.

## Open questions

- Resolved at this split: the writers are first-party (no CAD kernel, no worker; writing trusted in-memory elements needs no isolation), built on the shared Part 21 printer (`IO.CAD.STEP.P21`) and IGES section writer (`IO.CAD.IGES.SECT`). `depends_on` changed accordingly (IO.CAD.WORKER dropped).

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| IO.CAD.EXPORT.BOUND | Export plan and extents for unbounded elements (leaf) | 002 | none |
| IO.CAD.EXPORT.STEPW | STEP export of geometric elements (leaf) | 001 003 004 005 | GOM.EXP.07 |
| IO.CAD.EXPORT.IGESW | IGES export of geometric elements (leaf) | 001 003 004 005 | GOM.EXP.07, PW.EXP.03 |
