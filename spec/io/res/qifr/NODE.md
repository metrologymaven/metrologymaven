---
id: IO.RES.QIFR
title: QIF Results and QIF Statistics in and out
level: 3
parent: IO.RES
kind: module
status: open
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-IO.RES-008, REQ-IO.RES-009, REQ-IO.RES-013]
covers: [PCD.EXP.03]
depends_on: [IO.RES.TRANS, IO.QIF.XML, IO.QIF.IDS]
cli: [fid import results-qif, fid import results-qif-stats, fid export results-qif, fid export results-qif-stats]
mcp: [import_results_qif, import_results_qif_stats, export_results_qif, export_results_qif_stats]
journal: [import.results_qif@1, import.results_qif_stats@1, export.results_qif@1, export.results_qif_stats@1]
updated: 2026-11-16
---

# IO.RES.QIFR: QIF Results and QIF Statistics

## Purpose

Maps between ResultRecords and the QIF 3.0 Results and Statistics applications: importing third-party QIF Results and Statistics as transcription records, and exporting ResultRecords as XSD-valid QIF Results and multi-part results as QIF Statistics, carrying value, unit, uncertainty class, decision rule and outcome through native fields or `fx` extensions.

## Scope

**In:**
- QIF Results → transcribed values (through `IO.RES.TRANS`); QIF Statistics → historical transcribed series
- ResultRecord → QIF `MeasurementResults` (native fields first, then `fx:ResultProvenance`, `fx:Uncertainty`, `fx:Decision`)
- Multi-part results → QIF Statistics
- Commands listed in frontmatter (imports W2; exports W1 `fs_effect: write_root`)

**Out:**
- XML serialisation and validation (`IO.QIF.XML`), the `fx` catalogue (CORE)

## Requirements

- **REQ-IO.RES.QIFR-001** When ResultRecords are exported to QIF Results, the document shall validate against the pinned QIF 3.0 XSDs and carry each value, unit, uncertainty class, decision rule and outcome.
- **REQ-IO.RES.QIFR-002** When multi-part results are exported to QIF Statistics, the document shall validate against the pinned QIF 3.0 XSDs.
- **REQ-IO.RES.QIFR-003** Each exported ResultRecord's `evaluation_basis`, uncertainty class and decision rule shall be written in a native field where QIF has one and otherwise in the sanctioned `fx` extension.
- **REQ-IO.RES.QIFR-004** When a QIF Results document from another system is imported, each measured characteristic shall become a transcribed value with the source software taken from the document's software definitions.
- **REQ-IO.RES.QIFR-005** When a document exported by this system is re-imported, every ResultRecord field carried in REQ-IO.RES.QIFR-001 shall round-trip unchanged.

## Interfaces

- **Offers:** the four commands in frontmatter.
- **Consumes:** `IO.RES.TRANS`, `IO.QIF.XML`, `IO.QIF.IDS`.

## Decomposition hints

RIN (Results import), ROUT (Results export), SIN (Statistics import), SOUT (Statistics export).

## Open questions

- Whether the platform's own QIF Results re-import is transcription (another system's claim) or restoration of its own records needs a TRUST view (signature check on the file).

## Children

_None yet: status `open`._
