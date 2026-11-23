---
id: RPT.FORMS.LAYOUT.FAI
title: Original locked templates for FAI forms 1 to 3
level: 4
parent: RPT.FORMS.LAYOUT
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-RPT.FORMS.LAYOUT-001, REQ-RPT.FORMS.LAYOUT-002, REQ-RPT.FORMS.LAYOUT-003, REQ-RPT.FORMS.LAYOUT-004]
covers: []
depends_on: [RPT.FORMS.LAYOUT.MAP, RPT.MODEL.TMPL.FORMAT]
uses_artifacts: [RPT.FORMS.HDR, RPT.FORMS.ROWS, I18N.CATALOG]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
built_in: [fid-report-forms]
updated: 2026-11-22
---

# RPT.FORMS.LAYOUT.FAI: Original locked templates for FAI forms 1 to 3

## Purpose

The three built-in templates that put FAI data on pages: form 1 (part number accountability), form 2 (product accountability) and form 3 (characteristic accountability). Under D-015 option C they are **original arrangements**: our own grouping, labels in our own words, our own grid and styling, citing AS9102 by designator, revision and clause only. They carry the same data an auditor looks for, and their field maps let anyone check completeness field by field. Users who hold a licence to the published form can rebuild its look in `RPT.DESIGN` against the same field IDs.

## Scope

**In:** template documents `tmpl:fai-form1`, `tmpl:fai-form2`, `tmpl:fai-form3` in the `RPT.MODEL.TMPL.FORMAT` format, embedded in the crate as canonical JSON; their field maps; label message keys and English source strings for the catalog; form 3 table with a repetition slot (one row per form 3 row) and continuation settings; a golden resolved-report test.

**Out:** the field-map, locking and clearance machinery (`RPT.FORMS.LAYOUT.MAP`); the data (`RPT.FORMS.HDR`, `RPT.FORMS.ROWS`); page breaking and the repetition of headers on continuation pages at render time (`RPT.OUTPUT.PDF.LAYOUT`, which honours the template's continuation flags); translations (`I18N`).

## Requirements

- **REQ-RPT.FORMS.LAYOUT.FAI-001** The crate shall ship three locked templates for FAI forms 1, 2 and 3, each with a field map covering every field the template shows.
- **REQ-RPT.FORMS.LAYOUT.FAI-002** Every label shall be a catalog message key whose English source text is written in the project's own words, and the templates shall contain no logo, artwork, border pattern or text taken from the published form.
- **REQ-RPT.FORMS.LAYOUT.FAI-003** Each template's footer shall cite AS9102 by designator and revision only, and fields that need a source shall cite a clause number only [unverified revision: Rev C assumed].
- **REQ-RPT.FORMS.LAYOUT.FAI-004** The form 3 template shall contain one table whose rows come from a repetition slot over the form 3 rows in ascending balloon-number order, with columns for balloon number, requirement, result, decision, evaluation basis and producing tool.
- **REQ-RPT.FORMS.LAYOUT.FAI-005** The form 3 table shall set the template flags that make the renderer repeat the column header row and the identifying header block (part number, part revision, FAI report number, page n of m) on every continuation page.
- **REQ-RPT.FORMS.LAYOUT.FAI-006** Field order and grouping shall be held as template data, initially grouped by the project's own logical order (identity, organisation, scope, sign-off), so that a counsel answer under D-015 changes data only.
- **REQ-RPT.FORMS.LAYOUT.FAI-007** When the three templates are resolved against fixture `fixtures/rpt/forms/fai_complete`, the resolved reports shall equal committed golden digests, and a 500-row form 3 shall resolve with no warning.

## Acceptance criteria

- **AC-1** Given the built crate, when `fid report show-layout-fai --json` runs, then it lists `tmpl:fai-form1`, `tmpl:fai-form2` and `tmpl:fai-form3`, each locked, each with a field map that validates.
- **AC-2** Given fixture `fixtures/rpt/forms/fai_complete`, when each template is resolved with the FAI `FieldSource`, then the resolved-report digests equal the golden files `fixtures/rpt/forms/golden/fai-form{1,2,3}.digest`.
- **AC-3** Given fixture `fixtures/rpt/forms/fai_500rows`, when form 3 is resolved and laid out by the PDF layout engine, then every page after the first repeats the column header and the identifying header block, and no warning is reported.
- **AC-4** (clean room) Given the templates, when the clean-room CI check of `RPT.FORMS.LAYOUT.MAP` runs, then it passes, and a reviewer checklist item "original arrangement, no copied artwork" is recorded in the PR.
- **AC-5** Given the templates, when every citation is validated, then each is designator + revision or clause only.

## Design

- **Registry commands:** none; templates are data registered by `RPT.FORMS.LAYOUT.MAP`.
- **Data structures:** three template documents (schema of `RPT.MODEL.TMPL.FORMAT`) with blocks of kind `form_field`, `text` (label keys only), `table` (form 3, slot over `form3.rows`) and `footer`; template flags `repeat_header_rows: 1` and `continuation_block: <block id>` on the form 3 table.
- **Algorithm:** none at run time beyond registration; generation and layout are done by the binding engine and the PDF layout engine.
- **Errors:** template validation errors are raised at registration (`report.template_invalid`) and fail the crate's startup test.
- **Determinism:** D0 (static data, canonical JSON).
- **Performance budget:** not applicable.
- **Crate:** `fid-report-forms` (data under `templates/fai/`).

## Test fixtures

`fixtures/rpt/forms/fai_complete` (shared), `fixtures/rpt/forms/fai_500rows` (synthetic, 500 balloons), golden digests under `fixtures/rpt/forms/golden/`. All synthetic, CC-BY-4.0.

## Build prompt

> In crate `fid-report-forms`, add the three built-in FAI templates (forms 1, 2, 3) as canonical-JSON template documents in the report template format, registered locked through the form field-map machinery.
>
> **Content rules (D-015 option C, CONSTITUTION §8 clean room, C9):** original arrangement and styling; labels are catalog keys with English source text written in our own words; no logos, artwork, border patterns or text from SAE AS9102; cite the standard by designator and revision in the footer and by clause number only where a field needs a source. Field order is data, grouped as identity, organisation, scope, sign-off.
>
> **Form 3:** one table, rows from a repetition slot over form 3 rows in ascending balloon order; columns balloon, requirement, result, decision, basis, producing tool; set `repeat_header_rows` and `continuation_block` so the PDF layout engine repeats the column header and the identifying block (part number, revision, FAI report number, page n of m) on continuation pages.
>
> **Acceptance:** AC-1..AC-5 in `spec/rpt/forms/layout/fai/NODE.md`, including golden digests against `fixtures/rpt/forms/fai_complete` and the 500-row continuation test.
>
> **Constraints:** set clearance `pending` until the counsel review required by D-015 is recorded; never paste standards text into code, comments, tests or commit messages.

## Open questions

- Whether auditors accept an original arrangement in practice is [unverified]; collect feedback from design partners before 1.0.

## Children

_Leaf: none._
