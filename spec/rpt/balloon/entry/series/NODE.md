---
id: RPT.BALLOON.ENTRY.SERIES
title: Multi-part reading series entry
level: 4
parent: RPT.BALLOON.ENTRY
kind: feature
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-RPT.BALLOON.ENTRY-001, REQ-RPT.BALLOON.ENTRY-007]
covers: []
depends_on: [RPT.BALLOON.ENTRY.RECORD, CORE.PRJ.STAGE]
side_effect: W2
determinism: D0
cli: [fid report enter-series-reading]
mcp: [report_enter_series_reading]
journal: [report.enter_series_reading@1]
updated: 2026-11-17
---

# RPT.BALLOON.ENTRY.SERIES: Multi-part reading series entry

## Purpose

In a multi-part run (several parts measured as stages of one run, `CORE.PRJ.STAGE`), an operator often measures one characteristic on every part with the same gauge and types the values in a column. This leaf enters such a series in one command: one reading per part, in part-sequence order, each validated exactly as a single reading (`RPT.BALLOON.ENTRY.RECORD`), committed together as one journaled event. It is `human_only` like single entry.

## Scope

**In:**
- `report.enter_series_reading@1` (W2, `human_only`): one characteristic, one gauge, an ordered list of (stage, value) pairs
- Reuse of the single-entry validation and ResultRecord assembly for every element
- All-or-nothing commit; per-element warnings
- Up to 500 elements per command

**Out** (and which node owns it instead):
- Single-reading validation, record assembly, voiding and listing (`RPT.BALLOON.ENTRY.RECORD`)
- Stage creation and ordering (`CORE.PRJ.STAGE`)
- The interactive entry grid widget (desktop app surfaces; it only calls this command)

## Requirements

- **REQ-RPT.BALLOON.ENTRY.SERIES-001** When a human enters a series, the system shall validate every element with the single-entry rules and, if all are valid, commit one `report.enter_series_reading@1` event that holds one `DIRECT_READING` ResultRecord per element in the given stage order.
- **REQ-RPT.BALLOON.ENTRY.SERIES-002** If any element is invalid, then the command shall fail with error `report.series_invalid` listing every failing element index and its error, and journal nothing.
- **REQ-RPT.BALLOON.ENTRY.SERIES-003** If a stage appears twice in the series or does not belong to the run, then the command shall fail with error `report.series_invalid` and journal nothing.
- **REQ-RPT.BALLOON.ENTRY.SERIES-004** The command shall return the single-entry warnings (calibration overdue, exceeds resolution) per element index.
- **REQ-RPT.BALLOON.ENTRY.SERIES-005** The command shall be declared `human_only: true` and shall refuse a series of more than 500 elements with error `report.series_too_long`.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/rpt/balloon/entry_five_parts` (run R1 with stages S1–S5, characteristic C1, gauge G-02) and a human session, when `fid report enter-series-reading --char C1 --run R1 --gauge G-02 --values S1=12.004mm,S2=12.006mm,S3=12.011mm,S4=12.002mm,S5=12.019mm --json` runs, then one event holds five ResultRecords in stage order S1–S5 with the literals as typed.
- **AC-2** (error path) When the value for S3 is `12,011mm` and for S5 is empty, then it fails with `report.series_invalid` listing indices 2 and 4, and the journal length is unchanged.
- **AC-3** When S2 is listed twice, then it fails with `report.series_invalid`; when 501 elements are sent, then `report.series_too_long`.
- **AC-4** (principal) Given an agent or non-upgraded script principal, then the bus refuses the command and nothing is journaled.
- **AC-5** (replay) Given the journal after AC-1, when replayed in exact mode, then the outcome is `IDENTICAL`.

## Design

- **Registry command:** `report.enter_series_reading@1(char: CharId, run: RunId, gauge: GaugeId, values: Vec<(StageId, Untrusted<Text>)>) -> { readings: Vec<ManualReading>, warnings: Vec<(usize, Warning)> }`: W2, `human_only: true`, `--json`, `--dry-run`.
- **Algorithm:** check length and stage uniqueness and membership; run the single-entry validation function from `RPT.BALLOON.ENTRY.RECORD` on every element, collecting all errors; if none, build all records and commit one event.
- **Errors:** `report.series_invalid { failures: Vec<(usize, ErrorCode)> }`, `report.series_too_long`; bus `policy` refusal for non-human principals.
- **Determinism:** D0; output order is the input stage order.
- **Performance budget:** a 500-element series in under 100 ms excluding journal I/O [unverified until measured].
- **Crate:** `fid-report` (module `balloon::entry::series`).

## Test fixtures

- `fixtures/rpt/balloon/entry_five_parts`: synthetic run with five stages built on `fixtures/rpt/balloon/entry_run`; CC-BY-4.0.

## Build prompt

> In crate `fid-report`, module `balloon::entry::series`, implement `report.enter_series_reading@1` (W2, `human_only: true`, `--json`, `--dry-run`; CLI `fid report enter-series-reading`; MCP name derived and projected request-only).
>
> Input: characteristic, run, gauge and an ordered list of (stage, typed value), at most 500 (`report.series_too_long`). Reject duplicate stages or stages outside the run. Validate every element with the single-entry validation function of the manual-entry module and collect all failures into `report.series_invalid { failures }`; journal nothing on any failure. Otherwise build one `DIRECT_READING` ResultRecord per element (decision from UNC through the GDT builder, never here) and commit them in one event, returning per-index warnings.
>
> **Acceptance tests:** AC-1 to AC-5 (fixture `fixtures/rpt/balloon/entry_five_parts`), including the principal refusal and exact replay giving `IDENTICAL`.
>
> **Constraints:** CONSTITUTION §8 (no panic, no `unsafe`, no `unwrap`/`expect`); C6 (typed values are untrusted). Spec: `spec/rpt/balloon/entry/series/NODE.md`.

## Open questions

- None yet.

## Children

_Leaf: none._
