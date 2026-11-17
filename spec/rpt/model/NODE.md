---
id: RPT.MODEL
title: Report model, templates and content blocks
level: 2
parent: RPT
kind: subsystem
status: split
phase: P1a
size: XL
owners: []
claimed_by:
satisfies: [REQ-RPT-001, REQ-RPT-002, REQ-RPT-007]
covers: [GOM.RPT.01, GOM.RPT.02, GOM.RPT.03, GOM.RPT.04, GOM.RPT.05, GOM.RPT.07, PCD.RPT.01, PCD.RPT.03, PW.RPT.02, PW.RPT.03, SA.RPT.01, SA.RPT.02, SA.RPT.04]
depends_on: [CORE, GDT, UNC, VIZ]
cli: [fid report *-report, fid report *-page, fid report *-template, fid report *-block]
mcp: [report_*_report, report_*_page, report_*_template, report_*_block]
journal: [report.*_report, report.*_page, report.*_template, report.*_block]
updated: 2026-11-16
---

# RPT.MODEL: Report model, templates and content blocks

## Purpose

The pure-data report document and the engine that fills it. A report is an ordered set of pages made of content blocks (text, characteristic tables, charts, restorable 3D view snapshots, images, form fields). Blocks bind to ResultRecords, saved views and statistics series by stable ID, never by copied values. Templates are reports with unbound slots. Generation resolves bindings deterministically from the journal, formats every number through the kernel's versioned formatters and labels transcription records by their source. Every other RPT child builds on this model, so it is its own unit.

## Scope

**In:**
- Report, page and block data model; stable block IDs (UUIDv7, CONSTITUTION §5)
- Template format: slots, binding rules (by characteristic, element, tag, stage or query), repetition (one row or page per bound item), versioning
- Block types: text with bound fields, characteristic tables with column and row customisation, chart blocks (trend, histogram, control chart, vector group plot) that lay out series supplied by other nodes, 3D view snapshots that store the VIZ saved-view ID so the scene can be restored, images, page header and footer
- Multi-stage reports: one report spanning several stages or runs of a project
- Generation: resolve bindings against journaled ResultRecords, produce a resolved report document with a content digest
- Number formatting through the kernel's versioned formatters, with the conformance-safe rounding rule
- Source labelling of transcription records (`RESULTS_ONLY` basis) and of `DIRECT_READING` records
- Built-in starter templates (characteristic table, deviation summary, multi-stage table)

**Out** (and which node owns it instead):
- Interactive editing UI, page layout and styles (`RPT.DESIGN`)
- AS9102 and PPAP form templates (`RPT.FORMS`)
- Ballooned drawing blocks and manual reading entry (`RPT.BALLOON`)
- Computing statistics and chart series (`RPT.STATS`)
- PDF, spreadsheet and CSV rendering (`RPT.OUTPUT`)
- Rendering 3D views and saved-view storage (`VIZ`)
- Vector group computation (`LVM`)

## Requirements

- **REQ-RPT.MODEL-001** The report model shall represent a report as pages of typed content blocks, each with a stable ID that survives save, reload and regeneration.
- **REQ-RPT.MODEL-002** A content block shall reference ResultRecords, saved views and statistics series by stable ID, and shall not store copies of their values in the template.
- **REQ-RPT.MODEL-003** When a report is generated twice from the same journal head and the same template version, the system shall produce resolved report documents with identical content digests (RFC 8785 canonical JSON + SHA-256).
- **REQ-RPT.MODEL-004** When a binding resolves to a ResultRecord that is not in the journal, the system shall fail the generation with error `report.unjournaled_record` and journal nothing.
- **REQ-RPT.MODEL-005** When a bound ResultRecord has evaluation basis `RESULTS_ONLY`, the system shall label its row or field with the name and version of the source software recorded in the transcription record.
- **REQ-RPT.MODEL-006** When a bound ResultRecord has evaluation basis `DIRECT_READING`, the system shall label its row or field with the cited gauge ID.
- **REQ-RPT.MODEL-007** Every numeric value in a resolved report shall be produced by a kernel formatter whose ID and version are recorded in the resolved document.
- **REQ-RPT.MODEL-008** If rounding would make a value whose decision is `NONCONFORMS` display as equal to the limit it violates, then the system shall show enough additional digits to distinguish it from the limit, up to the formatter's maximum precision.
- **REQ-RPT.MODEL-009** A report shall show each result's decision outcome and uncertainty status, and shall show UEC-0 as "not evaluated" rather than a number.
- **REQ-RPT.MODEL-010** A 3D view block shall store the VIZ saved-view ID so that, when the user opens it, the scene is restored to that view.
- **REQ-RPT.MODEL-011** A template slot with a repetition rule shall emit one row or page per bound item, in a deterministic order defined by the rule (default: characteristic ID order).
- **REQ-RPT.MODEL-012** A table block shall let the user choose, order and rename columns from the ResultRecord fields and characteristic attributes.
- **REQ-RPT.MODEL-013** A report shall be able to bind results from several stages or runs of one project and show them side by side.
- **REQ-RPT.MODEL-014** When a template is saved, the system shall assign it a version, and a generated report shall record the template ID and version it used.
- **REQ-RPT.MODEL-015** Template text and user-entered strings shall be typed as untrusted content (CONSTITUTION C6).

## Interfaces

- **Offers:** commands `report.create_report@1`, `report.generate_report@1`, `report.save_template@1`, `report.apply_template@1`, `report.add_block@1`, `report.bind_block@1` (patterns above); types `Report`, `Page`, `Block`, `Template`, `ResolvedReport`; the template file format (QIF-adjacent JSON, final form decided at leaf level); the chart-series input contract consumed from `RPT.STATS`.
- **Consumes:** ResultRecords (GDT, UNC), saved views and snapshot capture (VIZ), kernel number formatters (KRN via CORE), journal and project model (CORE).
- **Side effects:** creating and editing reports and templates is W2; generation into scratch is R1.

## Decomposition hints

DOC (report/page/block types and storage), TMPL (template format, slots and repetition), BIND (binding resolution and digest), FMT (formatter use, conformance-safe rounding, labelling), BLOCKS (table, chart, view, text block types), STARTER (built-in templates).

## Split rationale

The split follows the **generation pipeline**: the document (DOC) holds pages and blocks; templates (TMPL) say what to bind; binding (BIND) resolves against a journal head and digests; formatting (FMT) is the only place values become text; and block kinds (BLOCKS) describe what each block shows. FMT is separate from BIND so that every RPT node (forms, dashboards, output) shares one conformance-safe formatting layer over `KRN.NUM`.

- The starter templates were folded into TMPL rather than a separate STARTER child: they are data in the template format and have no logic of their own.
- Page commands (`*-page`) were added to this node's CLI, MCP and journal patterns for DOC.

Alternatives considered:
- **By block type** (one child per block with its own binding). Rejected: binding, digests and formatting would be duplicated per block.
- **Template and binding together.** Rejected: the template format is an exchanged, versioned contract, while binding is an engine; they change at different rates.

## Open questions

- Should templates be stored as QIF extensions or as a separate JSON format? QIF's sanctioned hooks (CONSTITUTION §5) may not fit page layout. To decide at the TMPL leaf.
- The kernel's versioned number formatters (`fmt.*`) and the exact decimal comparator (`kdec`) are owned by `KRN.NUM` (level-2 review); `RPT.MODEL.FMT` depends on it.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| RPT.MODEL.DOC | Report document model and storage (leaf) | 001 015 | GOM.RPT.01 |
| RPT.MODEL.TMPL | Template format, slots, repetition and starter templates | 011 013 014 015 | GOM.RPT.02, PCD.RPT.01, PW.RPT.02, SA.RPT.01 |
| RPT.MODEL.BIND | Binding resolution and report generation | 002 003 004 013 | GOM.RPT.07 |
| RPT.MODEL.FMT | Value formatting, decision display and source labels | 005 006 007 008 009 | none |
| RPT.MODEL.BLOCKS | Content block types | 001 010 012 | GOM.RPT.03-05, PCD.RPT.03, PW.RPT.03, SA.RPT.02/04 |
