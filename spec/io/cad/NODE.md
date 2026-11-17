---
id: IO.CAD
title: CAD exchange: STEP AP242 with semantic PMI, IGES, native translators
level: 2
parent: IO
kind: subsystem
status: open
phase: P1b
size: XXL
owners: []
claimed_by:
satisfies: [REQ-IO-001, REQ-IO-004]
covers: [GOM.IMP.05, GOM.IMP.06, GOM.IMP.07, GOM.EXP.07, PCD.CAD.01, PW.IMP.03, PW.IMP.04, PW.EXP.03, SA.CAD.01]
depends_on: [IO.REG, IO.QIF, GEO]
cli: [fid import cad*, fid export cad*]
mcp: [import_cad*, export_cad*]
journal: [import.cad*, export.cad*]
updated: 2026-11-16
---

# IO.CAD: CAD exchange

## Purpose

Brings nominal geometry and product manufacturing information into the platform: STEP AP242 (ISO 10303-242) B-rep geometry and semantic PMI mapped into GEO CAD bodies and the QIF MBD model, IGES geometry, export of geometric elements back to STEP and IGES, and an optional plugin path for native CAD formats.

## Scope

**In:**
- STEP AP242 B-rep geometry import (also AP203/AP214 geometry files [unverified: exact schema list to fix at leaf level])
- AP242 semantic PMI: characteristics, datums, datum reference frames, tolerances and modifiers, mapped into QIF MBD
- Classification of semantic versus graphic-only PMI in the import report
- Keeping the original STEP file as the geometry source of truth, by digest
- IGES geometry import (surfaces, trimmed surfaces, curves, points)
- Export of geometric elements (points, lines, planes, circles, cylinders, curves, surfaces) to STEP and IGES
- The plugin contract for native CAD translators (CATIA, NX, Creo, SolidWorks, Parasolid, JT)

**Out** (and which node owns it instead):
- B-rep and NURBS types, tessellation and point-to-CAD distance (GEO)
- Turning imported PMI into checks and evaluating them (GDT)
- Generating CMM features from CAD and PMI (CMM)
- The QIF model and its XML/JSON forms (`IO.QIF`)
- Shipping licensed translators (third-party plugins; see open questions)

## Requirements

- **REQ-IO.CAD-001** When a STEP AP242 file is imported, the system shall create GEO CAD bodies with faces, edges and their names and IDs as stored in the file.
- **REQ-IO.CAD-002** When a STEP file is imported, the system shall store the original file bytes as a content-addressed blob and link every created body to that blob's digest.
- **REQ-IO.CAD-003** When a STEP AP242 file carries semantic PMI, the system shall map each characteristic, datum feature, datum reference frame and tolerance into QIF MBD entities linked to the CAD faces they reference.
- **REQ-IO.CAD-004** When a STEP file carries graphic-only PMI, the system shall list each item in the import report as `graphic_only` and shall not create characteristics from it.
- **REQ-IO.CAD-005** If a semantic PMI item uses a construct the mapper does not support, then the system shall list it in the import report as `unsupported` with its STEP entity number and shall not drop it silently.
- **REQ-IO.CAD-006** Where the import approximates an exact surface, the system shall record the maximum approximation error per face.
- **REQ-IO.CAD-007** The PMI mapper shall reproduce the expected PMI content for each NIST MBE PMI test model in the CAD corpus [unverified: availability and licence of those models to confirm in `IO.CORP`].
- **REQ-IO.CAD-008** When an IGES file is imported, the system shall create GEO curves, surfaces, trimmed surfaces and points, and report unsupported entity types by count.
- **REQ-IO.CAD-009** The system shall export selected geometric elements to STEP and IGES with their units and frame.
- **REQ-IO.CAD-010** The CAD readers shall run isolated from the document process so that a reader crash or hang fails the import with error `reader_crashed` or `limit_exceeded` and leaves the document unchanged.
- **REQ-IO.CAD-011** The system shall read native CAD formats only through translator plugins registered with `IO.REG`, and no native-format reader shall ship in the core.

## Interfaces

- **Offers:** `import.cad_step@1`, `import.cad_iges@1`, `export.cad_step@1`, `export.cad_iges@1`, and the translator-plugin contract (a translator turns a native file into STEP AP242 or directly into GEO and QIF entities).
- **Consumes:** `IO.REG` harness, limits and plugin interface; `IO.QIF` MBD types; GEO CAD bodies.

## Decomposition hints

STEP (geometry), PMI (semantic PMI to QIF MBD, report classification), IGES, EXPORT (STEP and IGES writers for elements), XLATE (native translator plugin contract). Expect STEP and PMI to split again.

## Open questions

- Native CAD (GOM.IMP.06): plugin-only, or out of scope? Needs licensed translators. REQ-IO.CAD-011 assumes plugin-only; the expander of this node should open a decision record to confirm it, as the IO parent noted.
- REQ-IO.CAD-010 implies a worker-process boundary for the CAD kernel; the concept docs name OCCT (LGPL) in a worker process, which depends on D-001.
- Which STEP schemas besides AP242 must the geometry reader accept? [unverified]

## Children

_None yet: status `open`._
