---
id: IO.DMIS.OUT
title: DMIS output-file reader for measured results
level: 3
parent: IO.DMIS
kind: module
status: open
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-IO.DMIS-003, REQ-IO.DMIS-006]
covers: []
depends_on: [IO.DMIS.PARSE]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# IO.DMIS.OUT: DMIS output-file reader

## Purpose

Turns a parsed DMIS output file (`.dmo`) into a typed list of reported features and tolerance results, exactly as written, for `IO.RES` to turn into transcription records. This is the P1a path that lets the platform report on incumbent CMM results.

## Scope

**In:**
- Output statements for actual features and tolerance results (label, nominal, actual, deviation, limits, out-of-tolerance flag)
- Units and angle units declared in the file
- Authored decimal forms kept lexically
- QPIds present in DMIS 5.3 output carried onto the result rows

**Out:**
- Transcription records and the `import.results_dmis@1` command (`IO.RES.TRANS`)
- Program semantics (`IO.DMIS.PROG`)

## Requirements

- **REQ-IO.DMIS.OUT-001** When a DMIS output file is read, each reported feature and tolerance result shall be extracted with its label, nominal, actual, deviation, unit and out-of-tolerance flag as written.
- **REQ-IO.DMIS.OUT-002** Numeric values shall keep their authored decimal form and the unit in force at that statement.
- **REQ-IO.DMIS.OUT-003** QPIds present on output statements shall be carried onto the extracted rows unchanged.
- **REQ-IO.DMIS.OUT-004** Output statements the reader does not map shall be listed with line number and statement type in the read report.

## Interfaces

- **Offers:** `read_dmis_output(tree) -> DmisOutput { rows, report }` (library; the registry command lives in `IO.RES.TRANS`).
- **Consumes:** `IO.DMIS.PARSE`.

## Decomposition hints

FEAT (feature actuals), TOL (tolerance results), UNIT (unit state), REPORT. Likely a single leaf once PARSE fixes the tree types.

## Open questions

- The IO.DMIS parent lists `import.dmis_output@1` while IO.RES lists `import.results_dmis@1` for the same user action. This child offers only the library; the single command is `import.results_dmis@1` in `IO.RES.TRANS`. The parent's Interfaces line should be reconciled when IO.DMIS is reviewed.

## Children

_None yet: status `open`._
