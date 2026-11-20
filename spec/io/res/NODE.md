---
id: IO.RES
title: Results and SPC formats, transcription records
level: 2
parent: IO
kind: subsystem
status: split
phase: P1a
size: XL
owners: []
claimed_by:
satisfies: [REQ-IO-001, REQ-IO-006, REQ-IO-007]
covers: [GOM.EXP.02, GOM.EXP.03, GOM.EXP.04, GOM.EXP.05, GOM.EXP.06, PW.IMP.05, PW.EXP.02, PW.EXP.04, PCD.EXP.03]
depends_on: [IO.REG, IO.QIF, IO.DMIS]
cli: [fid import results*, fid export results*]
mcp: [import_results*, export_results*]
journal: [import.results*, export.results*]
updated: 2026-11-19
---

# IO.RES: Results and SPC formats, transcription records

## Purpose

Moves characteristic results in and out. Inbound, it turns third-party results (QIF Results, DMIS output, CSV, AQDEF, QIF Statistics) into transcription records that are clearly marked as reported by their source and never as re-evaluated. Outbound, it writes ResultRecords as CSV and tables, QIF Results, QIF Statistics, AQDEF and PiWeb. This is what lets the platform sit downstream of incumbents from P1a.

## Scope

**In:**
- Transcription-record import from QIF Results, DMIS output (through `IO.DMIS`), CSV with a column map, AQDEF DFQ and QIF Statistics
- Mapping of each imported value to a ResultRecord with `evaluation_basis`, UEC-0 and the as-reported decision rule
- Export of ResultRecords to CSV and generic tables (column maps), QIF Results, QIF Statistics, AQDEF DFQ (uncertified) and PiWeb
- The export command for report files, delegating content and layout to RPT
- Readers used for historical import (the results catalog itself is RPT)

**Out** (and which node owns it instead):
- QIF document model and XML/JSON serialisation (`IO.QIF`)
- DMIS parsing (`IO.DMIS`)
- Point coordinate export (`IO.CLOUD`)
- Report layout, PDF rendering, AS9102 and PPAP forms, results catalog and statistics (RPT)
- Re-evaluating transcribed results from points (GDT, KRN)
- ResultRecord semantics, decision rules and uncertainty classes (GDT, UNC)

## Requirements

- **REQ-IO.RES-001** When third-party results are imported, the system shall create one transcription record per source file that stores the source software name, source software version (or `unknown`), file SHA-256 digest and import time.
- **REQ-IO.RES-002** Each imported value shall become a ResultRecord with record status `transcribed`, uncertainty class UEC-0, decision rule `dr:as-reported-by-source@1` and null kernel and algorithm fields.
- **REQ-IO.RES-003** Each transcribed ResultRecord shall carry `evaluation_basis` `RESULTS_ONLY`, unless the source file also carries the measured points or features, in which case it shall carry `POINTS` or `FEATURES` respectively.
- **REQ-IO.RES-004** The source's own pass/fail status shall be mapped to the constitution's outcome enum, and a value with no source status shall get outcome `NOT_DECIDABLE`.
- **REQ-IO.RES-005** Every export and every MCP or CLI view of a transcribed record shall show its record status `transcribed` and its source software, so it is never presented as re-evaluated.
- **REQ-IO.RES-006** When a CSV result file is imported, the system shall require a versioned column map, and the map's ID and digest shall be journaled.
- **REQ-IO.RES-007** The system shall export ResultRecords to CSV and generic tables through versioned column maps, keeping each value's authored decimal form and unit.
- **REQ-IO.RES-008** The system shall export ResultRecords to QIF Results documents that validate against the pinned QIF 3.0 XSDs, carrying value, unit, uncertainty class, decision rule and outcome.
- **REQ-IO.RES-009** The system shall export multi-part results to QIF Statistics documents that validate against the pinned QIF 3.0 XSDs.
- **REQ-IO.RES-010** The system shall export results as AQDEF DFQ built from publicly documented K-fields, and every such export and its documentation shall be labelled as not Q-DAS certified.
- **REQ-IO.RES-011** The system shall export results in a PiWeb-importable form [unverified: whether a public format specification exists; may become plugin-only].
- **REQ-IO.RES-012** When a report file export is requested, the system shall write the report produced by RPT to an approved export root and shall not itself lay out report content.
- **REQ-IO.RES-013** Exported results shall carry each ResultRecord's `evaluation_basis`, uncertainty class, decision rule and `interpretation` label (D-045) wherever the target format has a field or extension for them.
- **REQ-IO.RES-014** If a results export would write a ResultRecord whose `interpretation` is `provisional` to a format or column map that cannot carry the label, then the export shall fail with `interpretation_label_unsupported` naming the record and write nothing (D-045).

## Interfaces

- **Offers:** `import.results_qif@1`, `import.results_dmis@1`, `import.results_csv@1`, `import.results_aqdef@1`, `import.results_qif_stats@1`; `export.results_csv@1`, `export.results_qif@1`, `export.results_qif_stats@1`, `export.results_aqdef@1`, `export.results_piweb@1`, `export.results_report@1`; the transcription-record type.
- **Consumes:** `IO.REG` harness and provenance; `IO.QIF` Results and Statistics types; `IO.DMIS` output-file reader; ResultRecord type (GDT/CORE); RPT rendered reports.

## Decomposition hints

TRANS (transcription record model and source mapping), QIFR (QIF Results and Statistics in and out), CSV (column maps in and out), AQDEF, PIWEB, REPORT (report-file export envelope).

## Open questions

- **D-045 (accepted 2026-11-19):** REQ-IO.RES-013 now includes the `interpretation` label and REQ-IO.RES-014 refuses a provisional record a format cannot label; both are enforced by the shared guard `IO.RES.ILABEL`, which every exporter (`IO.RES.QIFR.ROUT`, `IO.RES.CSV.OUT`, `IO.RES.AQDEF.OUT`, `IO.RES.PIWEB`, `IO.RES.REPORT`) calls before writing. Wiring the call into the already-built `IO.RES.CSV.OUT` is a follow-up build task.
- GOM.EXP.03 (export reports as PDF) is in IO's covers, but RPT owns PDF output. This node covers it only as the export envelope (REQ-IO.RES-012). If maintainers prefer, the seed could be moved to RPT, which would need a change to IO's `covers:` in the parent.
- AQDEF is owned by a competitor; certification terms and fees are [unverified]. VDA QDX as a neutral alternative is not in IO's scope yet.
- Where the ResultRecord type itself lives (CORE data model or GDT) is not settled in the parents; this node only consumes it.

## Split rationale

One child owns the **meaning** of a transcribed result (TRANS: record model, status, outcome and display rules, plus the DMIS command whose parser lives elsewhere); the others own one **format family** each, in and out, and all call TRANS so the "never presented as re-evaluated" rule is enforced once. REPORT is only an export envelope around RPT output.

Alternatives considered:
- **Inbound vs outbound children.** Rejected: column maps, K-field tables and QIF mappings are shared by the two directions of each format.
- **Transcription rules inside each format child.** Rejected: REQ-IO.RES-002..005 must be identical for every source.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| IO.RES.TRANS | Transcription record model, source mapping and DMIS results import | 001 002 003 004 005 | PW.IMP.05 |
| IO.RES.QIFR | QIF Results and QIF Statistics in and out | 008 009 013 | PCD.EXP.03 |
| IO.RES.CSV | CSV and table result exchange through versioned column maps | 006 007 013 | GOM.EXP.02, GOM.EXP.06, PW.EXP.02 |
| IO.RES.AQDEF | AQDEF DFQ export and historical import (uncertified) | 010 013 | GOM.EXP.04, PW.EXP.04 |
| IO.RES.PIWEB | PiWeb-importable results export | 011 | GOM.EXP.05 |
| IO.RES.REPORT | Report-file export envelope (leaf) | 012 | GOM.EXP.03 |
| IO.RES.ILABEL | Interpretation label guard for every results export (leaf) | 013 014 | none |
