---
id: RPT.FORMS.PPAP
title: PPAP dimensional results data
level: 3
parent: RPT.FORMS
kind: feature
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-RPT.FORMS-002]
covers: []
depends_on: [RPT.BALLOON.REG, RPT.MODEL.FMT, RPT.MODEL.BIND.GEN, RPT.FORMS.LAYOUT.MAP, CORE.PRJ.CONT, GDT.CHAR.RECORD]
uses_artifacts: [RPT.FORMS.LAYOUT.PPAP]
side_effect: [W2, W2, R1]
determinism: D0
cli: [fid report create-ppap, fid report set-header-ppap, fid report generate-ppap]
mcp: [report_create_ppap, report_set_header_ppap, report_generate_ppap]
journal: [report.create_ppap@1, report.set_header_ppap@1, report.generate_ppap@1]
updated: 2026-11-17
---

# RPT.FORMS.PPAP: PPAP dimensional results data

## Purpose

The data behind a PPAP dimensional results report: a PPAP record with its header (part, revision, organisation, submission context, sample parts) and rows for every ballooned characteristic of the drawing revision, with the requirement, one result per sample part and an OK / not-OK outcome derived from the journaled decisions. `report.generate_ppap@1` builds the draft as a scratch `ResolvedReport` by resolving the locked `tmpl:ppap-dim` template against these rows.

## Scope

**In:** `PpapRecord`, `PpapHeader`, `PpapDimRow`; the header field list in our own words; the ordered list of sample parts (each a run or part instance of a multi-part run); the row builder; the outcome rule; the `FieldSource` implementation for the PPAP template; `report.create_ppap@1`, `report.set_header_ppap@1` (W2) and `report.generate_ppap@1` (R1).

**Out:** other PPAP elements (control plan, PFMEA, MSA studies; not in RPT); completeness check and finalisation (`RPT.FORMS.FINAL`); template appearance (`RPT.FORMS.LAYOUT.PPAP`); balloon numbering (`RPT.BALLOON.REG`); deciding conformance (`UNC.DEC`).

## Requirements

- **REQ-RPT.FORMS.PPAP-001** When `report.create_ppap@1` runs for a part revision and drawing revision, the system shall create a PPAP record with a UUIDv7 ID, an empty header and an empty sample-part list, and journal one event.
- **REQ-RPT.FORMS.PPAP-002** When `report.set_header_ppap@1` sets the sample parts, the system shall store them as an ordered list of run or part-instance IDs of the same part revision, with 1 to 300 entries, and fail with error `report.ppap_parts_invalid` otherwise.
- **REQ-RPT.FORMS.PPAP-003** The dimensional results shall contain one row per ballooned characteristic of the drawing revision in ascending balloon-number order, with one result cell per sample part in sample-list order.
- **REQ-RPT.FORMS.PPAP-004** Each result cell shall show the latest journaled result for that characteristic in that sample part, formatted by `RPT.MODEL.FMT`, or the fixed text "no result" when there is none.
- **REQ-RPT.FORMS.PPAP-005** A row's outcome shall be OK only if every sample part has a result with decision `CONFORMS`; otherwise the outcome shall be not OK and shall list each distinct non-conforming outcome (`NONCONFORMS`, `INDETERMINATE`, `NOT_DECIDABLE`, or missing) with the part sequence numbers that caused it.
- **REQ-RPT.FORMS.PPAP-006** `report.generate_ppap@1` shall be side-effect class R1, write nothing to the Record Journal, and produce identical content digests for the same journal head and PPAP record.
- **REQ-RPT.FORMS.PPAP-007** The header field list and labels shall be written in the project's own words, citing the AIAG PPAP manual by edition and section only [unverified edition: 4th assumed].
- **REQ-RPT.FORMS.PPAP-008** All header text shall be untrusted text of at most 512 UTF-8 bytes without NUL (CONSTITUTION C6), and otherwise the command shall fail with error `report.text_invalid`.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/rpt/forms/ppap_6parts_complete`, when `fid report create-ppap --part-rev <r> --drawing-rev <d> --json` and `set-header-ppap --parts p1,...,p6` run, then the record holds six parts in order and two events are journaled.
- **AC-2** Given AC-1, when `fid report generate-ppap --ppap <id> --json` runs, then the draft has one row per balloon in ascending order, six result cells per row in part order, and the Record Journal length is unchanged.
- **AC-3** Given fixture `fixtures/rpt/forms/ppap_6parts_gap` (balloon 7: part 4 missing, part 2 `NONCONFORMS`), when generated, then row 7's outcome is not OK listing `NONCONFORMS [2]` and `missing [4]`, and cell (7, 4) shows "no result".
- **AC-4** (error path) Given AC-1, when `set-header-ppap --parts` names a run of another part revision, or 301 parts, then it fails with `report.ppap_parts_invalid` and journals nothing.
- **AC-5** (determinism) Given AC-2, when generation runs twice at the same head, then the two content digests are identical; the digest also equals the golden `fixtures/rpt/forms/golden/ppap-dim.digest`.
- **AC-6** (replay) Given the journal from AC-1, when replayed in exact mode, then the outcome is `IDENTICAL`.

## Design

- **Registry commands:**
  - `report.create_ppap@1(part_rev: PartRevisionId, drawing_rev: DrawingRevisionId) -> PpapSummary` (W2)
  - `report.set_header_ppap@1(ppap: PpapId, fields: BTreeMap<FieldId, Option<Text>>, parts: Option<Vec<SampleRef>>) -> PpapHeader` (W2)
  - `report.generate_ppap@1(ppap: PpapId, head: Option<JournalHash>) -> ResolvedReport` (R1)
- **Data structures:** `PpapRecord { id, part_rev, drawing_rev, header: PpapHeader }`; `PpapHeader { fields: BTreeMap<FieldId, Untrusted<Text>>, parts: Vec<SampleRef> }`; `PpapDimRow { balloon: u32, characteristic, requirement: ValueText, cells: Vec<Cell>, outcome: Ok | NotOk(Vec<(Cause, Vec<u32>)>) }`.
- **Algorithm:** read the balloon register of the drawing revision; for each balloon in ascending order and each sample part, select the latest journaled ResultRecord (`GDT.CHAR.RECORD`) for that characteristic and part up to the head; format through `RPT.MODEL.FMT`; derive the outcome; expose rows and header through the `FieldSource` trait of `RPT.MODEL.BIND.RESOLVE`; resolve `tmpl:ppap-dim` with `RPT.MODEL.BIND.GEN`.
- **Errors:** `report.not_found`, `report.ppap_parts_invalid`, `report.text_invalid`, generation errors from the binding engine.
- **Determinism:** D0.
- **Performance budget:** generate 500 rows by 6 parts in under 1 s on reference hardware [unverified until measured].
- **Crate:** `fid-report-forms`.

## Test fixtures

`fixtures/rpt/forms/ppap_6parts_complete`, `fixtures/rpt/forms/ppap_6parts_gap` (synthetic multi-part projects from the CORE project test builder), golden digest from `RPT.FORMS.LAYOUT.PPAP`. CC-BY-4.0.

## Build prompt

> In crate `fid-report-forms`, implement the PPAP dimensional results record and draft generation.
>
> **Commands:** `report.create_ppap@1`, `report.set_header_ppap@1` (W2, one journal event each) and `report.generate_ppap@1` (R1, nothing in the Record Journal). CLI `fid report create-ppap | set-header-ppap | generate-ppap`, `--json`.
>
> **Rules:** sample parts are an ordered list (1..300) of runs or part instances of the same part revision, else `report.ppap_parts_invalid`. Rows: one per balloon of the drawing revision in ascending balloon number; one cell per sample part in list order holding the latest journaled result up to the head, formatted by the report formatter layer, or "no result". Outcome OK only if every part `CONFORMS`; else not OK listing each cause with part sequence numbers. Header text is `Untrusted`, max 512 bytes, no NUL. Generation resolves the locked `tmpl:ppap-dim` template through the binding engine with this record as the `FieldSource`, and the content digest is identical for the same head and record.
>
> **Acceptance:** AC-1..AC-6 in `spec/rpt/forms/ppap/NODE.md`.
>
> **Constraints:** CONSTITUTION C1 (never compute or override a decision; read UNC outcomes), C6, §8 clean room (own-words labels, cite the AIAG PPAP manual by edition and section only), §8 (no panic, no `unsafe`, deny `unwrap`/`expect`).

## Open questions

- Number of sample parts expected by default (often six) [unverified]; it is not defaulted here: the user lists the parts.
- When a sample part was measured several times, the latest result is used, as for FAI form 3.

## Children

_Leaf: none._
