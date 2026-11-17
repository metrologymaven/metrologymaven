---
id: IO.CLOUD
title: Point-cloud formats: ASCII/CSV, E57, LAS/LAZ, COPC, PTX
level: 2
parent: IO
kind: subsystem
status: split
phase: P1b
size: XL
owners: []
claimed_by:
satisfies: [REQ-IO-001, REQ-IO-003, REQ-IO-006]
covers: [GOM.IMP.04, GOM.IMP.08, GOM.EXP.02, PW.IMP.01, PW.EXP.01, PCD.EXP.02]
depends_on: [IO.REG, GEO]
cli: [fid import cloud*, fid export cloud*]
mcp: [import_cloud*, export_cloud*]
journal: [import.cloud*, export.cloud*]
updated: 2026-11-16
---

# IO.CLOUD: Point-cloud formats

## Purpose

Readers and writers for point data, from small tables of nominal points to scanner clouds of 10^8+ points. It streams into GEO's out-of-core cloud, keeps precision honest (scaled-integer formats record their quantisation) and keeps per-scan frames.

## Scope

**In:**
- ASCII/CSV/XYZ point tables with an explicit column mapping and a role (nominal or actual points)
- E57 (ASTM E2807), including multiple scans with poses, and intensity and colour attributes
- LAS 1.4 and LAZ, and COPC 1.0
- PTX, including per-scan header transforms
- Streaming import into GEO's chunked clouds
- Export of point coordinates and per-point attributes to ASCII/CSV/XYZ, E57, LAS/LAZ and COPC

**Out** (and which node owns it instead):
- PLY point files (`IO.MESH`)
- Characteristic values and result tables (`IO.RES`)
- Out-of-core storage, chunking and LOD (GEO)
- Cloud processing such as filtering and registration (SCAN)
- Live scanner streams (HAL)
- Limits, roots, provenance and unit-required rule (`IO.REG`)

## Requirements

- **REQ-IO.CLOUD-001** When an ASCII or CSV point file is imported, the system shall require a column mapping (given or detected from a header row and confirmed in the import report) and a role of `nominal` or `actual`.
- **REQ-IO.CLOUD-002** If a row in an ASCII or CSV point file cannot be parsed, then the import shall fail with error `malformed` naming the line number, unless the caller sets `skip_bad_rows`, in which case each skipped line shall be listed in the import report.
- **REQ-IO.CLOUD-003** When an E57 file with several scans is imported, the system shall create one point set per scan with that scan's pose recorded as a frame transform.
- **REQ-IO.CLOUD-004** E57 double-precision coordinates shall be imported without loss.
- **REQ-IO.CLOUD-005** When a LAS, LAZ or COPC file is imported, the system shall record each axis's scale and offset and attach the quantisation uncertainty q/sqrt(12) per axis as point-set metadata for UNC.
- **REQ-IO.CLOUD-006** When a PTX file is imported, the system shall record each scan's header transform as a frame and shall not silently bake it into coordinates.
- **REQ-IO.CLOUD-007** The cloud readers shall stream into GEO chunked clouds so that peak resident memory for importing a 10^8-point file stays under 4 GiB [unverified until measured].
- **REQ-IO.CLOUD-008** The system shall export point sets with selected per-point attributes to ASCII/CSV/XYZ, E57, LAS/LAZ and COPC.
- **REQ-IO.CLOUD-009** When the system writes LAS, LAZ or COPC, it shall use a scale of at most 1e-7 m per axis, and if a coordinate is not representable at that scale, then the export shall fail with error `out_of_range`.
- **REQ-IO.CLOUD-010** When a point set is exported to E57 and re-imported, the coordinates shall be bit-identical.
- **REQ-IO.CLOUD-011** ASCII export shall write coordinates with enough decimal digits that re-import reproduces the f64 value exactly, unless the caller sets a fixed decimal count, which shall be recorded in the journal.

## Interfaces

- **Offers:** `import.cloud_ascii@1`, `import.cloud_e57@1`, `import.cloud_las@1` (LAS, LAZ, COPC), `import.cloud_ptx@1` and matching `export.cloud_*@1` commands, registered with `IO.REG`.
- **Consumes:** `IO.REG` harness and provenance; GEO point sets, chunked clouds and frames.

## Decomposition hints

ASCII (column mapping and roles), E57, LAS (LAS, LAZ and COPC together; shared scaled-integer handling), PTX, EXPORT.

## Open questions

- Will verifiers accept COPC's scaled-integer storage as evidence, or must evidence-grade point data be E57 f64? Belongs to TRUST/UNC; affects which export is the default.
- E57 library choice and its licence depend on the D-001 allowlist.

## Split rationale

One child per **format family**, each owning reader and writer, plus a shared STREAM child for out-of-core staging and export attribute selection. LAS, LAZ and COPC stay together because they share the scaled-integer model and the quantisation rules (REQ-IO.CLOUD-005/009).

Alternatives considered:
- **Separate COPC from LAS.** Rejected: COPC is LAZ 1.4 plus a hierarchy; separating them would duplicate the precision rules.
- **No STREAM child.** Rejected: REQ-IO.CLOUD-007 is a cross-format memory bound that each reader would otherwise solve differently.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| IO.CLOUD.ASCII | ASCII, CSV and XYZ point tables (leaf) | 001 002 008 011 | GOM.IMP.04, GOM.IMP.08, GOM.EXP.02, PCD.EXP.02, PW.IMP.01, PW.EXP.01 |
| IO.CLOUD.E57 | E57 reader and writer with multi-scan poses | 003 004 008 010 | PW.IMP.01, PW.EXP.01 |
| IO.CLOUD.LAS | LAS, LAZ and COPC with scaled-integer precision | 005 008 009 | PW.EXP.01 |
| IO.CLOUD.PTX | PTX reader with per-scan header transforms (leaf) | 006 | PW.IMP.01 |
| IO.CLOUD.STREAM | Streaming point sink and attribute selection | 007 008 | none |
