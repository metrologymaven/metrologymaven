---
id: RPT.OUTPUT
title: Report and statistics output
level: 2
parent: RPT
kind: subsystem
status: split
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-RPT-001, REQ-RPT-007]
covers: [GOM.RPT.06, PCD.RPT.04, PCD.RPT.05, PCD.RPT.06, PW.RPT.04, SA.RPT.03]
depends_on: [RPT.MODEL, IO, VIZ]
cli: [fid report export-*, fid stats export-*]
mcp: [report_export_*, stats_export_*]
journal: [report.export_*, stats.export_*]
updated: 2026-11-16
---

# RPT.OUTPUT: Report and statistics output

## Purpose

Turns a resolved report into deliverable files (PDF, spreadsheet, CSV) and sends statistics data to SPC systems (Q-DAS AQDEF, QIF Statistics, CSV). It owns page rendering to PDF and the mapping from report and statistics content to export records; the byte-level file writers for exchange formats stay in IO. Output is separate from the model because it is where reproducibility of bytes, fonts and embedded images is decided.

## Scope

**In:**
- PDF rendering of resolved reports: text, tables, vector charts, embedded view snapshots, fonts embedded
- Spreadsheet export (one sheet per table block, plus a metadata sheet) and CSV export of table blocks
- Statistics and results export to SPC systems via IO writers (Q-DAS AQDEF, QIF Statistics, CSV)
- Export metadata: report digest, template ID and version, kernel and formatter versions
- Export to approved export roots only, by handle

**Out** (and which node owns it instead):
- Byte-level writers for CSV, Q-DAS and QIF (`IO`)
- Report content and generation (`RPT.MODEL`)
- Snapshot rendering (`VIZ`)
- Signing exported files (`TRUST`)

## Requirements

- **REQ-RPT.OUTPUT-001** The system shall render a resolved report to PDF with all fonts embedded.
- **REQ-RPT.OUTPUT-002** When the same resolved report is rendered twice with the same renderer version, the system shall produce byte-identical PDF files.
- **REQ-RPT.OUTPUT-003** Every exported file shall carry the resolved report's content digest, template ID and version, and kernel version in its metadata.
- **REQ-RPT.OUTPUT-004** The system shall export each table block of a report to a spreadsheet sheet and to CSV, with values as numbers in SI or the authored unit and the unit stated in the column header.
- **REQ-RPT.OUTPUT-005** Exported numeric text shall be identical to the text shown in the PDF for the same value, because both come from the same formatter output.
- **REQ-RPT.OUTPUT-006** The system shall export per-characteristic results and statistics to Q-DAS AQDEF, QIF Statistics and CSV through the IO writers.
- **REQ-RPT.OUTPUT-007** Exports shall write only to approved export roots by handle, and agents shall not widen those roots (CONSTITUTION §3).
- **REQ-RPT.OUTPUT-008** If a report contains a block that cannot be rendered in the chosen format, then the export shall fail with error `report.unsupported_block` naming the block ID, and write no file.
- **REQ-RPT.OUTPUT-009** A PDF render of a 50-page report with 20 images shall complete in under 10 s on reference hardware [unverified].

## Interfaces

- **Offers:** `report.export_pdf@1`, `report.export_spreadsheet@1`, `report.export_csv@1`, `stats.export_qdas@1`, `stats.export_qif@1`, `stats.export_csv@1`.
- **Consumes:** `ResolvedReport` (`RPT.MODEL`), statistics (`RPT.STATS` through the resolved report or a data-set handle), IO writers, VIZ snapshot images.
- **Side effects:** writing to a local export root is W1-like local output; sending to a network SPC database is X1 (request only on MCP).

## Decomposition hints

PDF (page renderer and font embedding), SHEET (spreadsheet and CSV), SPC (Q-DAS, QIF Statistics, CSV export mapping), META (export metadata and digests).

## Split rationale

The split follows the **output format family**, with one shared envelope: PDF (paged, byte-reproducible rendering), SHEET (tabular spreadsheet and CSV of report tables), and SPC (statistics data to SPC systems through IO writers). All three write through META, which owns the export metadata record and the only code that writes into export roots, so the side-effect rule from D-007 / CONSTITUTION §3.1 (local export is W1 `write_root`; leaving the machine is X1) is enforced once.

- META is finished as a leaf (kind task): it has no command of its own, concrete rules and a crash-safety test.
- SPC export is P1b because data sets come from `RPT.STATS` (P1b).

Alternatives considered:
- **One child per file format** (PDF, XLSX, CSV, AQDEF, QIF). Rejected: CSV appears in both report and statistics exports with different inputs, and AQDEF/QIF bytes belong to IO.
- **Metadata inside each exporter.** Rejected: export-root handling and the W1/X1 boundary would be duplicated.

## Open questions

- Resolved by D-007 (CONSTITUTION §3.1): writing into a local export root is W1 with `fs_effect: write_root`; X1 only when data leaves the machine. Enforced in `RPT.OUTPUT.META`.
- PDF/A conformance for archiving (ISO 19005) is likely wanted by regulated users [unverified]; not required yet.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| RPT.OUTPUT.META | Export envelope: metadata and export-root writes (leaf) | 003 007 | none |
| RPT.OUTPUT.PDF | PDF renderer | 001 002 005 008 009 | GOM.RPT.06, PW.RPT.04, SA.RPT.03 |
| RPT.OUTPUT.SHEET | Spreadsheet and CSV export of report tables | 004 005 008 | PCD.RPT.04, PW.RPT.04, SA.RPT.03 |
| RPT.OUTPUT.SPC | Statistics export to SPC systems | 006 | PCD.RPT.05, PCD.RPT.06 |
