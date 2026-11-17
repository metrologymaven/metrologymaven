---
id: RPT.FORMS.ROWS
title: Characteristic accountability rows (form 3)
level: 3
parent: RPT.FORMS
kind: feature
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-RPT.FORMS-003, REQ-RPT.FORMS-004]
covers: []
depends_on: [RPT.BALLOON.REG, RPT.MODEL.FMT, RPT.MODEL.BIND.GEN, RPT.FORMS.HDR, RPT.FORMS.LAYOUT.MAP, GDT.CHAR.RECORD]
uses_artifacts: [RPT.FORMS.LAYOUT.FAI]
side_effect: R1
determinism: D0
cli: [fid report generate-fai]
mcp: [report_generate_fai]
journal: [report.generate_fai@1]
updated: 2026-11-17
---

# RPT.FORMS.ROWS: Characteristic accountability rows (form 3)

## Purpose

Builds the form 3 table of an FAI, one row per ballooned characteristic in balloon-number order, showing the requirement, the result, the decision, the evaluation basis and the tool or software that produced it, and assembles the whole draft FAI (header and forms 1-2 from `RPT.FORMS.HDR` plus these rows) as a scratch `ResolvedReport` by resolving the locked FAI templates. It reads the balloon register and journaled ResultRecords; it never creates results.

## Scope

**In:** `FaiForm3Row`; the row builder over the balloon register of the FAI's drawing revision; selection of the run used for the FAI; partial-FAI filtering; the `FieldSource` implementation for the FAI templates; `report.generate_fai@1` (R1) producing one `ResolvedReport` with forms 1, 2 and 3.

**Out:** balloon numbering (`RPT.BALLOON.REG`); header and forms 1-2 data (`RPT.FORMS.HDR`); completeness check and finalisation (`RPT.FORMS.FINAL`); template appearance (`RPT.FORMS.LAYOUT.FAI`); value formatting and labels (`RPT.MODEL.FMT`).

## Requirements

- **REQ-RPT.FORMS.ROWS-001** The form 3 table shall contain exactly one row per ballooned characteristic of the drawing revision in scope, ordered by ascending balloon number.
- **REQ-RPT.FORMS.ROWS-002** Each row shall show the balloon number, the characteristic's requirement (nominal, tolerance and callout text through `RPT.MODEL.FMT`), the formatted result, the decision outcome, the evaluation basis and the producing tool or software.
- **REQ-RPT.FORMS.ROWS-003** When a characteristic has several journaled results in the run selected for the FAI, the row shall use the latest one up to the journal head and shall name that run's ID.
- **REQ-RPT.FORMS.ROWS-004** When a ballooned characteristic has no journaled result in the selected run, its row shall show the fixed text "no result" in the result, decision and basis cells and shall not be omitted.
- **REQ-RPT.FORMS.ROWS-005** When the FAI is partial, only characteristics in the header's scope list shall get rows, still in ascending balloon-number order.
- **REQ-RPT.FORMS.ROWS-006** `report.generate_fai@1` shall be side-effect class R1, write nothing to the Record Journal, and produce identical content digests for the same journal head, FAI record and selected run.
- **REQ-RPT.FORMS.ROWS-007** The producing tool cell shall show the gauge ID for `DIRECT_READING`, the source software name and version for `RESULTS_ONLY`, and otherwise the algorithm ID and the program or plan that produced the result.
- **REQ-RPT.FORMS.ROWS-008** If the selected run does not belong to the FAI's part revision, then generation shall fail with error `report.fai_run_invalid` and produce nothing.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/rpt/forms/fai_complete` (12 balloons, mixed CMM, scan and hand-gauge results), when `fid report generate-fai --fai <id> --run <r> --json` runs, then form 3 has 12 rows in ascending balloon order, each with requirement, result, decision, basis and producing tool, and the Record Journal length is unchanged.
- **AC-2** Given fixture `fixtures/rpt/forms/fai_gaps`, when generated, then rows 3 and 11 show "no result" and are present.
- **AC-3** Given fixture `fixtures/rpt/forms/fai_remeasured` (balloon 4 measured twice in the run), when generated, then row 4 shows the later result and names the run.
- **AC-4** Given a partial FAI with scope `[C3, C7]`, when generated, then form 3 has exactly those two rows in balloon order.
- **AC-5** (labels) Given AC-1, then the hand-gauge row's tool cell shows the gauge ID and a transcribed row shows source software name and version.
- **AC-6** (error path) Given a run of another part revision, when generated, then it fails with `report.fai_run_invalid`.
- **AC-7** (determinism) Given AC-1, when generated twice at the same head, then the digests are identical and equal the golden digests of `RPT.FORMS.LAYOUT.FAI`.

## Design

- **Registry command:** `report.generate_fai@1(fai: FaiId, run: RunId, head: Option<JournalHash>) -> ResolvedReport` (R1).
- **Data structures:** `FaiForm3Row { balloon: u32, characteristic: CharacteristicId, requirement: ValueText, result: Option<ValueText>, decision: Option<OutcomeText>, basis: Option<EvaluationBasis>, tool: Option<ToolLabel>, run: RunId, record: Option<ResultRecordId> }`.
- **Algorithm:** read the FAI record (`RPT.FORMS.HDR`) and its drawing revision's balloon register (`RPT.BALLOON.REG`); filter by partial scope; for each balloon in ascending order pick the latest ResultRecord (`GDT.CHAR.RECORD`) of the characteristic in the run up to the head; format requirement, result, decision and labels through `RPT.MODEL.FMT`; expose header, forms 1-2 entries and rows through the `FieldSource` trait of `RPT.MODEL.BIND.RESOLVE`; resolve `tmpl:fai-form1..3` with `RPT.MODEL.BIND.GEN` into one `ResolvedReport`.
- **Errors:** `report.not_found`, `report.fai_run_invalid`, generation errors from the binding engine.
- **Determinism:** D0.
- **Performance budget:** a 500-balloon FAI generates in under 1 s on reference hardware [unverified until measured].
- **Crate:** `fid-report-forms`.

## Test fixtures

`fixtures/rpt/forms/fai_complete`, `fai_gaps` (shared with FINAL), `fai_remeasured` (synthetic), golden digests from `RPT.FORMS.LAYOUT.FAI`. CC-BY-4.0.

## Build prompt

> In crate `fid-report-forms`, implement the AS9102 form 3 row builder and draft FAI generation.
>
> **Command:** `report.generate_fai@1(fai, run, head?)` (R1; nothing in the Record Journal; CLI `fid report generate-fai --json`; MCP generated).
>
> **Rules:** one row per ballooned characteristic of the FAI's drawing revision in ascending balloon order (partial FAI: only the header's scope); requirement, result, decision, basis and producing tool per row; latest result of the selected run up to the head (row names the run); missing result -> fixed text "no result", row kept; tool cell = gauge ID for `DIRECT_READING`, source software and version for `RESULTS_ONLY`, else algorithm ID plus program or plan. All values and labels come from the report formatter layer. A run from another part revision -> `report.fai_run_invalid`. Implement the form `FieldSource` trait over the FAI record and rows and resolve the locked FAI templates through the binding engine into one `ResolvedReport` with a content digest that is identical for the same inputs.
>
> **Acceptance:** AC-1..AC-7 in `spec/rpt/forms/rows/NODE.md`.
>
> **Constraints:** CONSTITUTION C1 (never compute or override results or decisions), C6, §5, §8 (no panic, no `unsafe`, deny `unwrap`/`expect`); no std HashMap/HashSet.

## Open questions

- Using the latest result when a characteristic was measured several times in the run (rather than all results as sub-rows) is assumed; sub-rows would be a new major of the command.

## Children

_Leaf: none._
