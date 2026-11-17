---
id: IO.RES.TRANS.DMIS
title: DMIS results import command
level: 4
parent: IO.RES.TRANS
kind: feature
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-IO.RES.TRANS-006]
covers: [PW.IMP.05]
depends_on: [IO.RES.TRANS.MODEL, IO.DMIS.OUT, IO.REG.ENV.IMPORT]
side_effect: W2
determinism: D0
cli: [fid import results-dmis]
mcp: [import_results_dmis]
journal: [import.results_dmis@1]
updated: 2026-11-17
---

# IO.RES.TRANS.DMIS: DMIS results import command

## Purpose

`import.results_dmis@1`, the single command that imports a DMIS output file (`.dmo`) from any CMM software as transcribed results. `IO.DMIS.OUT` reads the file into typed rows; this leaf takes the source software from the file's header statements (or requires it from the caller), maps each tolerance result through `transcribe`, links rows to characteristics by label, and commits through the import envelope. This is the P1a path for reporting on incumbent CMM results.

## Scope

**In:**
- `import.results_dmis@1` (W2) on the import skeleton
- Source software from header statements when present, else the required `source_software` parameter
- Row mapping: label, nominal, actual, deviation, limits and out-of-tolerance flag → `SourceRow`; out-of-tolerance flag → status (absent flag → no status)
- Characteristic linking by DMIS label to existing characteristics, or creation of transcribed-only characteristics when `create_missing` is set
- Evaluation basis `FEATURES` for rows whose actual feature appears in the same file, otherwise `RESULTS_ONLY`

**Out:**
- DMIS lexing, parsing and output-statement reading (`IO.DMIS.OUT`, `IO.DMIS.PARSE`)
- Program import (`IO.DMIS.PROG`)

## Requirements

- **REQ-IO.RES.TRANS.DMIS-001** When a DMIS output file is imported, the source software shall be taken from the file's header statements when present, and otherwise the command shall require `source_software` and fail with `source_software_required` if it is missing.
- **REQ-IO.RES.TRANS.DMIS-002** Each tolerance result row shall become one transcribed result through `IO.RES.TRANS.MODEL`, with outcome `NONCONFORMS` when the file flags it out of tolerance, `CONFORMS` when it reports a within-tolerance result explicitly, and `NOT_DECIDABLE` when no status is written.
- **REQ-IO.RES.TRANS.DMIS-003** Each row shall link to the characteristic whose label equals the DMIS label; if none matches and `create_missing` is false, then the row shall be dropped with reason `characteristic_not_found`.
- **REQ-IO.RES.TRANS.DMIS-004** A row whose actual feature is reported in the same file shall carry evaluation basis `FEATURES`.
- **REQ-IO.RES.TRANS.DMIS-005** The import shall follow the import envelope: one journal event on success, nothing on failure, `dry_run` supported.
- **REQ-IO.RES.TRANS.DMIS-006** Units and angle units declared in the file shall be carried as the unit text of each value without conversion.

## Acceptance criteria

- **AC-1** Given `fixtures/io/res/trans/dmis/vendor_a.dmo` (header naming the software, 12 tolerance results, 2 out of tolerance) and a project with matching characteristic labels, when `fid import results-dmis --source root://in/vendor_a.dmo --json` runs, then 12 transcribed results are committed, 2 `NONCONFORMS`, source software from the header, one journal event.
- **AC-2** (error path) Given `no_header.dmo` and no `--source-software`, then the command fails `source_software_required` and journals nothing.
- **AC-3** Given `no_header.dmo` and `--source-software "VendorB 7"`, then the record's software is `VendorB 7`.
- **AC-4** Given a label with no matching characteristic and no `--create-missing`, then that row is dropped with `characteristic_not_found` and the rest are committed.
- **AC-5** Given `inch.dmo` declaring inch units, then value units read `inch` and value texts are unchanged.
- **AC-6** (replay) Given AC-1's journal, when replayed exactly, then `IDENTICAL`.

## Design

- **Registry command:** `import.results_dmis@1` W2; params `source: RootHandle`, `source_software?: String`, `source_version?: String`, `create_missing: bool = false`, `run?: RunId`, `stage?: StageId`, `dry_run: bool = false`; result `ImportReport` plus `{ record: EntityId, results: u64, dropped: u64 }`.
- **Data structures:** none new beyond `IO.RES.TRANS.MODEL` types.
- **Algorithm:** `IO.DMIS.OUT` → header lookup [unverified: which DMIS statements carry the producing software's name and version] → rows → `transcribe` → link by label → stage → commit.
- **Errors:** `source_software_required`, envelope errors; drops with `characteristic_not_found`, `row_incomplete`.
- **Determinism:** D0.
- **Performance budget:** 10^4 results imported in < 1 s [unverified until measured].
- **Crate:** `fid-io-res` (module `trans::dmis`).

## Test fixtures

Synthetic `.dmo` files under `fixtures/io/res/trans/dmis/` written by hand from the DMIS 5.3 grammar (CC-BY-4.0); real anonymised exports from partners when available [unverified].

## Build prompt

> In crate `fid-io-res`, add module `trans::dmis` registering `import.results_dmis@1` (W2; CLI `fid import results-dmis`, MCP `import_results_dmis`) on the `fid-io-reg` import skeleton. Read the file with `IO.DMIS.OUT`'s `read_dmis_output`; take source software from header statements or require `source_software` (`source_software_required`); map tolerance rows (out-of-tolerance → NONCONFORMS, explicit in-tolerance → CONFORMS, none → NOT_DECIDABLE) through `trans::model::transcribe`; basis FEATURES when the actual feature is in the same file; link by label, dropping unmatched rows with `characteristic_not_found` unless `create_missing`; keep unit text unconverted. One journal event; nothing on failure; `dry_run`. CONSTITUTION §8: no panics, no unwrap. Tests AC-1..AC-6.

## Open questions

- Whether DMIS 5.3 output files carry the producing software's name and version in a standard statement is [unverified]; if not, AC-1's header case applies only to files that use a comment or `DMISMD`-style statement, and the caller parameter is the normal path.

## Children

_Leaf: none._
