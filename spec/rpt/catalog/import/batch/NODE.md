---
id: RPT.CATALOG.IMPORT.BATCH
title: Historical import command, duplicates and limits
level: 4
parent: RPT.CATALOG.IMPORT
kind: feature
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-RPT.CATALOG.IMPORT-002, REQ-RPT.CATALOG.IMPORT-003, REQ-RPT.CATALOG.IMPORT-004, REQ-RPT.CATALOG.IMPORT-005]
covers: []
depends_on: [RPT.CATALOG.IMPORT.MAP, RPT.CATALOG.STORE.REG, IO.RES.CSV, IO.RES.AQDEF, IO.RES.QIFR, IO.REG.ROOTS, IO.REG.LIMITS]
side_effect: W2
determinism: D0
cli: [fid stats import-history]
mcp: [stats_import_history]
journal: [stats.import_history@1]
updated: 2026-11-17
---

# RPT.CATALOG.IMPORT.BATCH: Historical import command, duplicates and limits

## Purpose

The `stats.import_history@1` command: takes one or more files (CSV with a column map, Q-DAS AQDEF, QIF Statistics) from approved import roots, skips files already imported, reads each with IO's results readers, maps the records with `RPT.CATALOG.IMPORT.MAP`, commits everything into the historical-results project as one journaled event, and registers that project with the catalog. Any malformed or oversized file fails the whole command closed.

## Scope

**In:** the command; file handles; format selection; SHA-256 duplicate detection against the project's import log; size and record limits; creating the historical-results project on first import; registration with the catalog.

**Out:** parsing (`IO.RES.CSV`, `IO.RES.AQDEF`, `IO.RES.QIFR`); mapping (`RPT.CATALOG.IMPORT.MAP`); indexing (`RPT.CATALOG.STORE.INDEX`, on the next catalog command).

## Requirements

- **REQ-RPT.CATALOG.IMPORT.BATCH-001** `stats.import_history@1` shall be side-effect class W2 and, for agent and script principals, accept files only as approved import-root handles.
- **REQ-RPT.CATALOG.IMPORT.BATCH-002** If any file is malformed or exceeds the configured size limit (default 1 GiB per file and 10 000 000 records per command [unverified defaults]), then the command shall fail, report the file and the first offending line or element, and journal nothing.
- **REQ-RPT.CATALOG.IMPORT.BATCH-003** If a file's SHA-256 digest is already in the historical project's import log, then the system shall skip it and report it as `duplicate`.
- **REQ-RPT.CATALOG.IMPORT.BATCH-004** When all files are valid, the system shall commit the mapped entities and records and the new import-log entries as one journaled event in the historical-results project.
- **REQ-RPT.CATALOG.IMPORT.BATCH-005** After a successful import, the historical-results project shall be registered with the catalog (if not already) so that the next catalog command indexes it.
- **REQ-RPT.CATALOG.IMPORT.BATCH-006** If no historical-results project exists for the catalog, then the first import shall create it at the configured location.

## Acceptance criteria

- **AC-1** Given fixtures `fixtures/rpt/catalog/history_csv_two_parts` and `history_qifstats_small.qif` in an approved root, when `fid stats import-history --file root://hist/a.csv --map root://hist/a.map.json --file root://hist/b.qif --json` runs, then one event is journaled in the historical project, the catalog lists that project, and the next `query-results` returns the imported records with basis `RESULTS_ONLY`.
- **AC-2** When the same command runs again, then both files are reported `duplicate` and nothing is journaled.
- **AC-3** (error path) Given a CSV with a malformed line 17 plus a valid QIF file, then the command fails naming line 17 and journals nothing.
- **AC-4** (error path) Given a file over the size limit, then the command fails with `stats.import_too_large` and journals nothing; given an agent principal with a raw path, then it is refused.
- **AC-5** (replay) Given the historical project journal after AC-1, when replayed exactly, then `IDENTICAL`.

## Design

- **Registry command:** `stats.import_history@1(files: Vec<ImportFile { file: FileHandle, format: Csv{map: FileHandle} | Aqdef | QifStats }>) -> { imported: Vec<FileReport>, duplicates: Vec<Sha256> }` (W2, `--json`, `--dry-run`).
- **Algorithm:** resolve handles → hash files, drop duplicates → check limits → read each with its IO reader → map (pure) → open or create the historical project → commit one event → register with the catalog.
- **Errors:** `stats.import_malformed { file, at }`, `stats.import_too_large { file }`, `stats.import_unmapped`, root resolution errors.
- **Determinism:** D0; files processed in the given order; IDs derived by the mapper.
- **Performance budget:** import 100 000 records in under 30 s [unverified until measured].
- **Crate:** `fid-catalog` (module `import::batch`).

## Test fixtures

- `fixtures/rpt/catalog/history_csv_two_parts`, `history_qifstats_small.qif`, and an AQDEF sample from the `IO.RES.AQDEF` fixtures; all synthetic, CC-BY-4.0.

## Build prompt

> In crate `fid-catalog`, module `import::batch`, implement `stats.import_history@1` (W2, `--json`, `--dry-run`; CLI `fid stats import-history`; MCP name derived). Accept files as approved import-root handles (raw paths only for humans), skip files whose SHA-256 is in the historical project's import log (`duplicate`), enforce limits (`stats.import_too_large`), read each file with the IO results reader for its format (CSV with column map, AQDEF, QIF Statistics), fail closed on the first malformed line or element of any file (`stats.import_malformed { file, at }`), map with the import mapper, create the historical-results project on first use, commit entities, records and import-log entries in one event, and register the project with the catalog. Tests AC-1 to AC-5. CONSTITUTION §8 and C6 apply. Spec: `spec/rpt/catalog/import/batch/NODE.md`.

## Open questions

- **Cross-domain:** `IO.RES.CSV`, `IO.RES.AQDEF` and `IO.RES.QIFR` are still open; this leaf assumes each offers a bounded reader returning transcription records with a first-error location.

## Children

_Leaf: none._
