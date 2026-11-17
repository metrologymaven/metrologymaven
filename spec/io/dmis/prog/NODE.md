---
id: IO.DMIS.PROG
title: DMIS program import to a program model and QIF
level: 3
parent: IO.DMIS
kind: module
status: open
phase: P2
size: XL
owners: []
claimed_by:
satisfies: [REQ-IO.DMIS-004, REQ-IO.DMIS-005, REQ-IO.DMIS-006]
covers: []
depends_on: [IO.DMIS.PARSE, IO.QIF.GEN, IO.QIF.IDS, IO.REG.ENV]
cli: [fid import dmis-program]
mcp: [import_dmis_program]
journal: [import.dmis_program@1]
updated: 2026-11-16
---

# IO.DMIS.PROG: DMIS program import

## Purpose

Imports DMIS input programs (`.dmi`) into a typed DMIS program model plus QIF features, characteristics and plan items, so CMM can turn them into Command Programs. Every statement that is not supported is reported with its line; in strict mode, one unsupported statement fails the import.

## Scope

**In:**
- Program model: feature definitions, tolerance definitions, datum definitions, sensor and probe declarations, measurement blocks, output requests, control flow kept as model nodes
- Mapping of definitions to QIF features, characteristics and plan items
- QPId preservation on created entities
- `strict` mode
- `import.dmis_program@1` (W2)

**Out:**
- Turning the model into a Command Program and executing it (CMM)
- Run-time variables and loops (AUTO)

## Requirements

- **REQ-IO.DMIS.PROG-001** When a DMIS program is imported, every statement the importer does not support shall be listed in the import report with its line number and statement type.
- **REQ-IO.DMIS.PROG-002** If `strict` is set and the program contains an unsupported statement, then the import shall fail with `dmis_unsupported` and add nothing.
- **REQ-IO.DMIS.PROG-003** QPIds present in the program shall be preserved on the created QIF entities and never re-minted.
- **REQ-IO.DMIS.PROG-004** Each created QIF feature and characteristic shall reference the program line it came from.

## Interfaces

- **Offers:** `import.dmis_program@1` (W2), `DmisProgram` model for CMM.
- **Consumes:** `IO.DMIS.PARSE`, `IO.QIF` types and ID rules, `IO.REG` envelope.

## Decomposition hints

MODEL, FEAT, TOL, DATUM, SENS, MAPQIF, CMD.

## Open questions

- None beyond the parent's.

## Children

_None yet: status `open`._
