---
id: RPT.FORMS.LAYOUT.MAP
title: Form field maps, form_field block kind, locking and clearance
level: 4
parent: RPT.FORMS.LAYOUT
kind: feature
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-RPT.FORMS.LAYOUT-001, REQ-RPT.FORMS.LAYOUT-002, REQ-RPT.FORMS.LAYOUT-003, REQ-RPT.FORMS.LAYOUT-005]
covers: []
depends_on: [RPT.MODEL.DOC, RPT.MODEL.TMPL.FORMAT, RPT.MODEL.BIND.RESOLVE, RPT.MODEL.FMT]
uses_artifacts: [RPT.FORMS.HDR, RPT.FORMS.ROWS, RPT.FORMS.PPAP]
side_effect: [R0, R0]
determinism: D0
cli: [fid report show-layout-fai, fid report show-layout-ppap]
mcp: [report_show_layout_fai, report_show_layout_ppap]
journal: [report.show_layout_fai@1, report.show_layout_ppap@1]
updated: 2026-11-17
---

# RPT.FORMS.LAYOUT.MAP: Form field maps, form_field block kind, locking and clearance

## Purpose

The machinery shared by the fixed FAI and PPAP templates. A **field map** lists every field a form shows (field ID, own-words label key, data source in the form data, citation by designator and clause only). The `form_field` block kind places one mapped field on a page and resolves it from a `FieldSource`. Built-in form templates are registered **locked**, so no user can edit them, and each carries a **clearance** state for the counsel review that D-015 requires before release. Licensed users who want the published form's exact look build their own template in `RPT.DESIGN` and bind it to the same field IDs (D-015 option C).

## Scope

**In:** `FieldMap` schema (versioned, canonical JSON) and its publication with each form template; `form_field` block kind (payload schema, `BlockResolver` implementation) reading values from the `FieldSource` trait of `RPT.MODEL.BIND.RESOLVE`, which form data providers implement; registration of built-in locked templates; the `clearance` record and its release gate; `report.show_layout_fai@1` and `report.show_layout_ppap@1` (R0); the clean-room label check.

**Out:** the FAI form 1-3 template content (`RPT.FORMS.LAYOUT.FAI`); the PPAP template content (`RPT.FORMS.LAYOUT.PPAP`); form data (`RPT.FORMS.HDR`, `RPT.FORMS.ROWS`, `RPT.FORMS.PPAP`, which implement `FieldSource`); refusing edits to locked templates in the save path (`RPT.MODEL.TMPL.VER`, which honours the `locked` flag defined in `RPT.MODEL.TMPL.FORMAT`); user-made templates (`RPT.DESIGN`).

## Requirements

- **REQ-RPT.FORMS.LAYOUT.MAP-001** Each built-in form template shall be registered with `locked = true`, and any attempt to save, edit or delete it shall be refused by the template layer with error `report.template_locked`.
- **REQ-RPT.FORMS.LAYOUT.MAP-002** Each built-in form template shall publish a `FieldMap` listing, for every field it shows, the field ID, the label message key, the data source path in the form data and an optional citation, sorted by field ID with no duplicates.
- **REQ-RPT.FORMS.LAYOUT.MAP-003** A citation in a field map or template shall consist only of a standard designator, a revision or edition and a clause number, and the map validator shall reject any citation longer than 64 bytes or containing text beyond that pattern with error `report.citation_invalid`.
- **REQ-RPT.FORMS.LAYOUT.MAP-004** When a `form_field` block is resolved, the system shall look up its field ID in the template's field map and take the value from the registered `FieldSource`, and if the field ID is not in the map it shall fail generation with error `report.field_unknown` naming the block.
- **REQ-RPT.FORMS.LAYOUT.MAP-005** Every numeric value a `form_field` block shows shall come from `RPT.MODEL.FMT`, and every text value shall stay typed as untrusted (CONSTITUTION C6).
- **REQ-RPT.FORMS.LAYOUT.MAP-006** Each built-in form template shall carry a clearance state `pending` or `cleared` with a reference to a counsel review record, and a build with the `release` profile shall fail its packaging test if any shipped form template is `pending` (D-015 condition).
- **REQ-RPT.FORMS.LAYOUT.MAP-007** When `report.show_layout_fai@1` or `report.show_layout_ppap@1` runs, the system shall return the field maps, template IDs, versions, digests and clearance states of the built-in templates for that form kind.
- **REQ-RPT.FORMS.LAYOUT.MAP-008** A CI check shall fail if any label text in a shipped form template or field map, after normalisation, hashes to an entry of the maintained clean-room phrase hash list.

## Acceptance criteria

- **AC-1** Given the built-in templates from the FAI and PPAP leaves, when `fid report show-layout-fai --json` runs, then it lists three templates (forms 1, 2 and 3) each with a field map sorted by field ID, a digest and a clearance state; `show-layout-ppap` lists one.
- **AC-2** (error path) Given a built-in form template ID, when `fid report save-template --template <it>` runs, then it fails with `report.template_locked` and journals nothing.
- **AC-3** (error path) Given fixture `fixtures/rpt/forms/bad_fieldmaps` (a duplicate field ID, a citation with prose after the clause number, an unknown field in a `form_field` block), then validation fails with the duplicate reported, `report.citation_invalid` and `report.field_unknown` respectively.
- **AC-4** Given fixture `fixtures/rpt/forms/fai_complete` and a stub `FieldSource`, when a template with `form_field` blocks for `part_number` and a numeric field is resolved, then the text field is marked untrusted and the numeric field carries a formatter ID from `RPT.MODEL.FMT`.
- **AC-5** (clearance gate) Given a form template marked `pending`, when the packaging test runs under the `release` profile, then it fails naming the template; under the default profile it passes with a warning.
- **AC-6** (clean room) Given a test label whose normalised hash is in `fixtures/rpt/forms/cleanroom_phrases`, when the CI check runs, then it fails naming the label key.

## Design

- **Registry commands:** `report.show_layout_fai@1() -> Vec<FormLayoutInfo>` and `report.show_layout_ppap@1() -> Vec<FormLayoutInfo>` (R0).
- **Data structures:** `FieldMap { schema_version: u32, form: FormKind, fields: Vec<FieldEntry> }`; `FieldEntry { field_id, label_key, source: SourcePath, citation: Option<Citation> }`; `Citation { designator, revision, clause }`; `FormLayoutInfo { template: TemplateRef, field_map_digest, clearance: Pending | Cleared { review_ref } }`; block kind `form_field` with payload `{ field_id, show_label: bool }`; the `FieldSource` trait (`field(id)`, `rows(table)`) is defined in `RPT.MODEL.BIND.RESOLVE`.
- **Algorithm:** at startup register the built-in templates (embedded canonical JSON) as locked; validate each field map; `form_field` resolves through the `BlockResolver` trait of `RPT.MODEL.BIND.RESOLVE`, which supplies the active `FieldSource`.
- **Errors:** `report.template_locked` (raised by the template layer), `report.citation_invalid`, `report.field_unknown`, `report.fieldmap_invalid`.
- **Determinism:** D0.
- **Performance budget:** template registration under 5 ms at startup [unverified until measured].
- **Crate:** `fid-report-forms`.

## Test fixtures

`fixtures/rpt/forms/bad_fieldmaps` (hand-written invalid maps), `fixtures/rpt/forms/fai_complete` (shared), `fixtures/rpt/forms/cleanroom_phrases` (phrase hashes only; shared with `RPT.FORMS.HDR`). CC-BY-4.0.

## Build prompt

> In crate `fid-report-forms`, implement the shared machinery for fixed form templates.
>
> **Deliver:** `FieldMap`/`FieldEntry`/`Citation` types (canonical JSON, versioned) and a validator (sorted, unique field IDs; citations are designator + revision/edition + clause only, max 64 bytes, else `report.citation_invalid`); block kind `form_field` (payload `{field_id, show_label}`) implementing the `BlockResolver` trait from the binding engine, resolving values from the binding engine's `FieldSource` trait that form-data crates implement (`report.field_unknown` for unmapped IDs); numbers only through the report formatter layer, text stays `Untrusted`. Register built-in form templates as `locked` (the template save path refuses them with `report.template_locked`). Each template carries `clearance: pending | cleared{review_ref}`; a packaging test under the `release` profile fails if any is pending (D-015 counsel condition). Commands `report.show_layout_fai@1` and `report.show_layout_ppap@1` (R0; CLI `fid report show-layout-fai | show-layout-ppap`, `--json`). CI check: no label whose normalised SHA-256 is in the clean-room phrase hash list.
>
> **Acceptance:** AC-1..AC-6 in `spec/rpt/forms/layout/map/NODE.md`.
>
> **Constraints:** CONSTITUTION §8 clean room and C9 (no text, layout artwork, logos or trade dress from SAE AS9102 or the AIAG PPAP manual; cite designators and clauses only), C6, §8 (no panic, no `unsafe`, deny `unwrap`/`expect`).

## Open questions

- D-015 asks counsel whether an original layout may still order fields by the standards' clause order [unverified]. Field order lives in template data, so either answer is a data change in the FAI and PPAP leaves, not a code change here.

## Children

_Leaf: none._
