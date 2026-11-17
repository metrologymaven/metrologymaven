---
id: RPT.CATALOG.IMPORT
title: Historical results import
level: 3
parent: RPT.CATALOG
kind: module
status: open
phase: P1b
size: L
owners: []
claimed_by:
satisfies: [REQ-RPT.CATALOG-005, REQ-RPT.CATALOG-006]
covers: []
depends_on: [IO.RES, IO.REG, RPT.CATALOG.STORE, CORE.PRJ]
cli: [fid stats import-history]
mcp: [stats_import_history]
journal: [stats.import_history@1]
updated: 2026-11-16
---

# RPT.CATALOG.IMPORT: Historical results import

## Purpose

Brings years of results from earlier systems into the catalog: CSV (with a column map), Q-DAS AQDEF and QIF Statistics files are read by IO's results readers and stored as transcription records in a dedicated historical-results project, which is then registered with the catalog. RPT owns the mapping into projects, parts and runs; IO owns parsing.

## Scope

**In:**
- Historical-results project: created on first import, one per catalog by default
- Mapping of imported files to parts, revisions, runs and characteristics, preserving source characteristic IDs, timestamps and source-software name
- Duplicate-file detection by SHA-256
- Fail-closed behaviour and size limits

**Out** (and which node owns it instead):
- Parsing CSV, AQDEF and QIF files and creating transcription ResultRecords (`IO.RES`)
- Approved import roots and handles (`IO.REG`)

## Requirements

- **REQ-RPT.CATALOG.IMPORT-001** When the user imports historical results, the system shall store them as transcription records with basis `RESULTS_ONLY` in a historical-results project, preserving the source file's characteristic IDs, timestamps and source-software name.
- **REQ-RPT.CATALOG.IMPORT-002** `stats.import_history@1` shall be side-effect class W2 and shall accept files only through approved import-root handles for agent principals.
- **REQ-RPT.CATALOG.IMPORT-003** If an import file is malformed or exceeds the configured size limit, then the import shall fail closed, report the first offending line or element, and journal nothing.
- **REQ-RPT.CATALOG.IMPORT-004** If a file with the same SHA-256 digest was already imported into the same historical-results project, then the system shall skip it and report it as a duplicate.
- **REQ-RPT.CATALOG.IMPORT-005** After a successful import, the historical-results project shall be registered with the catalog and indexed on the next catalog command.

## Interfaces

- **Offers:** `stats.import_history@1` (W2).
- **Consumes:** results readers and transcription records (`IO.RES`), import roots (`IO.REG`), catalog registration (`RPT.CATALOG.STORE`), project model (`CORE.PRJ`).

## Decomposition hints

MAP (file to part, run and characteristic mapping), BATCH (multi-file import, duplicates and limits).

## Open questions

- Should historical imports go into one historical project per part number instead of one per catalog? Leave to the MAP leaf.

## Children

_None yet: status `open`._
