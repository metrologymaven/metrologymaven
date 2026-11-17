---
id: IO.DMIS.EXPORT
title: DMIS output and program export
level: 3
parent: IO.DMIS
kind: module
status: open
phase: P4
size: L
owners: []
claimed_by:
satisfies: [REQ-IO.DMIS-009]
covers: [PCD.EXP.01]
depends_on: [IO.DMIS.PARSE, IO.REG.WRITE, IO.DMIS.CONF]
cli: [fid export dmis-output, fid export dmis-program]
mcp: [export_dmis_output, export_dmis_program]
journal: [export.dmis_output@1, export.dmis_program@1]
updated: 2026-11-16
---

# IO.DMIS.EXPORT: DMIS output and program export

## Purpose

Writes DMIS output files from features and characteristic results, and DMIS programs from features and plans, to the extent the conformance table declares. The concept roadmap puts DMIS export in P4 [unverified against later roadmap changes].

## Scope

**In:**
- Output-file writer from ResultRecords and features
- Program writer from features, characteristics and plan items
- Writing only statements declared supported in the conformance table
- `export.dmis_output@1`, `export.dmis_program@1` (W1, `fs_effect: write_root`)

**Out:**
- Parsing and printing primitives (`IO.DMIS.PARSE`)

## Requirements

- **REQ-IO.DMIS.EXPORT-001** The system shall export DMIS output for features and characteristics using only statements the conformance table declares supported for export.
- **REQ-IO.DMIS.EXPORT-002** If an entity selected for export has no supported DMIS mapping, then the export shall fail with `dmis_unsupported` naming it, unless `skip_unsupported` is set, in which case it shall be listed in the export report.
- **REQ-IO.DMIS.EXPORT-003** Every exported file shall parse with `IO.DMIS.PARSE` without `Unknown` nodes.
- **REQ-IO.DMIS.EXPORT-004** QPIds of exported entities shall be written where DMIS 5.3 allows them.

## Interfaces

- **Offers:** `export.dmis_output@1`, `export.dmis_program@1` (W1 write_root).
- **Consumes:** `IO.DMIS.PARSE` printer, `IO.DMIS.CONF` support declarations, `IO.REG.WRITE`.

## Decomposition hints

OUTW (output writer), PROGW (program writer).

## Open questions

- Phase P4 follows the concept roadmap; maintainers may pull output export earlier for PCD.EXP.01 parity.

## Children

_None yet: status `open`._
