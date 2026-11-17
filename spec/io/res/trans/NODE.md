---
id: IO.RES.TRANS
title: Transcription record model, source mapping and DMIS results import
level: 3
parent: IO.RES
kind: module
status: open
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-IO.RES-001, REQ-IO.RES-002, REQ-IO.RES-003, REQ-IO.RES-004, REQ-IO.RES-005]
covers: [PW.IMP.05]
depends_on: [IO.REG.ENV, IO.DMIS.OUT]
cli: [fid import results-dmis]
mcp: [import_results_dmis]
journal: [import.results_dmis@1]
updated: 2026-11-16
---

# IO.RES.TRANS: Transcription record model and source mapping

## Purpose

The one place that defines how a third-party result becomes a platform record: the transcription record (source software, version, file digest, import time), the per-value mapping to a ResultRecord with status `transcribed`, UEC-0, `dr:as-reported-by-source@1` and null kernel fields, the evaluation-basis rule, and the outcome mapping. Every results importer (QIF, CSV, AQDEF, DMIS) calls this mapping, so "never presented as re-evaluated" holds uniformly. It also hosts the DMIS results import command because DMIS parsing lives in `IO.DMIS`.

## Scope

**In:**
- `TranscriptionRecord` and `TranscribedValue` types and their mapping to ResultRecord fields
- Evaluation-basis rule (`RESULTS_ONLY`, or `POINTS`/`FEATURES` when the source carries them)
- Source status to outcome mapping (`CONFORMS`, `NONCONFORMS`, `INDETERMINATE`, `NOT_DECIDABLE`)
- Display rule: every view and export marks `transcribed` and the source software
- `import.results_dmis@1` (W2) over `IO.DMIS.OUT`

**Out:**
- The ResultRecord type itself (GDT/CORE; consumed here)
- Format parsing for QIF, CSV and AQDEF (`IO.RES.QIFR`, `IO.RES.CSV`, `IO.RES.AQDEF`)

## Requirements

- **REQ-IO.RES.TRANS-001** When third-party results are imported, the system shall create one transcription record per source file holding source software name, source software version or `unknown`, file SHA-256 and import time.
- **REQ-IO.RES.TRANS-002** Each imported value shall become a ResultRecord with record status `transcribed`, uncertainty class UEC-0, decision rule `dr:as-reported-by-source@1`, and null kernel version and algorithm fields.
- **REQ-IO.RES.TRANS-003** Each transcribed ResultRecord shall carry `evaluation_basis` `RESULTS_ONLY`, unless the source also carries the measured points or fitted features for that value, in which case it shall carry `POINTS` or `FEATURES`.
- **REQ-IO.RES.TRANS-004** The source's pass or fail status shall be mapped to `CONFORMS` or `NONCONFORMS`, any explicit source "undecided" status to `INDETERMINATE`, and a value with no source status to `NOT_DECIDABLE`.
- **REQ-IO.RES.TRANS-005** Every CLI and MCP view and every export of a transcribed record shall show status `transcribed` and the source software name and version.
- **REQ-IO.RES.TRANS-006** When a DMIS output file is imported with `import.results_dmis@1`, the source software shall be taken from the file's header statements when present and otherwise required from the caller.

## Interfaces

- **Offers:** `TranscriptionRecord` type, `transcribe(rows, source) -> Vec<ResultRecord>` used by every results importer, `import.results_dmis@1` (W2).
- **Consumes:** ResultRecord type (GDT/CORE), `IO.DMIS.OUT`, `IO.REG` envelope.

## Decomposition hints

MODEL (record types), MAP (value and outcome mapping), VIEW (display rule helpers for CLI/MCP/export), DMIS (command).

## Open questions

- Where ResultRecord lives (CORE data model or GDT) is unsettled in the parents (carried from IO.RES); this node cannot become a leaf until it is fixed.

## Children

_None yet: status `open`._
