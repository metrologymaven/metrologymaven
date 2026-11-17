---
id: IO.RES.AQDEF
title: AQDEF DFQ export and historical import (uncertified)
level: 3
parent: IO.RES
kind: module
status: split
phase: P1b
size: L
owners: []
claimed_by:
satisfies: [REQ-IO.RES-010, REQ-IO.RES-013]
covers: [GOM.EXP.04, PW.EXP.04]
depends_on: [IO.RES.TRANS, IO.REG.ENV, IO.REG.WRITE]
cli: [fid import results-aqdef, fid export results-aqdef]
mcp: [import_results_aqdef, export_results_aqdef]
journal: [import.results_aqdef@1, export.results_aqdef@1]
updated: 2026-11-17
---

# IO.RES.AQDEF: AQDEF DFQ export and historical import

## Purpose

Writes results as AQDEF DFQ files built only from publicly documented K-fields, so SPC systems that read AQDEF can take the platform's results, and reads DFQ files as historical transcription records so SPC continues across a migration. Every output and its documentation say plainly that it is not Q-DAS certified.

## Scope

**In:**
- K-field subset table (part, characteristic, value fields) with a source citation for each field's public documentation [unverified: which fields are publicly documented]
- DFQ (combined) writer and reader; DFD/DFX pair reading [unverified: need]
- "Not Q-DAS certified" label in the file comment field where available, in the export report and in docs
- Commands listed in frontmatter

**Out:**
- Certification (a commercial matter; see open questions)

## Requirements

- **REQ-IO.RES.AQDEF-001** The exporter shall write only K-fields listed in the published subset table, each with a public documentation source.
- **REQ-IO.RES.AQDEF-002** Every AQDEF export shall state "not Q-DAS certified" in its export report and, where the format has a free-text field, in the file.
- **REQ-IO.RES.AQDEF-003** When a DFQ file is imported, each value shall become a transcribed value with source software from the file's fields when present or from the caller otherwise.
- **REQ-IO.RES.AQDEF-004** Where the subset has a field for decision outcome or a free-text attribute, the exporter shall write the record status and `evaluation_basis` there.

## Interfaces

- **Offers:** the two commands; the K-field subset table.
- **Consumes:** `IO.RES.TRANS`, `IO.REG` envelope and writer.

## Decomposition hints

KSET (subset table), WRITE, READ.

## Open questions

- AQDEF is owned by a competitor; certification terms and fees are [unverified]. VDA QDX as a neutral alternative is not in IO's scope (carried from IO.RES).
- Which K-fields are publicly documented, and under what terms, is [unverified]; `IO.RES.AQDEF.KSET` makes the citation mandatory per row, so the gap is visible in CI rather than hidden.
- DFD/DFX pair reading is deferred (`IO.RES.AQDEF.IN`).
- The `piweb` profile of `IO.RES.AQDEF.OUT` is reused by `IO.RES.PIWEB`.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| IO.RES.AQDEF.KSET | Published AQDEF K-field subset table (leaf) | 001 | none |
| IO.RES.AQDEF.OUT | AQDEF DFQ export, uncertified (leaf) | 001 002 004 | GOM.EXP.04, PW.EXP.04 |
| IO.RES.AQDEF.IN | AQDEF DFQ import as historical transcribed results (leaf) | 003 | none |
