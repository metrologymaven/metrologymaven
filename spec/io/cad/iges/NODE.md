---
id: IO.CAD.IGES
title: IGES geometry import
level: 3
parent: IO.CAD
kind: module
status: open
phase: P1b
size: L
owners: []
claimed_by:
satisfies: [REQ-IO.CAD-008]
covers: [GOM.IMP.05, PCD.CAD.01, PW.IMP.03, SA.CAD.01]
depends_on: [IO.CAD.WORKER, IO.REG.ENV, GEO.NURBS]
cli: [fid import cad-iges]
mcp: [import_cad_iges]
journal: [import.cad_iges@1]
updated: 2026-11-16
---

# IO.CAD.IGES: IGES geometry import

## Purpose

Imports IGES 5.3 [unverified: edition] geometry, mainly surfaces, trimmed surfaces, curves and points, into GEO elements, and reports every entity type it does not import by count. IGES is still common for legacy nominal data and for feature hand-offs.

## Scope

**In:**
- Fixed-column IGES sections (Start, Global, Directory Entry, Parameter Data, Terminate) parsed with bounds
- Entity types: points (116), lines (110), circular arcs (100), conics (104), rational B-spline curves (126) and surfaces (128), trimmed surfaces (144), bounded surfaces (143), composite curves (102), transformation matrices (124) [unverified: entity list]
- Units from the Global section
- Import report counts per unsupported entity type
- `import.cad_iges@1` (W2)

**Out:**
- IGES B-rep solids (186) beyond counting [unverified]; STEP is the solid path (`IO.CAD.STEP`)
- IGES export (`IO.CAD.EXPORT`)

## Requirements

- **REQ-IO.CAD.IGES-001** When an IGES file is imported, the system shall create GEO points, curves, surfaces and trimmed surfaces for every supported entity, keeping its directory-entry sequence number and label.
- **REQ-IO.CAD.IGES-002** Unsupported entity types shall be listed in the import report with a count per type number.
- **REQ-IO.CAD.IGES-003** The importer shall use the unit declared in the Global section and record it in the provenance.
- **REQ-IO.CAD.IGES-004** Transformation-matrix entities shall be recorded as frames and not baked into coordinates.
- **REQ-IO.CAD.IGES-005** If a directory entry points outside the Parameter Data section, then the import shall fail with `malformed` naming the sequence number.

## Interfaces

- **Offers:** `import.cad_iges@1` (W2).
- **Consumes:** `IO.CAD.WORKER` (if the kernel parses IGES) or a first-party bounded parser; GEO.NURBS.

## Decomposition hints

SECT (section parser), ENT (entity mapping), XFORM (transforms), CMD. A pure-Rust bounded section parser could avoid the worker; decide at the next split.

## Open questions

- Worker-hosted kernel versus first-party pure-Rust IGES parser.

## Children

_None yet: status `open`._
