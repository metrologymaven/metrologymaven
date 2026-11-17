---
id: RPT.FORMS.LAYOUT
title: Fixed form templates for FAI and PPAP
level: 3
parent: RPT.FORMS
kind: module
status: decision
decision: D-015
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-RPT.FORMS-001, REQ-RPT.FORMS-002, REQ-RPT.FORMS-008]
covers: []
depends_on: [RPT.MODEL.TMPL, RPT.FORMS.HDR, RPT.FORMS.ROWS, RPT.FORMS.PPAP]
cli: [fid report show-layout-fai, fid report show-layout-ppap]
mcp: [report_show_layout_fai, report_show_layout_ppap]
journal: []
updated: 2026-11-16
---

# RPT.FORMS.LAYOUT: Fixed form templates for FAI and PPAP

## Purpose

The fixed, non-editable visual templates that turn FAI forms 1–3 and the PPAP dimensional data into pages: field labels, arrangement, column order and pagination of long form 3 tables. They are fixed so that every customer receives the same structure. How closely they may follow the published forms is blocked on D-015.

## Scope

**In:**
- Fixed templates for FAI forms 1, 2 and 3 and the PPAP dimensional report, built in the `RPT.MODEL.TMPL` format and locked against editing
- Field labels in the project's own words
- Pagination and continuation headers for long tables
- A machine-readable field map (field ID, label, data source) published with each template

**Out** (and which node owns it instead):
- Form data (`RPT.FORMS.HDR`, `RPT.FORMS.ROWS`, `RPT.FORMS.PPAP`)
- Rendering to PDF or spreadsheet (`RPT.OUTPUT`)
- User-made look-alike templates (`RPT.DESIGN`, subject to D-015)

## Requirements

- **REQ-RPT.FORMS.LAYOUT-001** The system shall ship fixed templates for FAI forms 1, 2 and 3 and the PPAP dimensional results report, and users shall not be able to edit them.
- **REQ-RPT.FORMS.LAYOUT-002** The form templates shall contain no text, layout artwork, logos or trade dress copied from the published standards (CONSTITUTION §8), within the limits set by D-015.
- **REQ-RPT.FORMS.LAYOUT-003** Each template shall cite its standard by designator, revision or edition, and clause only.
- **REQ-RPT.FORMS.LAYOUT-004** When a form 3 or PPAP table spans several pages, every continuation page shall repeat the column headers and the form's identifying header fields.
- **REQ-RPT.FORMS.LAYOUT-005** Each template shall publish a machine-readable field map from each field ID to its label and data source.

## Interfaces

- **Offers:** locked templates `tmpl:fai-form1`, `tmpl:fai-form2`, `tmpl:fai-form3`, `tmpl:ppap-dim` (IDs illustrative); `report.show_layout_fai@1`, `report.show_layout_ppap@1` (R0) to show field maps.
- **Consumes:** template format (`RPT.MODEL.TMPL`), form data from its siblings.

## Decomposition hints

FAI (form 1–3 templates), PPAP (dimensional template), MAP (field maps), PAGE (pagination).

## Open questions

- **Blocked on D-015:** may the layout follow the published forms' field order and grid, or must it be an original arrangement? Choosing either now would be guessing on a legal question, so this node stays in `decision` until D-015 is decided.

## Children

_None yet: status `decision`._
