---
id: IO.RES.QIFR.RIN
title: QIF Results import as transcribed results
level: 4
parent: IO.RES.QIFR
kind: feature
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-IO.RES.QIFR-004, REQ-IO.RES.QIFR-005]
covers: []
depends_on: [IO.RES.TRANS.MODEL, IO.QIF.XML.CMD, IO.QIF.IDS.MAP, IO.QIF.IDS.EXT, IO.REG.ENV.IMPORT]
side_effect: W2
determinism: D0
cli: [fid import results-qif]
mcp: [import_results_qif]
journal: [import.results_qif@1]
updated: 2026-11-17
---

# IO.RES.QIFR.RIN: QIF Results import as transcribed results

## Purpose

`import.results_qif@1` reads a QIF Results document (from any system, including this one) through the validating QIF-XML path and turns each measured characteristic into a transcribed result, with the source software taken from the document's own software definitions. When the document carries this platform's `fx` result extensions, their uncertainty class, decision rule and outcome are kept in the `as_reported` block, so a round trip through QIF loses none of them.

## Scope

**In:**
- `import.results_qif@1` (W2) on the import skeleton, using the QIF-XML reader and validator
- Mapping measured characteristics (actual values, nominal links, status) → `SourceRow` → `transcribe`
- Source software name and version from the document header's application or software definitions [unverified: exact QIF 3.0 element names]
- Reading `fx:ResultProvenance`, `fx:Uncertainty`, `fx:Decision` into `as_reported`
- Evaluation basis `POINTS` or `FEATURES` when the document holds measured points or actual features for the characteristic
- Characteristic linking by QPId first, then by QIF characteristic name

**Out:**
- Restoring this platform's own signed records as non-transcribed records (a TRUST concern, a later layer; see open questions)
- QIF Statistics (`IO.RES.QIFR.STATS`)

## Requirements

- **REQ-IO.RES.QIFR.RIN-001** When a QIF Results document is imported, each measured characteristic shall become one transcribed result through `IO.RES.TRANS.MODEL`, with source software name and version from the document's software definitions, or `unknown` version when absent.
- **REQ-IO.RES.QIFR.RIN-002** If the document has no software definition and the caller gives no `source_software`, then the import shall fail with `source_software_required`.
- **REQ-IO.RES.QIFR.RIN-003** When the document carries `fx` result extensions for a characteristic, their uncertainty class, decision rule, outcome and evaluation basis shall be stored in `as_reported` unchanged.
- **REQ-IO.RES.QIFR.RIN-004** A document exported by `IO.RES.QIFR.ROUT` and re-imported shall give, for each result, value text, unit, and `as_reported` uncertainty class, decision rule and outcome equal to the exported record's fields.
- **REQ-IO.RES.QIFR.RIN-005** Results shall link to characteristics by QPId when present, else by name; unmatched results shall be dropped with `characteristic_not_found` unless `create_missing` is set.
- **REQ-IO.RES.QIFR.RIN-006** The document shall pass the full QIF-XML validation (`IO.QIF.XML.CMD`) before any result is staged.

## Acceptance criteria

- **AC-1** Given `fixtures/io/res/qifr/rin/third_party.qif` (software "VendorQ 3.1", 8 characteristics, 1 failing), when `fid import results-qif --source root://in/third_party.qif --json` runs, then 8 transcribed results with software `VendorQ`, version `3.1`, one `NONCONFORMS`.
- **AC-2** Given `fixtures/io/res/qifr/rin/own_export.qif` produced by `IO.RES.QIFR.ROUT` from `own_records.json`, when imported, then for every result the `as_reported` UEC, rule and outcome equal those in `own_records.json`, and the record status is `transcribed`.
- **AC-3** (error path) Given `no_software.qif` and no `--source-software`, then `source_software_required`, nothing journaled.
- **AC-4** (error path) Given `invalid.qif` failing XSD validation, then `qif_schema_invalid`.
- **AC-5** Given `with_points.qif` holding measured points for one characteristic, then that result has basis `POINTS`.
- **AC-6** (replay) Given AC-1's journal, when replayed exactly, then `IDENTICAL`.

## Design

- **Registry command:** `import.results_qif@1` W2; params `source: RootHandle`, `source_software?: String`, `source_version?: String`, `create_missing: bool = false`, `run?: RunId`, `stage?: StageId`, `dry_run: bool = false`; result `ImportReport` plus `{ record, results, dropped }`.
- **Data structures:** uses `TranscribedResult` and the QIF model; the QIF document itself is also kept (with its original-bytes blob) for traceability.
- **Algorithm:** QIF-XML import path (validate, bind, `IdMap`) without committing the QIF document separately → walk `MeasurementResults` → rows → `transcribe` → link → stage both the QIF document and the transcribed results in one transaction.
- **Errors:** `source_software_required`, `qif_schema_invalid`, `xml_forbidden_construct`, envelope errors.
- **Determinism:** D0.
- **Performance budget:** 10^4 characteristics in < 2 s [unverified until measured].
- **Crate:** `fid-io-res` (module `qifr::rin`).

## Test fixtures

Hand-written QIF Results documents under `fixtures/io/res/qifr/rin/` (CC-BY-4.0); `own_export.qif` generated by `IO.RES.QIFR.ROUT` tests; DMSC sample Results files from the `IO.CORP` manifest (Boost [unverified]).

## Build prompt

> In crate `fid-io-res`, add module `qifr::rin` registering `import.results_qif@1` (W2; CLI `fid import results-qif`, MCP `import_results_qif`). Validate and bind the document through the `fid-qif` XML import path (`qif_schema_invalid` before staging); take source software and version from the document's software definitions, else require `source_software` (`source_software_required`); map each measured characteristic to a `SourceRow` and call `trans::model::transcribe`; copy `fx` uncertainty class, rule, outcome and basis into `as_reported`; basis POINTS/FEATURES when the document holds them; link by QPId then name (`characteristic_not_found` unless `create_missing`); commit the QIF document and results in one transaction. CONSTITUTION §8: no panics, no unwrap. Tests AC-1..AC-6.

## Open questions

- Re-import of this platform's own QIF Results stays a transcription (as REQ-IO.RES.TRANS-002 requires); restoring them as original records would need a signature check by TRUST, a later layer that may call IO but not the reverse. This is how REQ-IO.RES.QIFR-005's "round-trip unchanged" is met: through the `as_reported` block.

## Children

_Leaf: none._
