---
id: IO.CLOUD.E57
title: E57 reader and writer with multi-scan poses
level: 3
parent: IO.CLOUD
kind: module
status: open
phase: P1b
size: L
owners: []
claimed_by:
satisfies: [REQ-IO.CLOUD-003, REQ-IO.CLOUD-004, REQ-IO.CLOUD-008, REQ-IO.CLOUD-010]
covers: [PW.IMP.01, PW.EXP.01]
depends_on: [IO.CLOUD.STREAM, IO.REG.ENV, IO.REG.WRITE, GEO.FRAME]
cli: [fid import cloud-e57, fid export cloud-e57]
mcp: [import_cloud_e57, export_cloud_e57]
journal: [import.cloud_e57@1, export.cloud_e57@1]
updated: 2026-11-16
---

# IO.CLOUD.E57: E57 reader and writer

## Purpose

Reads and writes ASTM E2807 E57 files: multiple 3D scans with poses, Cartesian or spherical coordinates, intensity and colour, and the XML section that describes them. E57 is the evidence-grade exchange form for point data because it can hold f64 coordinates exactly.

## Scope

**In:**
- E57 physical layer (pages with CRC), XML section (hardened), binary sections and compressed vectors
- `data3D` scans with pose (rotation quaternion and translation) as frames; Cartesian and spherical coordinates; intensity, colour, row/column indices, invalid-state flags
- `images2D` counted and reported, not imported
- Export of point sets with selected attributes as one scan per point set, f64 Cartesian
- `import.cloud_e57@1` (W2), `export.cloud_e57@1` (W1 write_root)

**Out:**
- Streaming plumbing (`IO.CLOUD.STREAM`)
- Image import (later; not in IO scope)

## Requirements

- **REQ-IO.CLOUD.E57-001** When an E57 file with several scans is imported, the system shall create one point set per scan, each in its own frame holding that scan's pose.
- **REQ-IO.CLOUD.E57-002** Double-precision Cartesian coordinates shall be imported without loss, and scaled-integer or float coordinates shall record their source precision.
- **REQ-IO.CLOUD.E57-003** Spherical coordinates shall be converted to Cartesian through the `fid-num` facade, and the conversion shall be recorded in the provenance.
- **REQ-IO.CLOUD.E57-004** If a page CRC fails, then the import shall fail with `checksum_mismatch` naming the page offset.
- **REQ-IO.CLOUD.E57-005** When a point set with f64 coordinates is exported to E57 and re-imported, the coordinates shall be bit-identical.
- **REQ-IO.CLOUD.E57-006** Points flagged invalid in the file shall be skipped and counted in the import report.

## Interfaces

- **Offers:** `import.cloud_e57@1` (W2), `export.cloud_e57@1` (W1, `fs_effect: write_root`).
- **Consumes:** `IO.CLOUD.STREAM`, `IO.REG` envelope and writer, GEO.FRAME.

## Decomposition hints

PHYS (paged physical layer and CRC), XML (section parser), CVEC (compressed-vector codec), READ, WRITE.

## Open questions

- Library choice: the concept docs name libE57Format (BSL-1.0, C++) [unverified]; a C++ dependency implies FFI and a worker process (CONSTITUTION §8 forbids `unsafe` without a named exception). A pure-Rust reader may be preferable; needs the D-001 dependency allowlist (ECO.RULES).
- Will verifiers require E57 f64 as evidence-grade point data (carried from IO.CLOUD)?

## Children

_None yet: status `open`._
