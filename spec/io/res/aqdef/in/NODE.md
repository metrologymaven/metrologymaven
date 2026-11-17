---
id: IO.RES.AQDEF.IN
title: AQDEF DFQ import as historical transcribed results
level: 4
parent: IO.RES.AQDEF
kind: feature
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-IO.RES.AQDEF-003]
covers: []
depends_on: [IO.RES.AQDEF.KSET, IO.RES.TRANS.MODEL, IO.REG.ENV.IMPORT]
side_effect: W2
determinism: D0
cli: [fid import results-aqdef]
mcp: [import_results_aqdef]
journal: [import.results_aqdef@1]
updated: 2026-11-17
---

# IO.RES.AQDEF.IN: AQDEF DFQ import as historical transcribed results

## Purpose

`import.results_aqdef@1` reads a combined DFQ file as a historical series of transcribed results, so SPC continues across a migration from an incumbent. It reads the K-fields in the published subset table, keeps every other K-field it meets as untrusted raw text on the transcription record (reported, never dropped silently), and takes the source software from the file when present or from the caller.

## Scope

**In:**
- `import.results_aqdef@1` (W2) on the import skeleton
- Combined DFQ parsing: bounded line reader, K-field lines with indices, value lines
- Mapping per `aqdef-kset@1`: part → part identity, characteristic → characteristic link, value lines → `SourceRow`s with stage or part identity
- Unknown K-fields kept as `(K-number, index, raw text)` on the record and counted in the report

**Out:**
- DFD/DFX pairs (not read in this leaf; see open questions)
- Export (`IO.RES.AQDEF.OUT`)

## Requirements

- **REQ-IO.RES.AQDEF.IN-001** Each value in a DFQ file shall become a transcribed result through `IO.RES.TRANS.MODEL`, with the source software from the file's fields when the subset table maps one and otherwise from the required `source_software` parameter.
- **REQ-IO.RES.AQDEF.IN-002** If the source software is neither in the file nor given, then the import shall fail with `source_software_required`.
- **REQ-IO.RES.AQDEF.IN-003** K-fields not in the subset table shall be kept as untrusted raw text on the transcription record and counted in the import report as `kfield_unmapped`.
- **REQ-IO.RES.AQDEF.IN-004** The reader shall run under `IO.REG.LIMITS` with a 64 KiB line cap and fail with `malformed` naming the line for any line that is not a valid K-field or value line.
- **REQ-IO.RES.AQDEF.IN-005** Value texts shall be stored exactly as written in the file, after decimal-mark normalisation recorded in the report.

## Acceptance criteria

- **AC-1** Given `fixtures/io/res/aqdef/in/history.dfq` (2 characteristics × 50 values, software field present), when `fid import results-aqdef --source root://in/history.dfq --json` runs, then 100 transcribed results under one record with that software.
- **AC-2** (error path) Given `no_software.dfq` and no `--source-software`, then `source_software_required`.
- **AC-3** Given `extra_fields.dfq` with 4 K-fields outside the table, then the record keeps them as raw text and the report counts `kfield_unmapped: 4`.
- **AC-4** (error path) Given `garbage_line.dfq` with a malformed line 12, then `malformed` naming line 12, nothing journaled.
- **AC-5** Given the output of `IO.RES.AQDEF.OUT` AC-1, when imported, then value texts equal the exported ones.
- **AC-6** (replay) Given AC-1's journal, when replayed exactly, then `IDENTICAL`.

## Design

- **Registry command:** `import.results_aqdef@1` W2; params `source: RootHandle`, `source_software?: String`, `source_version?: String`, `create_missing: bool = false`, `run?: RunId`, `dry_run: bool = false`; result `ImportReport` plus `{ record, results, unmapped }`.
- **Data structures:** `DfqLine { k: u16, index: Option<u32>, raw: UntrustedText }`; `RawKField` on the record.
- **Algorithm:** bounded line reader with encoding detection per the format description [unverified]; parse K-lines and value lines; group by characteristic index; map via the table; `transcribe`; link characteristics by number and name.
- **Errors:** `source_software_required`, `malformed`, `limit_exceeded`, envelope errors.
- **Determinism:** D0.
- **Performance budget:** 10^6 values in < 5 s [unverified until measured].
- **Crate:** `fid-io-res` (module `aqdef::in`).

## Test fixtures

Hand-written DFQ files under `fixtures/io/res/aqdef/in/` from the public format description (CC-BY-4.0); anonymised partner exports when available [unverified].

## Build prompt

> In crate `fid-io-res`, add module `aqdef::in` registering `import.results_aqdef@1` (W2; CLI `fid import results-aqdef`, MCP `import_results_aqdef`) on the import skeleton. Parse combined DFQ with a bounded line reader (64 KiB line cap; `malformed` with line number), map K-fields through `aqdef-kset@1`, keep unmapped K-fields as untrusted raw text on the record and count them (`kfield_unmapped`), take source software from the file or require it (`source_software_required`), keep value texts (decimal-mark normalisation recorded), and map values through `trans::model::transcribe`. CONSTITUTION §8: no panics, no unwrap. Tests AC-1..AC-6.

## Open questions

- Reading separate DFD/DFX pairs needs a two-file source handle; deferred until a user needs it [unverified need] (carried from IO.RES.AQDEF).

## Children

_Leaf: none._
