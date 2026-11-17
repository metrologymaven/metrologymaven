---
id: CMM.GEN.DMIS
title: DMIS program conversion to and from Command Programs
level: 3
parent: CMM.GEN
kind: module
status: open
phase: P2
size: XL
owners: []
claimed_by:
satisfies: [REQ-CMM.GEN-007, REQ-CMM.GEN-008]
covers: []
depends_on: [IO.DMIS, AUTO.PROG, CMM.GEN.SEQ, CMM.GEN.PROV]
cli: [fid cmm convert-dmis, fid cmm convert-program-to-dmis]
mcp: [cmm_convert_dmis, cmm_convert_program_to_dmis]
journal: [cmm.convert_dmis@1, cmm.convert_program_to_dmis@1]
updated: 2026-11-16
---

# CMM.GEN.DMIS: DMIS program conversion to and from Command Programs

## Purpose

Converts a parsed DMIS 5.3 measurement program (from `IO.DMIS`) into a draft Command Program, with a conversion report that lists, by DMIS line number, every statement that was mapped one-to-one, approximated or dropped. It also owns the reverse direction: exporting a Command Program back to DMIS through the `IO.DMIS` writer, with a matching report of steps that have no DMIS equivalent. Ownership of the export half was assigned to `CMM.GEN` in the level-2 review (seed PCD.PRG.10, export half).

## Scope

**In:**
- Mapping from the DMIS program model to Command Program steps, using the DMIS mapping table owned by `AUTO.PROG`
- DMIS conversion report (per statement: `MAPPED`, `APPROXIMATED`, `DROPPED`, with reason)
- Reverse mapping from Command Program steps to the DMIS program model, and the export report
- `cmm.convert_dmis@1` (W2 draft) and `cmm.convert_program_to_dmis@1` (W1, `fs_effect: write_root`, per CONSTITUTION §3.1)

**Out** (and which node owns it instead):
- DMIS lexing, parsing and writing (owned by `IO.DMIS`)
- The DMIS mapping table itself (owned by `AUTO.PROG`)
- Sequence assembly of converted features (owned by `CMM.GEN.SEQ`)

## Requirements

- **REQ-CMM.GEN.DMIS-001** When converting a parsed DMIS 5.3 program, the system shall produce a draft Command Program and a conversion report that classifies every DMIS statement, by line number, as `MAPPED`, `APPROXIMATED` or `DROPPED`, with a reason for the last two.
- **REQ-CMM.GEN.DMIS-002** The system shall never silently drop a motion-relevant statement (GOTO, PTMEAS, tip and head selection, clearance settings): each such statement shall appear in the report and, if dropped, shall make the conversion result `INCOMPLETE`.
- **REQ-CMM.GEN.DMIS-003** When exporting a Command Program to DMIS, the system shall produce a DMIS program through the `IO.DMIS` writer and an export report listing every step without a DMIS equivalent.
- **REQ-CMM.GEN.DMIS-004** Conversion in both directions shall be deterministic: the same input digest, mapping-table version and generator version shall give byte-identical output and report.
- **REQ-CMM.GEN.DMIS-005** Export shall write only into an approved local export root (class W1 with `fs_effect: write_root`), and shall be class X1 if the destination leaves the machine.

## Interfaces

- **Offers:** `cmm.convert_dmis@1` (W2 draft), `cmm.convert_program_to_dmis@1` (W1 write_root); DmisConversionReport, DmisExportReport types.
- **Consumes:** `IO.DMIS` program model, parser and writer; `AUTO.PROG` DMIS mapping table and Command Program format; `CMM.GEN.SEQ`; `CMM.GEN.PROV`.
- **Journal:** `cmm.convert_dmis@1`, `cmm.convert_program_to_dmis@1`.

## Decomposition hints

IMPORT (DMIS to Command Program, phase P2), REPORT (conversion report model), EXPORT (Command Program to DMIS; phase follows the `IO.DMIS` writer, currently `later`).

## Open questions

- Coverage bookkeeping: seed PCD.PRG.10 (DMIS program import and export) is covered in frontmatter by `AUTO.PROG`, and `CMM` does not cover `PCD.PRG.*`, so this child cannot list it in `covers:` without a warning. The behaviour is owned here by assignment; the coverage tag stays with `AUTO.PROG`.
- `IO.DMIS` schedules `export.dmis_program@1` as `later`, so the EXPORT grandchild will need phase `later` (ratification proposed in D-018).
- Which DMIS statements map to which steps is a domain-expert task; the report makes gaps visible rather than guessing.

## Children

_None yet: status `open`._
