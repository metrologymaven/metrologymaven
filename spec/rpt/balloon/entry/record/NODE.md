---
id: RPT.BALLOON.ENTRY.RECORD
title: Single manual reading entry, voiding and listing
level: 4
parent: RPT.BALLOON.ENTRY
kind: feature
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-RPT.BALLOON.ENTRY-001, REQ-RPT.BALLOON.ENTRY-002, REQ-RPT.BALLOON.ENTRY-003, REQ-RPT.BALLOON.ENTRY-004, REQ-RPT.BALLOON.ENTRY-005, REQ-RPT.BALLOON.ENTRY-006, REQ-RPT.BALLOON.ENTRY-007, REQ-RPT.BALLOON.ENTRY-008]
covers: []
depends_on: [RPT.BALLOON.GAUGE, GDT.CHAR.RECORD, CORE.BUS.UNIT, CORE.BUS.PRIN, CORE.BUS.PIPE]
side_effect: [W2, W2, R0]
determinism: D0
cli: [fid report enter-reading, fid report void-reading, fid report list-reading]
mcp: [report_enter_reading, report_void_reading, report_list_reading]
journal: [report.enter_reading@1, report.void_reading@1, report.list_reading@1]
updated: 2026-11-17
---

# RPT.BALLOON.ENTRY.RECORD: Single manual reading entry, voiding and listing

## Purpose

The core of manual entry: an operator types one reading taken with a hand gauge for one characteristic in one run, and the system validates it and records it as a journaled ResultRecord with evaluation basis `DIRECT_READING`, the value and unit exactly as typed, the cited gauge, the operator and the entry time. Readings are a person's observation, so the entry and void commands are `human_only` (CONSTITUTION §3, D-031): the bus refuses agent and script principals. This leaf also voids a reading with a reason and lists readings.

## Scope

**In:**
- `report.enter_reading@1` (W2, `human_only`), `report.void_reading@1` (W2, `human_only`), `report.list_reading@1` (R0)
- Bounded parsing of the typed value through `CORE.BUS.UNIT` (decimal literal plus unit), unit compatibility with the characteristic's quantity
- Gauge checks through `RPT.BALLOON.GAUGE`: gauge required, retired gauge refused, overdue flag, decimals versus resolution warning, uncertainty class
- ResultRecord assembly through the `GDT.CHAR.RECORD` builder with basis `DIRECT_READING`; the decision comes from UNC through that builder
- `ManualReading` record type (reading ID, characteristic, run, value literal, gauge, flags, operator, entry time, void state)

**Out** (and which node owns it instead):
- Entering one value per part across a multi-part run (`RPT.BALLOON.ENTRY.SERIES`)
- The gauge register (`UNC.INST.LIB`) and gauge checks (`RPT.BALLOON.GAUGE`)
- Decision rules and uncertainty evaluation (`UNC.DEC`, `UNC.INST`)
- Driver-connected gauge input (`HAL.DRV`)

## Requirements

- **REQ-RPT.BALLOON.ENTRY.RECORD-001** When a human enters a valid reading, the system shall commit one `report.enter_reading@1` event holding a ResultRecord with basis `DIRECT_READING`, the value literal and unit exactly as typed, the SI value from one journaled rounding, the gauge ID, the operator principal and the entry time.
- **REQ-RPT.BALLOON.ENTRY.RECORD-002** If the reading cites no gauge, then the command shall fail with error `report.reading_without_gauge` and journal nothing.
- **REQ-RPT.BALLOON.ENTRY.RECORD-003** If the cited gauge is retired or unknown, then the command shall fail with error `report.gauge_retired` or `report.gauge_unknown` and journal nothing.
- **REQ-RPT.BALLOON.ENTRY.RECORD-004** If the gauge's calibration was overdue on the entry date, then the system shall record the reading with flag `gauge_calibration_overdue` and return warning `report.gauge_calibration_overdue`.
- **REQ-RPT.BALLOON.ENTRY.RECORD-005** If the typed value has more decimal places than the gauge's resolution supports in the typed unit, then the system shall return warning `report.reading_exceeds_resolution`, add flag `exceeds_resolution`, and keep the value as typed.
- **REQ-RPT.BALLOON.ENTRY.RECORD-006** The record shall carry the uncertainty class reported by `RPT.BALLOON.GAUGE` for the cited gauge, which is UEC-0 unless the gauge links an accepted instrument model.
- **REQ-RPT.BALLOON.ENTRY.RECORD-007** If the typed value is longer than 64 bytes, is not a decimal literal, or carries a unit not compatible with the characteristic's quantity, then the command shall fail with error `report.reading_invalid` and journal nothing.
- **REQ-RPT.BALLOON.ENTRY.RECORD-008** `report.enter_reading@1` and `report.void_reading@1` shall be declared `human_only: true`, so that the bus refuses agent and script principals with its policy error and MCP projects them as request-only.
- **REQ-RPT.BALLOON.ENTRY.RECORD-009** When a human voids a reading with a reason of 1 to 500 bytes, the system shall journal the void, keep the original event, and mark the reading `voided` so that reports and data sets exclude it.
- **REQ-RPT.BALLOON.ENTRY.RECORD-010** `report.list_reading@1` shall be R0 and return the readings of a run, or of a characteristic across runs, ordered by characteristic, run and entry journal position, including voided readings with their reasons.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/rpt/balloon/entry_run` (characteristic C1 diameter 12.000 mm +0.018/0 ASME with a bound rule, run R1, gauge G-02 resolution 0.001 mm due 2026-06-30) and a CLI session upgraded to human, when `fid report enter-reading --char C1 --run R1 --value "12.009 mm" --gauge G-02 --json` runs on 2026-05-01, then one event is journaled whose ResultRecord has basis `DIRECT_READING`, literal `12.009 mm`, gauge G-02, UEC-0 and the outcome UNC returns.
- **AC-2** (error path) When the same command runs without `--gauge`, then it fails with `report.reading_without_gauge` and the journal length is unchanged; with `--value "12.0O9 mm"`, it fails with `report.reading_invalid`; with `--value "12.009 s"`, it fails with `report.reading_invalid`.
- **AC-3** Given the entry date 2026-07-01, then the reading is recorded with flag `gauge_calibration_overdue` and the warning is returned.
- **AC-4** Given `--value "12.0091 mm"` with G-02, then warning `report.reading_exceeds_resolution` is returned and the literal is stored as `12.0091 mm`.
- **AC-5** (principal) Given an MCP agent principal, or a CLI session that is not upgraded (script principal), when `enter-reading` or `void-reading` is called, then the bus refuses it and nothing is journaled.
- **AC-6** Given AC-1, when the reading is voided with reason "gauge dropped", then `list-reading --run R1` shows it as `voided` with the reason, the original event is still in the journal, and a report generated at the new head excludes it.
- **AC-7** (replay) Given the journal after AC-6, when replayed in exact mode, then the outcome is `IDENTICAL`.

## Design

- **Registry commands:**
  - `report.enter_reading@1(char: CharId, run: RunId, value: Untrusted<Text>, gauge: Option<GaugeId>, note: Option<Untrusted<Text>>) -> { reading: ManualReading, warnings: Vec<Warning> }`: W2, `human_only: true`, `--json`, `--dry-run`.
  - `report.void_reading@1(reading: ReadingId, reason: Untrusted<Text>) -> ManualReading`: W2, `human_only: true`.
  - `report.list_reading@1(run: Option<RunId>, char: Option<CharId>) -> Vec<ManualReading>`: R0.
- **Data structures:** `ManualReading { id: ReadingId (UUIDv7), char, run, literal: AuthoredQuantity, gauge: GaugeId, flags: BTreeSet<ReadingFlag>, operator: PrincipalId, entered_at: Timestamp, result: ResultRecordRef, void: Option<VoidInfo { reason, by, at }> }`; `ReadingFlag = GaugeCalibrationOverdue | ExceedsResolution`.
- **Algorithm (enter):** bus authorises (`human_only`) → length and literal check, then `CORE.BUS.UNIT` parse and unit-quantity check → gauge check (`RPT.BALLOON.GAUGE`) on the UTC date of the entry time → build the ResultRecord with the `GDT.CHAR.RECORD` builder (basis `DIRECT_READING`, UEC from the gauge check, decision from UNC through the builder) → commit one event.
- **Errors:** `report.reading_without_gauge`, `report.gauge_unknown`, `report.gauge_retired`, `report.reading_invalid { reason }`, `report.not_found`, `report.already_voided`, `report.text_invalid`; bus `policy` refusal for non-human principals.
- **Determinism:** D0; the entry time and IDs are journaled values read back on replay.
- **Performance budget:** under 5 ms per entry excluding journal I/O [unverified until measured].
- **Crate:** `fid-report` (module `balloon::entry`).

## Test fixtures

- `fixtures/rpt/balloon/entry_run`: synthetic plan with characteristics C1–C3, run R1, a bound decision rule and the gauge register from `fixtures/rpt/balloon/gauge_register`; built by a test builder, CC-BY-4.0.

## Build prompt

> In crate `fid-report`, module `balloon::entry`, implement manual reading entry: `report.enter_reading@1` (W2, `human_only: true`), `report.void_reading@1` (W2, `human_only: true`) and `report.list_reading@1` (R0), each with `--json`; CLI `fid report enter-reading`, `void-reading`, `list-reading`; MCP names derived (the human-only ones project as request-only).
>
> Enter: reject values over 64 bytes or not a decimal literal with a unit compatible with the characteristic's quantity (`report.reading_invalid`); parse through the authored-literal normaliser so the literal is kept exactly and the SI value comes from one journaled rounding. Require a gauge (`report.reading_without_gauge`); refuse unknown or retired gauges; use the gauge-check library for calibration (overdue → flag `gauge_calibration_overdue` and warning), decimals versus resolution (too many → warning `report.reading_exceeds_resolution`, flag, value kept) and the uncertainty class (UEC-0 unless an accepted model is linked). Build the ResultRecord with the GDT ResultRecord builder, basis `DIRECT_READING`; the decision comes only from UNC through that builder. Commit one event. Void: reason 1–500 bytes, keep the original event, mark the reading voided so reports and data sets exclude it. List: ordered by characteristic, run, journal position, voided readings included.
>
> **Acceptance tests:** AC-1 to AC-7 (fixture `fixtures/rpt/balloon/entry_run`), including the principal refusal test and exact replay giving `IDENTICAL`.
>
> **Constraints:** CONSTITUTION §8 (no panic, no `unsafe`, no `unwrap`/`expect`); C1 (no decision or uncertainty computed here); C6 (typed values and notes are untrusted); `BTreeMap`/`BTreeSet` only. Spec: `spec/rpt/balloon/entry/record/NODE.md`.

## Open questions

- **Cross-domain:** `GDT.CHAR.RECORD` requires a DMT row ID and operator-chain digest on every record (REQ-GDT.CHAR.RECORD-001) and rejects a second record per run and characteristic (REQ-GDT.CHAR.RECORD-006). Direct readings have no DMT row, and re-entry after a void needs the voided record exempted; GDT should define the `DIRECT_READING` variant of both rules.
- Voiding is made `human_only` here as well as entry, because it removes an operator's observation from reports; confirm with maintainers.

## Children

_Leaf: none._
