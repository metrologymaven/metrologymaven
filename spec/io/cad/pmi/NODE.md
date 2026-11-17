---
id: IO.CAD.PMI
title: AP242 semantic PMI mapping to QIF MBD
level: 3
parent: IO.CAD
kind: module
status: open
phase: P1b
size: XL
owners: []
claimed_by:
satisfies: [REQ-IO.CAD-003, REQ-IO.CAD-004, REQ-IO.CAD-005, REQ-IO.CAD-007]
covers: [GOM.IMP.07, PW.IMP.04]
depends_on: [IO.CAD.STEP, IO.QIF.GEN, IO.QIF.IDS]
cli: [fid import cad-pmi]
mcp: [import_cad_pmi]
journal: [import.cad_pmi@1]
updated: 2026-11-16
---

# IO.CAD.PMI: AP242 semantic PMI mapping to QIF MBD

## Purpose

Maps STEP AP242 semantic PMI (dimensions, geometric tolerances with modifiers, datum features, datum systems) into QIF MBD characteristics, datums and datum reference frames linked to the imported CAD faces, and classifies everything it saw in an import report: mapped, graphic-only or unsupported. Nothing is dropped silently, and graphic-only PMI never becomes a characteristic.

## Scope

**In:**
- Reading semantic PMI entities and their links to shape aspects and faces
- Mapping to QIF MBD characteristic definitions, nominals and items; datum definitions and DRFs
- The PMI import report: `mapped`, `graphic_only`, `unsupported` with STEP entity number
- A proposed `SpecContext` from evidence in the file (never adopted automatically; adoption is GDT, human-only for agent proposals per C5)
- Conformance against the NIST MBE PMI test models via the CAD corpus

**Out:**
- Evaluating PMI (GDT), defaults profiles and the Dialect Mapping Table (GDT.RES, D-009)
- Geometry (`IO.CAD.STEP`), QIF types (`IO.QIF`)

## Requirements

- **REQ-IO.CAD.PMI-001** When an AP242 file carries semantic PMI, the system shall create one QIF characteristic per semantic tolerance or dimension, linked to the faces of its referenced shape aspects by their imported IDs.
- **REQ-IO.CAD.PMI-002** The system shall create QIF datum definitions and datum reference frames for every semantic datum feature and datum system, keeping precedence and modifiers.
- **REQ-IO.CAD.PMI-003** Graphic-only PMI (presentation without semantic representation) shall be listed in the import report as `graphic_only` and shall create no characteristic.
- **REQ-IO.CAD.PMI-004** If a semantic PMI item uses a construct the mapper does not support, then it shall be listed as `unsupported` with its STEP entity number and type, and the item shall not be dropped silently.
- **REQ-IO.CAD.PMI-005** Tolerance values shall keep their authored decimal form and unit.
- **REQ-IO.CAD.PMI-006** For each NIST MBE PMI test model in the CAD corpus, the mapped PMI shall match the expected characteristic list, and the per-model match rate shall be published by `IO.CORP` [unverified: availability and licence of the models].
- **REQ-IO.CAD.PMI-007** When the file gives evidence of a GD&T dialect, the importer shall attach a proposed `SpecContext` marked `proposed` and shall not commit it as adopted.

## Interfaces

- **Offers:** `import.cad_pmi@1` (W2; also run as part of `import.cad_step@1` when `pmi=true`); `PmiImportReport`.
- **Consumes:** STEP entity map (`IO.CAD.STEP`), QIF MBD types (`IO.QIF.GEN`), ID rules (`IO.QIF.IDS`).

## Decomposition hints

READ (semantic PMI entities), MAPTOL (tolerances and modifiers), MAPDAT (datums and DRFs), MAPDIM (dimensions), CLASS (report classification), CTX (spec-context proposal).

## Open questions

- Mapping rules for modifiers are standards interpretation; any ambiguity is a GDT/D-009 matter, not decided here.
- CAx-IF recommended practices and NIST SFA as oracles [unverified: licences and edition].

## Children

_None yet: status `open`._
