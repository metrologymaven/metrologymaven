---
id: RPT.FORMS.LAYOUT.PPAP
title: Original locked template for the PPAP dimensional results report
level: 4
parent: RPT.FORMS.LAYOUT
kind: task
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-RPT.FORMS.LAYOUT-001, REQ-RPT.FORMS.LAYOUT-002, REQ-RPT.FORMS.LAYOUT-003, REQ-RPT.FORMS.LAYOUT-004]
covers: []
depends_on: [RPT.FORMS.LAYOUT.MAP, RPT.MODEL.TMPL.FORMAT]
uses_artifacts: [RPT.FORMS.PPAP, I18N.CATALOG]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# RPT.FORMS.LAYOUT.PPAP: Original locked template for the PPAP dimensional results report

## Purpose

The built-in template that puts PPAP dimensional data on pages: a header block and a table with one row per ballooned characteristic and one result column per sample part, plus the row outcome. Like the FAI templates, it is an original arrangement under D-015 option C, citing the AIAG PPAP manual by edition and section only, with a field map for completeness checks.

## Scope

**In:** template document `tmpl:ppap-dim` in the `RPT.MODEL.TMPL.FORMAT` format; its field map; label keys and English source strings; a table whose sample-part columns repeat per part (column repetition slot); continuation flags; column overflow rule for many sample parts; golden test.

**Out:** field-map, locking and clearance machinery (`RPT.FORMS.LAYOUT.MAP`); PPAP data and outcomes (`RPT.FORMS.PPAP`); page breaking at render (`RPT.OUTPUT.PDF.LAYOUT`).

## Requirements

- **REQ-RPT.FORMS.LAYOUT.PPAP-001** The crate shall ship one locked template `tmpl:ppap-dim` with a field map covering every field it shows.
- **REQ-RPT.FORMS.LAYOUT.PPAP-002** Labels shall be catalog keys with English source text in the project's own words, and the template shall contain no logo, artwork or text from the AIAG PPAP manual; citations shall name the manual by edition and section only [unverified edition: 4th assumed].
- **REQ-RPT.FORMS.LAYOUT.PPAP-003** The table shall have one row per PPAP row in ascending balloon-number order and, after the specification column, one result column per sample part in part-sequence order, followed by the outcome column.
- **REQ-RPT.FORMS.LAYOUT.PPAP-004** When the number of sample parts exceeds the columns that fit the page width (6 by default for A4 or Letter landscape [unverified]), the template shall split the parts into column groups on successive page sets, each repeating the balloon, specification and outcome columns.
- **REQ-RPT.FORMS.LAYOUT.PPAP-005** The table shall set the continuation flags so every continuation page repeats the column header row and the identifying header block (part number, part revision, submission ID, page n of m).

## Acceptance criteria

- **AC-1** Given the built crate, when `fid report show-layout-ppap --json` runs, then it lists `tmpl:ppap-dim`, locked, with a valid field map.
- **AC-2** Given fixture `fixtures/rpt/forms/ppap_6parts_complete`, when the template is resolved with the PPAP `FieldSource`, then the resolved-report digest equals `fixtures/rpt/forms/golden/ppap-dim.digest`.
- **AC-3** Given fixture `fixtures/rpt/forms/ppap_10parts` (10 sample parts, 80 rows), when resolved and laid out, then parts 1-6 and 7-10 appear in two column groups, each repeating balloon, specification and outcome, and every continuation page repeats the header block.
- **AC-4** (clean room) Given the template, when the clean-room CI check runs, then it passes and every citation is edition and section only.

## Design

- **Registry commands:** none.
- **Data structures:** one template document with `form_field` header blocks, one `table` block with a row slot over `ppap.rows`, a column slot over `ppap.parts` with `max_columns_per_group: 6`, `repeat_columns: [balloon, spec, outcome]`, `repeat_header_rows: 1` and `continuation_block`.
- **Errors:** registration validation (`report.template_invalid`).
- **Determinism:** D0.
- **Performance budget:** not applicable.
- **Crate:** `fid-report-forms` (data under `templates/ppap/`).

## Test fixtures

`fixtures/rpt/forms/ppap_6parts_complete` (shared with FINAL), `fixtures/rpt/forms/ppap_10parts` (synthetic), golden digest. CC-BY-4.0.

## Build prompt

> In crate `fid-report-forms`, add the built-in PPAP dimensional results template `tmpl:ppap-dim` as a canonical-JSON template document, registered locked with its field map.
>
> **Content rules (D-015 option C, CONSTITUTION §8 clean room, C9):** original arrangement; labels are catalog keys with own-words English text; no artwork or text from the AIAG PPAP manual; cite it by edition and section only. Table: rows over PPAP rows in ascending balloon order; after the specification column one result column per sample part in part order, then outcome; at most 6 part columns per group, further parts in additional column groups that repeat balloon, specification and outcome; continuation flags repeat the column header and identifying block.
>
> **Acceptance:** AC-1..AC-4 in `spec/rpt/forms/layout/ppap/NODE.md`.
>
> **Constraints:** clearance `pending` until the D-015 counsel review is recorded; never paste standards text anywhere.

## Open questions

- The column-group default (6) and whether customers prefer one row per part instead of one column per part are [unverified]; both are template data.
- Column repetition (`column slot`) must be supported by `RPT.MODEL.TMPL.SLOT` and the PDF layout engine; it is named there.

## Children

_Leaf: none._
