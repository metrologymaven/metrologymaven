---
id: IO.DMIS.EXPORT.OUTW
title: DMIS output-file export from features and results
level: 4
parent: IO.DMIS.EXPORT
kind: feature
status: leaf
phase: P4
size: M
owners: []
claimed_by:
satisfies: [REQ-IO.DMIS.EXPORT-001, REQ-IO.DMIS.EXPORT-002, REQ-IO.DMIS.EXPORT-003, REQ-IO.DMIS.EXPORT-004]
covers: [PCD.EXP.01]
depends_on: [IO.DMIS.PARSE.PRINT, IO.DMIS.PARSE.GDEF, IO.QIF.GEN, IO.REG.ENV, IO.REG.WRITE]
uses_artifacts: [IO.DMIS.CONF]
side_effect: W1
determinism: D0
cli: [fid export dmis-output]
mcp: [export_dmis_output]
journal: [export.dmis_output@1]
updated: 2026-11-17
---

# IO.DMIS.EXPORT.OUTW: DMIS output-file export

## Purpose

`export.dmis_output@1` writes a DMIS output file (`.dmo`) from measured features and characteristic results, so downstream systems that consume DMIS output (statistics packages, incumbent reporting chains) can take the platform's results. It writes only statements whose descriptors declare export support, and every file it writes parses back without `Unknown` nodes.

## Scope

**In:**
- Header statements (`DMISMN`, `FILNAM`, `UNITS`, `ENDFIL`) with the platform's name and version as producing software
- Feature actuals (`FA(label)=FEAT/...`) from measured features; tolerance definitions (`T(label)`) and results (`TA(label)`) from characteristics and their result records
- Decision mapping: `CONFORMS` → in-tolerance flag, `NONCONFORMS` → out-of-tolerance flag; `INDETERMINATE` and `NOT_DECIDABLE` have no DMIS flag, so the result is written without a flag and listed in the export report as `decision_not_representable`
- Values written in the chosen unit (`mm` or `inch`) from the authored decimal when units match, else converted once and the conversion recorded in the report
- QPIds written where the statement form allows them
- `skip_unsupported` and the export report

**Out:**
- Program export (`IO.DMIS.EXPORT.PROGW`)
- Printing (`IO.DMIS.PARSE.PRINT`)

## Requirements

- **REQ-IO.DMIS.EXPORT.OUTW-001** The exporter shall emit only statement classes whose support declaration states export `full` or `partial`.
- **REQ-IO.DMIS.EXPORT.OUTW-002** If a selected feature or characteristic has no supported DMIS output mapping, then the export shall fail with `dmis_unsupported` naming it, unless `skip_unsupported` is set, in which case it shall be listed in the export report.
- **REQ-IO.DMIS.EXPORT.OUTW-003** Every file written shall parse with the DMIS parser without `Unknown` nodes, and this shall be checked before the file is committed to the export root.
- **REQ-IO.DMIS.EXPORT.OUTW-004** QPIds of exported entities shall be written where the statement form allows them, and entities whose QPId cannot be written shall be listed in the report.
- **REQ-IO.DMIS.EXPORT.OUTW-005** A result whose decision is `INDETERMINATE` or `NOT_DECIDABLE` shall be written without an in- or out-of-tolerance flag and listed as `decision_not_representable`.

## Acceptance criteria

- **AC-1** Given `fixtures/io/dmis/export/outw/run_small.qif` (4 measured features, 6 characteristic results: 4 CONFORMS, 1 NONCONFORMS, 1 INDETERMINATE), when `fid export dmis-output --run <id> --unit mm --target <handle> --json` runs, then the file parses without `Unknown` nodes, holds 4 `FA` and 6 `TA` statements with 4 in-tolerance and 1 out-of-tolerance flag, and the report lists 1 `decision_not_representable`.
- **AC-2** Given the AC-1 file read back with `IO.DMIS.OUT`, then labels, values and flags round-trip.
- **AC-3** (error path) Given a characteristic kind without an export declaration, then `dmis_unsupported` and no file is written; with `--skip-unsupported` it is listed.
- **AC-4** Given entities with QPIds, then the written statements carry them and `IO.DMIS.OUT` reads them back unchanged.
- **AC-5** Given the same export twice (same journal timestamp), then files are byte-identical.

## Design

- **Registry commands:** `export.dmis_output@1` (W1, `fs_effect: write_root`; `run` or `results: [EntityId]`, `target`, `unit: mm|inch`, `skip_unsupported?`, `overwrite?`, `dry_run?`; result `ExportReport`). CLI `fid export dmis-output`, MCP `export_dmis_output`.
- **Algorithm:** collect entities → map to generic `Statement` nodes using GDEF descriptors (check export declaration) → print → re-parse check → atomic write.
- **Errors:** `dmis_unsupported`, `target_exists`, `raw_path_forbidden`, `path_outside_root`.
- **Determinism:** D0.
- **Performance budget:** 10^4 results in < 1 s [unverified until measured].
- **Crate:** `fid-io-dmis`.

## Test fixtures

Project-written QIF fixtures under `fixtures/io/dmis/export/outw/` (CC-BY-4.0).

## Build prompt

> In crate `fid-io-dmis`, implement `export.dmis_output@1` (W1, `fs_effect: write_root`; CLI `fid export dmis-output`; MCP `export_dmis_output`) on the `fid-io-reg` export skeleton. From measured features and characteristic results (QIF model types) build header, `FA`, `T` and `TA` statements using only descriptors that declare export support; map CONFORMS/NONCONFORMS to in/out-of-tolerance flags and write INDETERMINATE/NOT_DECIDABLE without a flag, listed as `decision_not_representable`; write QPIds where allowed; unit mm or inch (authored decimals when units match, else one conversion, reported). Unmapped → `dmis_unsupported` unless `skip_unsupported`. Print, re-parse to confirm no `Unknown` nodes, then atomic write. CONSTITUTION §8: no panic, no unwrap. Tests AC-1..AC-5.

## Open questions

- Phase P4 follows the concept roadmap; PCD.EXP.01 parity would need it earlier (carried from IO.DMIS.EXPORT).

## Children

_Leaf: none._
