---
id: IO.CAD.STEP
title: STEP B-rep geometry import
level: 3
parent: IO.CAD
kind: module
status: split
phase: P1b
size: XL
owners: []
claimed_by:
satisfies: [REQ-IO.CAD-001, REQ-IO.CAD-002, REQ-IO.CAD-006]
covers: [GOM.IMP.05, PCD.CAD.01, PW.IMP.03, SA.CAD.01]
depends_on: [IO.CAD.WORKER, IO.REG.ENV, GEO.CAD]
cli: [fid import cad-step]
mcp: [import_cad_step]
journal: [import.cad_step@1]
updated: 2026-11-17
---

# IO.CAD.STEP: STEP B-rep geometry import

## Purpose

Imports STEP (ISO 10303) B-rep geometry, AP242 first and AP203/AP214 geometry files as well, into GEO CAD bodies with the face, edge and shell names and identifiers the file carries. The original STEP bytes stay the geometry source of truth, stored by digest, and every approximation of an exact surface is measured and recorded per face.

## Scope

**In:**
- Schema detection from the STEP header (`FILE_SCHEMA`) and the accepted schema list
- Assembly structure (products, instances, placements) as frames and a body tree
- Faces, edges, vertices with STEP entity numbers and names; colours and layers as untrusted metadata
- Length unit from the file's unit context; no `unit` argument needed
- Original file as a content-addressed blob linked from each body
- Per-face approximation error where the kernel converts an exact surface to NURBS or another representation
- `import.cad_step@1` (W2)

**Out:**
- PMI (`IO.CAD.PMI`), worker isolation (`IO.CAD.WORKER`), B-rep types and tessellation (GEO.CAD)

## Requirements

- **REQ-IO.CAD.STEP-001** When a STEP file is imported, the system shall create one GEO CAD body per solid or shell, keeping each face's and edge's STEP entity number and name.
- **REQ-IO.CAD.STEP-002** When a STEP file is imported, the original bytes shall be stored as a content-addressed blob and every created body shall reference that blob's SHA-256.
- **REQ-IO.CAD.STEP-003** The importer shall accept AP242 and the AP203 and AP214 geometry schemas, and if `FILE_SCHEMA` names another schema, then the import shall fail with `step_schema_unsupported` [unverified: final schema list].
- **REQ-IO.CAD.STEP-004** Where the kernel approximates an exact surface, the importer shall record the maximum distance between the exact and approximated surface per face, in metres.
- **REQ-IO.CAD.STEP-005** Assembly placements shall be recorded as frames and shall not be baked into body coordinates.
- **REQ-IO.CAD.STEP-006** The importer shall use the file's declared length unit and record it in the provenance.

## Interfaces

- **Offers:** `import.cad_step@1` (W2); `StepImportReport` (bodies, faces, approximations, skipped entities); STEP entity-number map consumed by `IO.CAD.PMI`.
- **Consumes:** `IO.CAD.WORKER`, `IO.REG` envelope, GEO.CAD body types, CORE.STO blobs.

## Decomposition hints

HDR (header, schema, units), ASM (assembly structure to frames), BREP (bodies, topology, names), APPROX (approximation error), CMD.

## Open questions

- Which STEP schemas besides AP242 are required [unverified; carried from IO.CAD]?
- Large assemblies (10^5 faces) and memory: the worker cap must be tuned on the CAx-IF corpus [unverified].
- D-024 (accepted, option C) applied: STEP parsing, geometry mapping and B-rep building are first-party Rust (`fid-io-step`) running inside the isolated CAD worker; OCCT or other healers are optional plugins only (`IO.CAD.XLATE`, `IO.REG.PLUGIN`), never on this path. The time-boxed spike on adopting an existing Rust B-rep crate (option D) concerns GEO.CAD.TESS, not this reader.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| IO.CAD.STEP.P21 | Bounded ISO 10303-21 exchange-structure parser and printer (leaf) | 001 003 | none |
| IO.CAD.STEP.HDR | STEP schema detection and unit contexts (leaf) | 003 006 | none |
| IO.CAD.STEP.GEOM | STEP curve and surface mapping with approximation error (leaf) | 004 | none |
| IO.CAD.STEP.BREP | STEP B-rep topology to GEO CAD bodies (leaf) | 001 | none |
| IO.CAD.STEP.ASM | STEP product structure and placements (leaf) | 005 | none |
| IO.CAD.STEP.CMD | STEP import command, source blob and body commit (leaf) | 001 002 | GOM.IMP.05, PCD.CAD.01, PW.IMP.03, SA.CAD.01 |
