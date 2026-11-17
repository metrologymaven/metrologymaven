---
id: IO.RES.TRANS.MODEL
title: Transcription record model and value mapping
level: 4
parent: IO.RES.TRANS
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-IO.RES.TRANS-001, REQ-IO.RES.TRANS-002, REQ-IO.RES.TRANS-003, REQ-IO.RES.TRANS-004]
covers: []
depends_on: [CORE.PRJ.TREE, CORE.PRJ.CONT, CORE.JRN.CANON]
uses_artifacts: [GDT.CHAR.RECORD]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# IO.RES.TRANS.MODEL: Transcription record model and value mapping

## Purpose

The one definition of how a third-party result becomes a platform record. A `TranscriptionRecord` holds what is known about the source file (software, version, digest, import time); each `TranscribedResult` carries the CONSTITUTION §5 minimum fields with record status `transcribed`, UEC-0, decision rule `dr:as-reported-by-source@1`, null kernel and algorithm fields, the evaluation basis, a mapped outcome, and an `as_reported` block that keeps what the source said verbatim. Every results importer calls `transcribe`, so the rules hold for every source.

## Scope

**In:**
- Types `TranscriptionRecord`, `TranscribedResult`, `AsReported` and their canonical JSON schema `transcribed-result@1`
- `transcribe(rows, source) -> Vec<TranscribedResult>` with the evaluation-basis and outcome rules
- Registration of both types as element types in the project tree
- Field names aligned with the ResultRecord canonical JSON published by `GDT.CHAR.RECORD` (read as an artifact, not a code dependency, because GDT is a higher L2 domain)

**Out:**
- The ResultRecord builder for evaluated results (`GDT.CHAR.RECORD`)
- Display and export marking (`IO.RES.TRANS.VIEW`)
- Format parsing (`IO.RES.QIFR`, `IO.RES.CSV`, `IO.RES.AQDEF`, `IO.RES.TRANS.DMIS`)

## Requirements

- **REQ-IO.RES.TRANS.MODEL-001** When `transcribe` runs for a source file, it shall create exactly one transcription record holding the source software name, the source software version or `unknown`, the file SHA-256 and the import time from the bus clock.
- **REQ-IO.RES.TRANS.MODEL-002** Each transcribed result shall carry record status `transcribed`, uncertainty class UEC-0 with no uncertainty value, decision rule `dr:as-reported-by-source@1`, and null kernel version and algorithm ID.
- **REQ-IO.RES.TRANS.MODEL-003** Each transcribed result shall carry `evaluation_basis` `RESULTS_ONLY`, unless the row states that the source file also holds that value's measured points (`POINTS`) or fitted features (`FEATURES`).
- **REQ-IO.RES.TRANS.MODEL-004** The source status shall map as pass → `CONFORMS`, fail → `NONCONFORMS`, an explicit undecided status → `INDETERMINATE`, and no status → `NOT_DECIDABLE`.
- **REQ-IO.RES.TRANS.MODEL-005** Each transcribed result shall keep an `as_reported` block with the source's value text, unit text, status text, and any uncertainty, decision rule or outcome the source states, all as untrusted lexical text.
- **REQ-IO.RES.TRANS.MODEL-006** Values shall keep their authored decimal text and unit and shall not pass through binary floating point inside this mapping.
- **REQ-IO.RES.TRANS.MODEL-007** If a row has no characteristic identifier or no value, then `transcribe` shall return a dropped-item entry for it with reason `row_incomplete` and continue.

## Acceptance criteria

- **AC-1** Given `fixtures/io/res/trans/model/rows_basic.json` (5 rows: pass, fail, undecided, no status, one with points flag) and source `{name: "VendorX", version: null}`, when `transcribe` runs, then one record has version `unknown`, the outcomes are `CONFORMS`, `NONCONFORMS`, `INDETERMINATE`, `NOT_DECIDABLE`, `CONFORMS`, and the last has basis `POINTS`.
- **AC-2** Given the same, then every result has status `transcribed`, UEC-0 without value, rule `dr:as-reported-by-source@1`, null kernel and algorithm, and canonical JSON equal to `rows_basic.out.json`.
- **AC-3** Given a row with value `0.0100` and unit `mm`, then the result value text is `0.0100` and unit `mm`.
- **AC-4** Given a row whose source states uncertainty `U=0.002 mm`, then the result is still UEC-0 and `as_reported.uncertainty` is `U=0.002 mm`.
- **AC-5** (error path) Given a row without a characteristic identifier, then it is listed as dropped with `row_incomplete` and the other rows are mapped.

## Design

- **Registry commands:** none (library used by every results import command).
- **Data structures:** `TranscriptionRecord { id: EntityId, source_software, source_version: String | "unknown", file_sha256, imported_at, format }`; `TranscribedResult { id, record: EntityId, characteristic: CharRef, measurand, evaluation_basis, value: Lexical, unit, uec: UEC0, decision: { rule: "dr:as-reported-by-source@1", outcome }, spec_context: Option<..>, algorithm: null, kernel_version: null, input_digests: [file_sha256], status: transcribed, as_reported: AsReported }`; `SourceRow` input type.
- **Algorithm:** pure mapping; IDs from the CORE ID service (UUIDv5 from file digest and row key, so re-importing the same file yields the same IDs); canonical JSON via `fid-canon`.
- **Errors:** dropped rows only; no hard errors.
- **Determinism:** D0.
- **Performance budget:** 10^5 rows in < 200 ms [unverified until measured].
- **Crate:** `fid-io-res` (module `trans::model`).

## Test fixtures

`fixtures/io/res/trans/model/` JSON rows and golden output (CC-BY-4.0).

## Build prompt

> In crate `fid-io-res`, add module `trans::model`: types `TranscriptionRecord` (source software, version or `unknown`, file SHA-256, import time, format) and `TranscribedResult` carrying the CONSTITUTION §5 ResultRecord fields with status `transcribed`, UEC-0 without value, rule `dr:as-reported-by-source@1`, null algorithm and kernel version, evaluation basis `RESULTS_ONLY` unless the row flags points or features, outcome mapping pass/fail/undecided/none → CONFORMS/NONCONFORMS/INDETERMINATE/NOT_DECIDABLE, and an `as_reported` block of untrusted lexical text. `transcribe(rows, source)` creates one record per file, maps rows (incomplete rows dropped with `row_incomplete`), keeps decimal text and units (no f64), derives IDs as UUIDv5 of file digest and row key via the CORE ID service, and registers both element types. Publish schema `transcribed-result@1`, field names aligned with the GDT ResultRecord canonical JSON. CONSTITUTION §8: no panics, no unwrap. Tests AC-1..AC-5.

## Open questions

- **Cross-domain:** the ResultRecord type is owned by `GDT.CHAR.RECORD`, a later L2 domain than IO, so IO may not depend on it. This leaf defines the transcribed variant in IO and only reads GDT's published canonical JSON as an artifact. A shared ResultRecord schema in CORE (L1) would remove the duplication; GDT should publish `result-record` as a versioned schema file.

## Children

_Leaf: none._
