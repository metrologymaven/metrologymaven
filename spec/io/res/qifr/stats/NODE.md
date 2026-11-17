---
id: IO.RES.QIFR.STATS
title: QIF Statistics import and export
level: 4
parent: IO.RES.QIFR
kind: feature
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-IO.RES.QIFR-002]
covers: []
depends_on: [IO.RES.TRANS.MODEL, IO.RES.TRANS.VIEW, IO.RES.QIFR.ROUT, IO.QIF.XML.CMD, IO.QIF.XML.WRITE, IO.REG.ENV.IMPORT, IO.REG.ENV.EXPORT]
side_effect: [W2, W1]
determinism: D0
cli: [fid import results-qif-stats, fid export results-qif-stats]
mcp: [import_results_qif_stats, export_results_qif_stats]
journal: [import.results_qif_stats@1, export.results_qif_stats@1]
updated: 2026-11-17
---

# IO.RES.QIFR.STATS: QIF Statistics import and export

## Purpose

Moves multi-part results through the QIF 3.0 Statistics application. Export writes the per-part values of several runs (stages or serials) for a set of characteristics as an XSD-valid QIF Statistics document. Import reads a third-party Statistics document as a historical series of transcribed results, one per part and characteristic, so SPC can continue across a migration. Statistics computed by the source are kept as reported text and never become values of record.

## Scope

**In:**
- `export.results_qif_stats@1` (W1, `fs_effect: write_root`): runs or stages × characteristics → QIF Statistics, part values only
- `import.results_qif_stats@1` (W2): per-part values → transcribed results grouped into one transcription record; summary statistics stored in `as_reported`
- Part identity (serial or stage label) kept as untrusted text

**Out:**
- Computing statistics, capability indices or control limits (RPT)
- Single-run QIF Results (`IO.RES.QIFR.RIN`, `IO.RES.QIFR.ROUT`)

## Requirements

- **REQ-IO.RES.QIFR.STATS-001** When multi-part results are exported, the output shall pass QIF-XML validation and hold one value per selected part and characteristic, with value text, unit and outcome.
- **REQ-IO.RES.QIFR.STATS-002** The exporter shall not compute or write summary statistics that the platform has not computed as values of record elsewhere, and shall write none in P1a.
- **REQ-IO.RES.QIFR.STATS-003** When a QIF Statistics document is imported, each per-part value shall become a transcribed result linked to the characteristic and carrying the part identity, all under one transcription record.
- **REQ-IO.RES.QIFR.STATS-004** Summary statistics found in an imported document shall be stored only as untrusted `as_reported` text on the transcription record and shall not create result values.
- **REQ-IO.RES.QIFR.STATS-005** Transcribed values in a Statistics export shall carry the transcription marking and pass the `IO.RES.TRANS.VIEW` conformance kit.

## Acceptance criteria

- **AC-1** Given `fixtures/io/res/qifr/stats/series.json` (5 stages × 4 characteristics), when `fid export results-qif-stats --stages S1..S5 --target root://out/s.qif --json` runs, then `s.qif` validates and holds 20 values.
- **AC-2** Given `fixtures/io/res/qifr/stats/third_party_stats.qif` (25 parts × 3 characteristics plus mean and standard deviation), when imported, then 75 transcribed results and one record whose `as_reported` holds the summary text, and no result value equals a mean.
- **AC-3** (error path) Given an invalid Statistics document, then `qif_schema_invalid` and nothing journaled.
- **AC-4** Given AC-1's export re-imported, then 20 transcribed results with value texts equal to the input.
- **AC-5** (replay) Given AC-1 and AC-2 journals, when replayed exactly, then `IDENTICAL`.

## Design

- **Registry commands:** `export.results_qif_stats@1` W1 `fs_effect: write_root`; params `stages: [StageId] | runs: [RunId]`, `characteristics?: [CharRef]`, `target`, `overwrite`, `dry_run`; result `ExportReport`. `import.results_qif_stats@1` W2; params `source`, `source_software?`, `create_missing: bool = false`, `dry_run`; result `ImportReport`.
- **Data structures:** reuses the QIF Results mapping table of `IO.RES.QIFR.ROUT` for per-value fields.
- **Algorithm:** export: gather values per (part, characteristic) → QIF Statistics model → validating writer. Import: validating QIF-XML path → per-part rows → `transcribe` with part identity.
- **Errors:** `qif_schema_invalid`, `source_software_required`, `no_results`, envelope errors.
- **Determinism:** D0.
- **Performance budget:** 10^5 values in < 3 s [unverified until measured].
- **Crate:** `fid-io-res` (module `qifr::stats`).

## Test fixtures

`fixtures/io/res/qifr/stats/` hand-written documents and JSON series (CC-BY-4.0); DMSC Statistics samples via `IO.CORP` if present (Boost [unverified]).

## Build prompt

> In crate `fid-io-res`, add module `qifr::stats` registering `export.results_qif_stats@1` (W1, `fs_effect: write_root`; CLI `fid export results-qif-stats`) and `import.results_qif_stats@1` (W2; CLI `fid import results-qif-stats`). Export: per-part values of selected stages or runs and characteristics into a QIF Statistics document via `fid-qif`'s validating writer, with no summary statistics; mark transcribed values via `trans::view`. Import: validate and bind through the QIF-XML path, turn each per-part value into a transcribed result under one record with the part identity, store summary statistics only as untrusted `as_reported` text. CONSTITUTION §8: no panics, no unwrap. Tests AC-1..AC-5.

## Open questions

- Whether RPT will want platform-computed statistics written into QIF Statistics later is open; REQ-002 keeps P1a to part values only so no unrecorded statistic leaves the platform.

## Children

_Leaf: none._
