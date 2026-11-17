---
id: IO.DMIS.EXPORT.PROGW
title: DMIS program writer and program export from QIF definitions
level: 4
parent: IO.DMIS.EXPORT
kind: feature
status: leaf
phase: P4
size: M
owners: []
claimed_by:
satisfies: [REQ-IO.DMIS.EXPORT-001, REQ-IO.DMIS.EXPORT-002, REQ-IO.DMIS.EXPORT-003, REQ-IO.DMIS.EXPORT-004]
covers: []
depends_on: [IO.DMIS.PARSE.PRINT, IO.DMIS.PARSE.GDEF, IO.DMIS.PARSE.GEXEC, IO.DMIS.PROG.MODEL, IO.QIF.GEN, IO.REG.ENV, IO.REG.WRITE]
uses_artifacts: [IO.DMIS.CONF]
side_effect: W1
determinism: D0
cli: [fid export dmis-program]
mcp: [export_dmis_program]
journal: [export.dmis_program@1]
updated: 2026-11-17
---

# IO.DMIS.EXPORT.PROGW: DMIS program writer and program export

## Purpose

Two things that share one mapping: a library writer that turns a `DmisProgram` model (as produced by `IO.DMIS.PROG.MODEL` or by `CMM.GEN.DMIS` from a Command Program) into DMIS text, and the `export.dmis_program@1` command that writes a definitions-only DMIS program (features, tolerances, datums) from the QIF plan of a part. Motion and measurement blocks come only from a model; this command does not invent them.

## Scope

**In:**
- `write_program(&DmisProgram) -> Result<String>`: model → generic statements via GDEF and GEXEC descriptors (export declarations checked) → printer
- `export.dmis_program@1` (W1): QIF features → `F(label)=FEAT/...`, characteristics → `T(label)=TOL/...`, datums → `D(label)=DATDEF/...`, header and units, in the plan's order
- QPIds written where allowed; unsupported entities handled per `skip_unsupported`
- Re-parse check before committing the file

**Out:**
- Command Program → DMIS conversion and its report (`CMM.GEN.DMIS`, which calls `write_program`)
- Output files (`IO.DMIS.EXPORT.OUTW`)

## Requirements

- **REQ-IO.DMIS.EXPORT.PROGW-001** The writer shall emit only statement classes whose support declaration states export `full` or `partial`.
- **REQ-IO.DMIS.EXPORT.PROGW-002** If a model node or QIF entity has no supported DMIS mapping, then the export shall fail with `dmis_unsupported` naming it (by model node ID or entity ID), unless `skip_unsupported` is set, in which case it shall be listed in the export report.
- **REQ-IO.DMIS.EXPORT.PROGW-003** Every program written shall parse with the DMIS parser without `Unknown` nodes, checked before commit.
- **REQ-IO.DMIS.EXPORT.PROGW-004** QPIds on model nodes and QIF entities shall be written where the statement form allows them.
- **REQ-IO.DMIS.EXPORT.PROGW-005** When a program imported with `import.dmis_program@1` is written back with `write_program`, re-imported and compared, the two program models shall be equal except for source positions, for every corpus program whose statements are all export-supported.

## Acceptance criteria

- **AC-1** Given `fixtures/io/dmis/export/progw/plan_small.qif` (5 features, 3 datums, 4 characteristics), when `fid export dmis-program --plan <id> --target <handle> --json` runs, then the file parses without `Unknown` nodes and holds 5 `F`, 3 `D` and 4 `T` statements in plan order.
- **AC-2** Given `fixtures/io/dmis/parse/gexec/program.dmi` imported to a model and written with `write_program`, then re-import gives an equal model (positions ignored).
- **AC-3** (error path) Given a characteristic kind without an export declaration, then `dmis_unsupported`; with `--skip-unsupported`, it is listed.
- **AC-4** Given entities with QPIds, then the written statements carry them.
- **AC-5** Given the same export twice, then files are byte-identical.

## Design

- **Registry commands:** `export.dmis_program@1` (W1, `fs_effect: write_root`; `plan`, `target`, `unit: mm|inch`, `skip_unsupported?`, `overwrite?`, `dry_run?`; result `ExportReport`). CLI `fid export dmis-program`, MCP `export_dmis_program`. Library `write_program` for `CMM.GEN.DMIS`.
- **Algorithm:** map → check export declarations → print → re-parse → atomic write.
- **Errors:** `dmis_unsupported`, `target_exists`, `raw_path_forbidden`, `path_outside_root`.
- **Determinism:** D0.
- **Performance budget:** 10^4 statements in < 1 s [unverified until measured].
- **Crate:** `fid-io-dmis`.

## Test fixtures

Project-written QIF plans under `fixtures/io/dmis/export/progw/` (CC-BY-4.0); round-trip programs from `IO.DMIS.PARSE.GEXEC` fixtures.

## Build prompt

> In crate `fid-io-dmis`, implement `write_program(&DmisProgram)` (model → generic statements via GDEF/GEXEC descriptors with export declarations checked → printer) and `export.dmis_program@1` (W1, `fs_effect: write_root`; CLI `fid export dmis-program`; MCP `export_dmis_program`) that writes header, units and `F`/`D`/`T` definitions from a QIF plan in plan order, with QPIds where allowed. Unmapped → `dmis_unsupported` unless `skip_unsupported`. Re-parse before commit (no `Unknown` nodes); atomic write; byte-identical output. Round-trip test: import → write → import gives an equal model. CONSTITUTION §8: no panic, no unwrap. Tests AC-1..AC-5.

## Open questions

- None beyond the parent's.

## Children

_Leaf: none._
