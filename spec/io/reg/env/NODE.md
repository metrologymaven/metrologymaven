---
id: IO.REG.ENV
title: Generic import and export command envelope
level: 3
parent: IO.REG
kind: module
status: split
phase: P0
size: L
owners: []
claimed_by:
satisfies: [REQ-IO.REG-007, REQ-IO.REG-013]
covers: [GOM.IMP.09]
depends_on: [IO.REG.DETECT, IO.REG.LIMITS, IO.REG.PROV, IO.REG.ROOTS, IO.REG.WRITE, CORE.BUS, CORE.PRJ]
cli: [fid import file, fid export file]
mcp: [import_file, export_file]
journal: [import.file@1, export.file@1]
updated: 2026-11-17
---

# IO.REG.ENV: Generic import and export command envelope

## Purpose

The `import.file@1` and `export.file@1` registry commands and the shared skeleton that every format-specific `import.*` and `export.*` command reuses: resolve the source or target, detect or check the format, run the reader under limits, build the entities off-document, then commit them as one bus transaction into the chosen stage, or commit nothing. Format children only supply a reader or writer function.

## Scope

**In:**
- `import.file@1` (W2) and `export.file@1` (W1, `fs_effect: write_root`, CONSTITUTION §3.1) and their dispatch to registered readers and writers
- The format-command skeleton used by every `import.<format>@1` and `export.<format>@1`
- All-or-nothing import: entities staged off-document and committed in one transaction
- Target stage selection, including creating entities as a new stage's data (further measurements as new stages)
- The import report type (counts, warnings, dropped items) and its return in `--json`
- Dry-run: parse and report without committing

**Out:**
- Detection (`IO.REG.DETECT`), limits and errors (`IO.REG.LIMITS`), provenance and units (`IO.REG.PROV`), root handles (`IO.REG.ROOTS`), atomic file writes (`IO.REG.WRITE`)
- Stage creation itself (CORE.PRJ `project.add_stage@1`)

## Requirements

- **REQ-IO.REG.ENV-001** When any import command fails at any step, the system shall leave the working document byte-identical to its state before the command and shall journal no committed event.
- **REQ-IO.REG.ENV-002** When an import succeeds, the system shall add all created entities in one bus transaction producing exactly one journal event.
- **REQ-IO.REG.ENV-003** When an import names a target stage, the system shall add the imported entities to that stage only, and every other stage's data shall be unchanged.
- **REQ-IO.REG.ENV-004** If a named target stage does not exist, then the import shall fail with error `stage_not_found`.
- **REQ-IO.REG.ENV-005** When an import or export runs with `dry_run`, the system shall return the full import or export report and shall neither change the document nor write a file.
- **REQ-IO.REG.ENV-006** The import report shall list entity counts by type, warnings and every dropped item with its source position.
- **REQ-IO.REG.ENV-007** When an import is cancelled, the system shall behave as for a failed import (REQ-IO.REG.ENV-001).

## Interfaces

- **Offers:** `import.file@1` (W2), `export.file@1` (W1 write_root); a `FormatCommand` skeleton (generic over a reader or writer) that format crates instantiate to get `import.<format>@1` and `export.<format>@1` with identical semantics; `ImportReport`, `ExportReport`.
- **Consumes:** CORE.BUS transactions and journal, CORE.PRJ stages; siblings named in `depends_on`.

## Decomposition hints

IMPORT (skeleton, staging, transaction, stage target), EXPORT (writer dispatch through `IO.REG.WRITE`), REPORT (report types, dry-run).

## Open questions

- Large imports (10^8 points) stage payloads as CORE blobs that become reachable only on commit (`IO.REG.ENV.IMPORT`). Garbage collection of blobs left unreferenced by failed, cancelled or dry-run imports must be provided by `CORE.STO.CAS` or `CORE.STO.SQL`; neither scope names it yet.
- "Import further measurements as new stages" (GOM.IMP.09) is an import into an existing named stage; creating the stage stays with `CORE.PRJ.STAGE`, so one import still yields exactly one journal event.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| IO.REG.ENV.REPORT | Import and export report types (leaf) | 006 | none |
| IO.REG.ENV.IMPORT | Generic import command and transactional import skeleton (leaf) | 001 002 003 004 005 007 | GOM.IMP.09 |
| IO.REG.ENV.EXPORT | Generic export command and export skeleton (leaf) | 005 | none |
