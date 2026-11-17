---
id: AUTO.PROG.DMISMAP
title: DMIS mapping contract
level: 3
parent: AUTO.PROG
kind: module
status: open
phase: P2
size: L
owners: []
claimed_by:
satisfies: [REQ-AUTO.PROG-015]
covers: [PCD.PRG.10]
depends_on: [AUTO.PROG.FMT, AUTO.PROG.FLOW]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# AUTO.PROG.DMISMAP: DMIS mapping contract

## Purpose

A machine-readable table stating, for each DMIS 5.3 statement class, the Command Program construct it maps to (a control-flow construct, an operator step, a variable operation, or "a CMM command step owned by CMM") or the reason it is unsupported. IO.DMIS parses against it, CMM.GEN converts with it (including export back to DMIS, owner assigned at level-2 review to CMM.GEN), and the conversion report cites it.

## Scope

**In:**
- Mapping table format (one row per DMIS statement class: class, mapping kind, target construct or command family, status, notes)
- Rows for the program-structure statement classes (control flow, variables, declarations, operator text, subroutines and macros)
- Placeholder rows for measurement and motion classes pointing to CMM-owned command families
- Generated support summary

**Out:**
- DMIS parsing and writing (`IO.DMIS`)
- Conversion logic and DMIS export of programs (`CMM.GEN`)
- Motion and probing semantics (`CMM`)

## Requirements

- **REQ-AUTO.PROG.DMISMAP-001** The table shall have exactly one row per DMIS 5.3 statement class that `IO.DMIS` recognises, and a CI check shall fail when a recognised class has no row.
- **REQ-AUTO.PROG.DMISMAP-002** Each row shall name a mapping kind (`program_construct`, `command_family`, `unsupported`) and, unless unsupported, the target construct or command family.
- **REQ-AUTO.PROG.DMISMAP-003** Each `unsupported` row shall give a reason code, and the table shall contain no text copied from the DMIS standard.

## Interfaces

- **Offers:** mapping table (data file) and generated summary for `IO.DMIS` and `CMM.GEN`.
- **Consumes:** statement-class list from `IO.DMIS`; FMT and FLOW constructs.

## Decomposition hints

TABLE (format and CI check), ROWS-STRUCT (program-structure rows, needs DMIS expertise), ROWS-CMM (placeholders linked to CMM).

## Open questions

- Phase: tagged P2 because DMIS program import is P2 (IO.DMIS); this is later than the parent's P1b, which is allowed.
- Row content needs a DMIS domain expert; agents can draft structure but each row's meaning must be reviewed (same spirit as D-009 for the DMT).

## Children

_None yet: status `open`._
