---
id: RPT.BALLOON.GAUGE
title: Gauge citation checks for manual readings
level: 3
parent: RPT.BALLOON
kind: feature
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-RPT.BALLOON-006, REQ-RPT.BALLOON-007, REQ-RPT.BALLOON-008]
covers: []
depends_on: [UNC.INST.LIB, UNC.INST.SCHEMA]
side_effect: R0
determinism: D0
cli: [fid report check-gauge]
mcp: [report_check_gauge]
journal: [report.check_gauge@1]
updated: 2026-11-17
---

# RPT.BALLOON.GAUGE: Gauge citation checks for manual readings

## Purpose

Manual readings must cite the gauge they were taken with. The gauge register itself (gauge ID, type, resolution, range, calibration due date, certificate reference, status, optional uncertainty model) is owned by `UNC.INST.LIB`, as agreed at review, so that hand entry here and driver input in `HAL.DRV` share one register. This leaf only **references** that register: it answers the three questions reading entry needs about a cited gauge on a given date (is it usable, how many decimals its resolution supports in the entered unit, which uncertainty class its readings get) as a library for `RPT.BALLOON.ENTRY` and as one R0 command so a person or script can check a gauge before entering readings.

## Scope

**In:**
- `GaugeCheck` library: lookup of a gauge by ID in the `UNC.INST.LIB` register (including retired gauges, so past readings still resolve)
- Calibration status on a date: `current`, `overdue` or `no_due_date`
- Maximum supported decimal places of a gauge's resolution in a requested unit, by exact rational arithmetic
- Uncertainty status: UEC-0 unless the gauge links an instrument model that `UNC.INST.SCHEMA` reports as accepted
- Command `report.check_gauge@1` (R0)

**Out** (and which node owns it instead):
- The gauge register, its records and its add, update and retire commands (`UNC.INST.LIB`)
- Instrument uncertainty models and their acceptance (`UNC.INST.SCHEMA`)
- Wired, wireless and keyboard-wedge gauge input (`HAL.DRV`)
- Recording readings, flags and warnings (`RPT.BALLOON.ENTRY`)

## Requirements

- **REQ-RPT.BALLOON.GAUGE-001** When asked about a gauge ID, the library shall resolve it in the `UNC.INST.LIB` register, including retired gauges, and if the ID is unknown, then it shall return error `report.gauge_unknown`.
- **REQ-RPT.BALLOON.GAUGE-002** The library shall report calibration status `overdue` for a date strictly later than the gauge's calibration due date, `current` for a date on or before it, and `no_due_date` when the register holds none.
- **REQ-RPT.BALLOON.GAUGE-003** The library shall report the maximum supported decimal places in a unit as the smallest integer d ≥ 0 for which 10^−d of that unit is not larger than the gauge resolution expressed in that unit, computed with exact rational unit factors.
- **REQ-RPT.BALLOON.GAUGE-004** The library shall report uncertainty class UEC-0 for a gauge's readings unless the gauge links an instrument model that `UNC.INST.SCHEMA` reports as accepted, in which case it shall report the model reference and the class that UNC assigns to it.
- **REQ-RPT.BALLOON.GAUGE-005** The library shall report whether the gauge is retired, so that entry can refuse new readings with a retired gauge while past readings still resolve.
- **REQ-RPT.BALLOON.GAUGE-006** `report.check_gauge@1` shall be side-effect class R0 and return, for a gauge ID, a date and an optional unit, the status, calibration status, maximum decimals and uncertainty class.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/rpt/balloon/gauge_register` (a caliper G-01 with resolution 0.01 mm due 2026-12-31, a micrometer G-02 with resolution 0.001 mm due 2026-06-30, a retired height gauge G-03), when `fid report check-gauge --gauge G-01 --date 2026-12-31 --unit mm --json` runs, then the result is `current`, 2 decimals, UEC-0.
- **AC-2** Given the same fixture, when G-01 is checked on 2027-01-01, then calibration status is `overdue`; when G-02 is checked with `--unit in`, then max decimals is 5 (0.001 mm is about 0.0000394 in).
- **AC-3** Given G-02 linked to an accepted instrument model in the fixture, then the result carries the model reference and the class UNC assigns; given G-01 with no model, UEC-0.
- **AC-4** Given G-03, then status `retired` is reported; given `G-99`, then error `report.gauge_unknown`.
- **AC-5** (journal) When `check-gauge` runs, then nothing is written to the Record Journal (R0; Activity Log only while recording, CONSTITUTION §3).

## Design

- **Registry command:** `report.check_gauge@1(gauge: GaugeId, date: Date, unit: Option<UnitId>) -> GaugeCheckResult` (R0, `--json`).
- **Library:** `trait GaugeRegister { fn get(&self, id: &GaugeId) -> Option<GaugeRecordView>; }` implemented over the `UNC.INST.LIB` register; `fn check(reg, id, date, unit) -> Result<GaugeCheckResult, GaugeError>`.
- **Data structures:** `GaugeCheckResult { gauge, status: Active | Retired, calibration: Current | Overdue | NoDueDate, max_decimals: Option<u8>, uncertainty: Uec0 | Model { model_ref, uec } }`. `GaugeRecordView` is the read view of the UNC-owned record (resolution as an authored decimal with unit, due date, certificate reference, status, model link).
- **Algorithm:** convert resolution to the requested unit as an exact rational using the unit catalogue factors; increase d from 0 until 10^−d ≤ resolution; cap at 9 (`report.resolution_too_fine` if exceeded). Dates are calendar dates; the caller supplies the entry date.
- **Errors:** `report.gauge_unknown`, `report.unit_incompatible` (unit not a length or angle compatible with the gauge's resolution), `report.resolution_too_fine`.
- **Determinism:** D0; rational integer arithmetic only.
- **Performance budget:** under 0.1 ms per check [unverified until measured].
- **Crate:** `fid-report` (module `balloon::gauge`).

## Test fixtures

- `fixtures/rpt/balloon/gauge_register`: three synthetic gauge records in the `UNC.INST.LIB` register format plus one accepted synthetic instrument model; authored in-project, CC-BY-4.0. Until `UNC.INST.LIB` publishes the gauge record format, tests use a stub `GaugeRegister`.

## Build prompt

> In crate `fid-report`, module `balloon::gauge`, implement gauge citation checks over the gauge register owned by the UNC instrument library (do not create a second register).
>
> Provide a `GaugeRegister` read trait (implemented over the UNC register; stub it in tests) and `check(reg, gauge_id, date, unit)` returning: active or retired; calibration `current` (date on or before due date), `overdue` (date after it) or `no_due_date`; maximum decimals d = smallest d ≥ 0 with 10^−d (in the requested unit) ≤ resolution, using exact rational unit factors (1 in = 25.4 mm exactly), capped at 9; uncertainty UEC-0 unless the gauge links an instrument model the UNC schema module reports as accepted, then that model reference and its UNC-assigned class. Expose it as `report.check_gauge@1` (R0, `--json`; CLI `fid report check-gauge`). Errors: `report.gauge_unknown`, `report.unit_incompatible`, `report.resolution_too_fine`.
>
> **Acceptance tests:** AC-1 to AC-5 (fixture `fixtures/rpt/balloon/gauge_register`).
>
> **Constraints:** CONSTITUTION §8 (no panic, no `unsafe`, no `unwrap`/`expect`); no floating point; no writes of any kind. Spec: `spec/rpt/balloon/gauge/NODE.md`.

## Open questions

- **Cross-domain:** `UNC.INST.LIB` (status open) must add gauge records to its register (ID, type, resolution with unit, range, calibration due date, certificate reference, active or retired, optional model link) and the commands to maintain them; this leaf assumes those fields. The former `report.add_gauge@1`, `report.update_gauge@1` and `report.retire_gauge@1` commands are dropped from RPT.
- Calendar date of an entry: the UTC date of the entry timestamp is assumed; a site time-zone setting may be needed for shops near midnight UTC.

## Children

_Leaf: none._
