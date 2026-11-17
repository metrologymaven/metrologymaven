---
id: IO
title: Import, export and interoperability
level: 1
parent: FID
kind: domain
status: split
phase: P0
size: XXL
owners: []
claimed_by:
satisfies: [REQ-FID-001, REQ-FID-010]
covers: [GOM.IMP.*, GOM.EXP.*, PW.IMP.*, PW.EXP.*, PCD.CAD.01, PCD.EXP.*, SA.CAD.01]
depends_on: [CORE, GEO]
cli: [fid import *, fid export *]
mcp: [import_*, export_*]
journal: [import.*, export.*]
updated: 2026-11-16
---

# IO: Import, export and interoperability

## Purpose

Bounded, fail-closed readers and writers for every format the platform touches. QIF 3.0 is the native model, and every other format maps onto it.

## Scope

**In:**
- QIF 3.0: XSD-driven code generation, plus a lossless QIF-JSON form
- Meshes: STL, PLY, OBJ, 3MF
- Point clouds: ASCII/CSV, E57, LAS/LAZ, COPC, PTX
- CAD: STEP AP242 geometry and semantic PMI, and IGES; native CAD through optional translators
- DMIS 5.3 import and export
- Results: CSV, Q-DAS AQDEF, PiWeb, QIF Results and Statistics
- Import of third-party results as transcription records
- Format corpora and round-trip fidelity metrics

**Out:**
- Geometry types (GEO)
- Turning DMIS into Command Programs (CMM)
- Report layout (RPT)
- Live device streams (HAL)

## Requirements

- **REQ-IO-001** Every reader shall be bounded and fail closed on hostile input: no panic, bounded recursion and allocation. A malformed file shall produce a typed error.
- **REQ-IO-002** The QIF 3.0 model shall be generated from the official XSDs, and QIF-XML ↔ QIF-JSON conversion shall be lossless up to a documented normalizer.
- **REQ-IO-003** The system shall import STL, PLY, OBJ, ASCII point clouds, E57 and LAS/LAZ into GEO elements with units and frames preserved.
- **REQ-IO-004** The system shall import STEP AP242 geometry and semantic PMI (characteristics, datums, tolerances) into the QIF model.
- **REQ-IO-005** The system shall import and export DMIS 5.3 programs and results to the extent documented in a conformance table.
- **REQ-IO-006** The system shall export results as CSV, QIF Results and Q-DAS AQDEF, and meshes and points in the formats it imports.
- **REQ-IO-007** Results from third-party software shall be imported as transcription records that keep the source software, version and file digest, and shall never be presented as re-evaluated.
- **REQ-IO-008** Every format shall have a published round-trip and fidelity score against a public test corpus, regenerated in CI.
- **REQ-IO-009** Import and export commands shall read and write only through approved import and export roots when invoked by agents or scripts.

## Interfaces

- **Offers:** `import.*` and `export.*` commands, the QIF model crate, a format registry for plugins.
- **Consumes:** GEO types, CORE registry and journal.

## Decomposition hints

QIF (codegen + JSON), MESH, CLOUD, CAD (STEP/IGES + PMI; native translators as plugins), DMIS, RES (result and SPC formats, transcription records), CORP (corpora and fidelity scoring).

## Split rationale

The split is by **format family**, with two cross-cutting children. Every format family (QIF, meshes, point clouds, CAD, DMIS, results) has its own specs, corpora, libraries and phase, so each can be owned and scheduled on its own. The safety and traceability rules that must be identical for every format (limits, fail-closed errors, roots, provenance, units, plugin readers) go into one framework child, IO.REG, that the others depend on. Evidence (corpora, fuzzing, fidelity scores) goes into IO.CORP so that no format owner grades its own work.

Alternatives considered:
- **Split by direction** (import vs export). Rejected: reader and writer for one format share the model mapping, corpus and round-trip score, so splitting them would duplicate format knowledge.
- **Split by data role** (nominal, actual, results). Rejected: several formats carry more than one role (PLY meshes and points, QIF MBD and Results, DMIS programs and outputs), so ownership would blur.
- **Fold IO.REG and IO.CORP into each format child.** Rejected: REQ-IO-001 and REQ-IO-009 must hold uniformly, and per-format copies would drift.
- **Merge IO.MESH and IO.CLOUD.** Rejected: clouds need streaming, out-of-core import and scaled-integer precision handling that meshes don't, and the combined node would be too large.

## Open questions

- Native CAD formats need licensed translators. Plugin-only, or out of scope? Carried into IO.CAD (REQ-IO.CAD-011 assumes plugin-only); a decision should be opened when IO.CAD is expanded.
- GOM.EXP.03 (PDF report export) overlaps RPT's PDF output. IO.RES covers it only as an export envelope; maintainers may prefer to move the seed to RPT.
- The side-effect class of writing an export file to a local export root (R0 or X1) is not fixed by the constitution; see IO.REG.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| IO.REG | Format registry and safe-read framework | 001 009 | GOM.IMP.03, GOM.IMP.09 |
| IO.QIF | QIF 3.0 model, XML and lossless QIF-JSON | 001 002 | none |
| IO.MESH | Mesh formats: STL, PLY, OBJ, 3MF | 001 003 006 | GOM.IMP.01/02, GOM.EXP.01, PW.IMP.02, PW.EXP.01 |
| IO.CLOUD | Point-cloud formats: ASCII/CSV, E57, LAS/LAZ, COPC, PTX | 001 003 006 | GOM.IMP.04/08, GOM.EXP.02, PW.IMP.01, PW.EXP.01, PCD.EXP.02 |
| IO.CAD | CAD exchange: STEP AP242 with semantic PMI, IGES, native translators | 001 004 | GOM.IMP.05/06/07, GOM.EXP.07, PCD.CAD.01, PW.IMP.03/04, PW.EXP.03, SA.CAD.01 |
| IO.DMIS | DMIS 5.3 parsing, import and export | 001 005 | PCD.EXP.01 |
| IO.RES | Results and SPC formats, transcription records | 001 006 007 | GOM.EXP.02/03/04/05/06, PW.IMP.05, PW.EXP.02/04, PCD.EXP.03 |
| IO.CORP | Format corpora, fuzzing and fidelity scores | 001 008 | none |
