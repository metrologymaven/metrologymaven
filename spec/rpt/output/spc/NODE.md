---
id: RPT.OUTPUT.SPC
title: Statistics export to SPC systems
level: 3
parent: RPT.OUTPUT
kind: module
status: open
phase: P1b
size: L
owners: []
claimed_by:
satisfies: [REQ-RPT.OUTPUT-006]
covers: [PCD.RPT.05, PCD.RPT.06]
depends_on: [RPT.OUTPUT.META, RPT.STATS.DSET, IO.RES]
cli: [fid stats export-qdas, fid stats export-qif, fid stats export-csv]
mcp: [stats_export_qdas, stats_export_qif, stats_export_csv]
journal: [stats.export_qdas@1, stats.export_qif@1, stats.export_csv@1]
updated: 2026-11-16
---

# RPT.OUTPUT.SPC: Statistics export to SPC systems

## Purpose

Sends per-characteristic results of a data set to SPC systems as files: Q-DAS AQDEF (uncertified), QIF Statistics and CSV. RPT decides *which* records go out, in which order and with which statistics context; the byte-level writers stay in `IO.RES`. Writing into a local export root is W1; delivery to a network SPC database is X1 and is a separate, request-only command.

## Scope

**In:**
- Mapping a `DataSet` (records, exclusions, characteristic metadata) to the IO.RES writer inputs for AQDEF, QIF Statistics and CSV
- Local file export through the export sink (W1)
- An X1 command for delivery to a network destination, request-only on MCP (named at leaf level)
- Labelling AQDEF exports as not Q-DAS certified

**Out** (and which node owns it instead):
- AQDEF, QIF and CSV byte writers and column maps (`IO.RES`)
- Data-set selection (`RPT.STATS.DSET`)
- Metadata record and export-root writes (`RPT.OUTPUT.META`)

## Requirements

- **REQ-RPT.OUTPUT.SPC-001** The system shall export the records of a data set per characteristic to Q-DAS AQDEF, QIF Statistics and CSV through the `IO.RES` writers, in data-set order.
- **REQ-RPT.OUTPUT.SPC-002** Records excluded from the data set shall not be exported, and the export metadata shall list the exclusion count per characteristic.
- **REQ-RPT.OUTPUT.SPC-003** Every AQDEF export shall be labelled as not Q-DAS certified in its metadata and in the command result.
- **REQ-RPT.OUTPUT.SPC-004** Local file exports shall be side-effect class W1 with `fs_effect: write_root`; delivery to a network destination shall be a separate X1 command that is request-only on MCP.
- **REQ-RPT.OUTPUT.SPC-005** Transcribed records shall keep their `transcribed` status and source software in every format that has a field for it.

## Interfaces

- **Offers:** `stats.export_qdas@1`, `stats.export_qif@1`, `stats.export_csv@1` (W1, `write_root`); an X1 delivery command named at leaf level.
- **Consumes:** data sets (`RPT.STATS.DSET`), IO.RES writers, export envelope (`RPT.OUTPUT.META`).

## Decomposition hints

MAP (data set to writer input), FILE (local exports), SEND (X1 network delivery).

## Open questions

- Which network SPC destinations to support for X1 delivery (file drop on a share vs. vendor database APIs) is undecided; vendor APIs may be plugin-only.

## Children

_None yet: status `open`._
