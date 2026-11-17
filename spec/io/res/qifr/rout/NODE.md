---
id: IO.RES.QIFR.ROUT
title: QIF Results export of ResultRecords
level: 4
parent: IO.RES.QIFR
kind: feature
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-IO.RES.QIFR-001, REQ-IO.RES.QIFR-003]
covers: [PCD.EXP.03]
depends_on: [IO.RES.TRANS.VIEW, IO.QIF.XML.WRITE, IO.QIF.IDS.MAP, IO.QIF.IDS.EXT, IO.REG.ENV.EXPORT]
uses_artifacts: [GDT.CHAR.RECORD, CORE.PRJ.QIF]
side_effect: W1
determinism: D0
cli: [fid export results-qif]
mcp: [export_results_qif]
journal: [export.results_qif@1]
updated: 2026-11-17
---

# IO.RES.QIFR.ROUT: QIF Results export of ResultRecords

## Purpose

`export.results_qif@1` writes a run's ResultRecords (evaluated and transcribed) as an XSD-valid QIF 3.0 Results document. Value, unit and outcome go into QIF's native measurement-result fields; `evaluation_basis`, uncertainty class, decision rule and transcription status go into native fields where QIF has them and otherwise into the sanctioned `fx` extensions defined by CORE. This is the open, verifiable results export (PCD.EXP.03).

## Scope

**In:**
- `export.results_qif@1` (W1, `fs_effect: write_root`) on the export skeleton
- Reading ResultRecords in their published canonical JSON (GDT) and transcribed results (`IO.RES.TRANS.MODEL`)
- Field mapping table `qif-results-map@1` (record field → QIF native element or `fx` element), versioned
- Identifier use: characteristic and feature QPIds kept; new entities numbered through `IO.QIF.IDS.MAP`
- Transcribed results marked through `IO.RES.TRANS.VIEW` (`fx:ResultProvenance` with status and source software)

**Out:**
- XML serialisation and validation (`IO.QIF.XML.WRITE`); the `fx` schema (`CORE.PRJ.QIF`)
- QIF Statistics (`IO.RES.QIFR.STATS`)

## Requirements

- **REQ-IO.RES.QIFR.ROUT-001** When ResultRecords are exported, the output document shall pass QIF-XML validation and carry each record's value, unit, uncertainty class, decision rule and outcome.
- **REQ-IO.RES.QIFR.ROUT-002** `evaluation_basis`, uncertainty class and decision rule shall be written in a native QIF field where the mapping table names one, and otherwise in the `fx` extension the table names.
- **REQ-IO.RES.QIFR.ROUT-003** Every transcribed result shall carry its transcription marking, and the export shall pass the `IO.RES.TRANS.VIEW` conformance kit.
- **REQ-IO.RES.QIFR.ROUT-004** Value text shall be written as the authored decimal text without reformatting.
- **REQ-IO.RES.QIFR.ROUT-005** Exporting the same records twice shall produce byte-identical files.
- **REQ-IO.RES.QIFR.ROUT-006** If the selected run has no ResultRecords, then the export shall fail with `no_results`.

## Acceptance criteria

- **AC-1** Given `fixtures/io/res/qifr/rout/own_records.json` (6 evaluated records, 2 transcribed), when `fid export results-qif --run R --target root://out/r.qif --json` runs, then `r.qif` validates, holds 8 results with the values, units, UEC, rules and outcomes of the input, and the journal holds one event with the file digest.
- **AC-2** Given AC-1's output, then the two transcribed results carry `fx:ResultProvenance` with status `transcribed` and their source software, and the conformance kit passes.
- **AC-3** Given a record with value `0.0250`, then the output text is `0.0250`.
- **AC-4** Given AC-1 run twice, then the files are byte-identical.
- **AC-5** (error path) Given an empty run, then `no_results`; given an existing target without `--overwrite`, then `target_exists`.
- **AC-6** (replay) Given AC-1's journal replayed into an empty root, then `IDENTICAL`.

## Design

- **Registry command:** `export.results_qif@1` W1 `fs_effect: write_root`; params `run: RunId`, `characteristics?: [CharRef]`, `target: RootHandle`, `overwrite: bool = false`, `dry_run: bool = false`; result `ExportReport`.
- **Data structures:** `QifResultsMap` loaded from `qif-results-map@1.toml`, with a CI check that every ResultRecord field of CONSTITUTION §5 has a row.
- **Algorithm:** load records → build QIF Results model (header with this software's name and version, measured characteristics, `fx` items) → `IO.QIF.XML.WRITE` (validates) → export skeleton.
- **Errors:** `no_results`, `qif_export_invalid`, envelope errors.
- **Determinism:** D0.
- **Performance budget:** 10^4 records in < 2 s [unverified until measured].
- **Crate:** `fid-io-res` (module `qifr::rout`).

## Test fixtures

`fixtures/io/res/qifr/rout/` record sets in GDT canonical JSON and transcribed JSON (CC-BY-4.0).

## Build prompt

> In crate `fid-io-res`, add module `qifr::rout` registering `export.results_qif@1` (W1, `fs_effect: write_root`; CLI `fid export results-qif`, MCP `export_results_qif`). Read the run's ResultRecords (GDT canonical JSON) and transcribed results, map them with a versioned table `qif-results-map@1` (native QIF fields first, then CORE `fx` extensions for evaluation basis, UEC, decision rule and provenance), keep authored decimal text, mark transcribed results via `trans::view` and run its conformance kit, write through `fid-qif`'s validating writer and the export skeleton. Fail `no_results` for an empty run. Byte-identical output for identical input. CONSTITUTION §8: no panics, no unwrap. Tests AC-1..AC-6.

## Open questions

- Which ResultRecord fields have native QIF 3.0 homes (for example decision rule and uncertainty class) is [unverified] until the mapping table is checked against the pinned XSDs.

## Children

_Leaf: none._
