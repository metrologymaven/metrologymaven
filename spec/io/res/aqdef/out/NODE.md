---
id: IO.RES.AQDEF.OUT
title: AQDEF DFQ export (uncertified)
level: 4
parent: IO.RES.AQDEF
kind: feature
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-IO.RES.AQDEF-001, REQ-IO.RES.AQDEF-002, REQ-IO.RES.AQDEF-004]
covers: [GOM.EXP.04, PW.EXP.04]
depends_on: [IO.RES.AQDEF.KSET, IO.RES.TRANS.VIEW, IO.REG.ENV.EXPORT, IO.REG.WRITE]
uses_artifacts: [GDT.CHAR.RECORD]
side_effect: W1
determinism: D0
cli: [fid export results-aqdef]
mcp: [export_results_aqdef]
journal: [export.results_aqdef@1]
updated: 2026-11-17
---

# IO.RES.AQDEF.OUT: AQDEF DFQ export (uncertified)

## Purpose

`export.results_aqdef@1` writes ResultRecords and transcribed results as a combined DFQ file so SPC systems that read AQDEF can take the platform's results. It writes only K-fields from the published subset table, puts record status and evaluation basis in a free-text field where the subset has one, and says "not Q-DAS certified" in its report and in the file. Profiles let other exporters (PiWeb) reuse it with a narrower field set.

## Scope

**In:**
- `export.results_aqdef@1` (W1, `fs_effect: write_root`) on the export skeleton
- Combined DFQ layout: part block, characteristic blocks, value lines for each part or stage
- Profile parameter (`default`, `piweb`) selecting the K-field subset
- Text encoding and line endings per the public format description [unverified]
- Field-length truncation with a report entry, never silent

**Out:**
- The K-field table (`IO.RES.AQDEF.KSET`); import (`IO.RES.AQDEF.IN`)
- Separate DFD/DFX pairs (not written)

## Requirements

- **REQ-IO.RES.AQDEF.OUT-001** The exporter shall write only K-fields in the chosen profile of the published subset table.
- **REQ-IO.RES.AQDEF.OUT-002** Every export report shall state `not Q-DAS certified`, and the file shall carry the same statement in a free-text field when the profile has one.
- **REQ-IO.RES.AQDEF.OUT-003** When the profile has a free-text field at characteristic or value level, the exporter shall write the record status, evaluation basis and, for transcribed results, the source software there.
- **REQ-IO.RES.AQDEF.OUT-004** Values shall be written from their authored decimal text, and if a K-field's maximum length would cut a value or text, then the value shall be truncated only for text fields, reported as `kfield_truncated`, and a decimal value that does not fit shall fail the export with `kfield_value_too_long`.
- **REQ-IO.RES.AQDEF.OUT-005** Exporting the same records with the same profile twice shall give byte-identical files.

## Acceptance criteria

- **AC-1** Given `fixtures/io/res/aqdef/out/records.json` (3 characteristics × 5 stages, one transcribed) and profile `default`, when `fid export results-aqdef --stages S1..S5 --target root://out/r.dfq --json` runs, then `r.dfq` equals `r.golden.dfq`, the report states `not Q-DAS certified`, and no K-field outside the profile appears.
- **AC-2** Given the transcribed result, then its free-text field holds `transcribed`, its evaluation basis and its source software.
- **AC-3** Given a characteristic name longer than its K-field limit, then it is truncated and the report lists `kfield_truncated`.
- **AC-4** (error path) Given a value whose text exceeds the value field length, then `kfield_value_too_long` and no file.
- **AC-5** Given `r.dfq` read back with `IO.RES.AQDEF.IN`, then the value texts equal the input.
- **AC-6** (replay) Given AC-1's journal replayed into an empty root, then `IDENTICAL`.

## Design

- **Registry command:** `export.results_aqdef@1` W1 `fs_effect: write_root`; params `run?: RunId`, `stages?: [StageId]`, `characteristics?: [CharRef]`, `profile: default|piweb = default`, `target`, `overwrite: bool = false`, `dry_run: bool = false`; result `ExportReport`.
- **Data structures:** `DfqWriter { profile, encoding, eol }`.
- **Algorithm:** gather records → part block → per characteristic, characteristic block with K-fields of that level → value lines per stage in stage order → encode → atomic write.
- **Errors:** `kfield_value_too_long`, `no_results`, envelope errors; warning `kfield_truncated`.
- **Determinism:** D0.
- **Performance budget:** 10^5 values in < 1 s [unverified until measured].
- **Crate:** `fid-io-res` (module `aqdef::out`).

## Test fixtures

`fixtures/io/res/aqdef/out/` records and golden DFQ files written from the public format description (CC-BY-4.0).

## Build prompt

> In crate `fid-io-res`, add module `aqdef::out` registering `export.results_aqdef@1` (W1, `fs_effect: write_root`; CLI `fid export results-aqdef`, MCP `export_results_aqdef`) on the export skeleton. Write a combined DFQ file using only the K-fields of the chosen profile in `aqdef-kset@1` (`default` or `piweb`); part block, characteristic blocks, value lines per stage; values from authored decimal text (`kfield_value_too_long` if a value does not fit; text fields truncated with `kfield_truncated`); record status, evaluation basis and source software in a free-text field when the profile has one; `not Q-DAS certified` in the report and in the file where possible; run the `trans::view` conformance kit. Byte-identical output for identical input. CONSTITUTION §8: no panics, no unwrap. Tests AC-1..AC-6.

## Open questions

- Text encoding and line-ending rules of DFQ are [unverified]; take them from the public format description and record them in `aqdef-kset@1`.

## Children

_Leaf: none._
