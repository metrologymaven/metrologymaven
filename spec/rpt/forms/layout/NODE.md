---
id: RPT.FORMS.LAYOUT
title: Fixed form templates for FAI and PPAP
level: 3
parent: RPT.FORMS
kind: module
status: split
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-RPT.FORMS-001, REQ-RPT.FORMS-002, REQ-RPT.FORMS-008]
covers: []
depends_on: [RPT.MODEL.TMPL, RPT.MODEL.BIND]
uses_artifacts: [RPT.FORMS.HDR, RPT.FORMS.ROWS, RPT.FORMS.PPAP]
cli: [fid report show-layout-fai, fid report show-layout-ppap]
mcp: [report_show_layout_fai, report_show_layout_ppap]
journal: [report.show_layout_fai@1, report.show_layout_ppap@1]
updated: 2026-11-17
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

- **Applied (2026-11-17): D-015 accepted, option C.** The project ships only original layouts (own labels, own arrangement, citations by designator, revision or edition, and clause only) plus machine-readable field maps; licensed users who need the published form's look import or build their own template in `RPT.DESIGN` and bind it to the same field IDs. The D-015 condition (counsel review before publication or release) is enforced by a `clearance` state on each built-in template and a release-profile packaging test (`RPT.FORMS.LAYOUT.MAP`). Whether field order may follow the standards' clause order awaits that counsel check [unverified]; field order is template data, so the answer changes data only.
- **Applied (D-034):** the form data nodes are read as published schemas (`uses_artifacts`), not code dependencies; form data providers implement the `FieldSource` trait defined in `RPT.MODEL.BIND.RESOLVE`.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| RPT.FORMS.LAYOUT.MAP | Form field maps, form_field block kind, locking and clearance (leaf) | 001 002 003 005 | none |
| RPT.FORMS.LAYOUT.FAI | Original locked templates for FAI forms 1 to 3 (leaf) | 001 002 003 004 | none |
| RPT.FORMS.LAYOUT.PPAP | Original locked template for the PPAP dimensional results report (leaf) | 001 002 003 004 | none |
