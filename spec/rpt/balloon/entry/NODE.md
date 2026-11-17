---
id: RPT.BALLOON.ENTRY
title: Manual reading entry
level: 3
parent: RPT.BALLOON
kind: module
status: split
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-RPT.BALLOON-004, REQ-RPT.BALLOON-005, REQ-RPT.BALLOON-006, REQ-RPT.BALLOON-007, REQ-RPT.BALLOON-008, REQ-RPT.BALLOON-010]
covers: []
depends_on: [RPT.BALLOON.GAUGE, GDT.CHAR.RECORD, CORE.BUS.UNIT, CORE.BUS.PRIN, CORE.BUS.PIPE, CORE.PRJ.STAGE]
cli: [fid report enter-reading, fid report void-reading, fid report list-reading, fid report enter-series-reading]
mcp: [report_enter_reading, report_void_reading, report_list_reading, report_enter_series_reading]
journal: [report.enter_reading@1, report.void_reading@1, report.enter_series_reading@1]
updated: 2026-11-17
---

# RPT.BALLOON.ENTRY: Manual reading entry

## Purpose

Lets an operator type in readings taken with hand gauges, for one part or for each part of a multi-part run. Each reading becomes a journaled ResultRecord with evaluation basis `DIRECT_READING`, the entered value and unit as authored, and the cited gauge. The reading is evaluated against the characteristic's tolerance by GDT and decided by UNC; this module never computes a decision itself.

## Scope

**In:**
- Single and repeated entry (one value per part in a multi-part run, in part-sequence order)
- Validation: gauge required, gauge calibration status, decimals versus gauge resolution, unit compatibility
- Creating the ResultRecord through GDT's result path with basis `DIRECT_READING`, UEC-0 unless the gauge has an accepted uncertainty model
- Voiding a reading with a reason (the original stays in the journal)
- Principal rule: readings are entered by humans only

**Out** (and which node owns it instead):
- Gauge register (`RPT.BALLOON.GAUGE`)
- Tolerance evaluation and decision (`GDT.CHAR`, `UNC.DEC`)
- Driver-connected gauge input (`HAL.DRV`)

## Requirements

- **REQ-RPT.BALLOON.ENTRY-001** When the user enters a reading, the system shall create a ResultRecord with evaluation basis `DIRECT_READING`, the value and unit exactly as entered, the cited gauge ID, the operator and the entry time, and journal it.
- **REQ-RPT.BALLOON.ENTRY-002** If a reading cites no gauge, then the system shall reject it with error `report.reading_without_gauge` and journal nothing.
- **REQ-RPT.BALLOON.ENTRY-003** If the cited gauge's calibration was overdue on the entry date, then the system shall record the reading with flag `gauge_calibration_overdue` and warn the user.
- **REQ-RPT.BALLOON.ENTRY-004** If a reading has more decimal places than the cited gauge's resolution supports in the entered unit, then the system shall warn the user and keep the value as entered.
- **REQ-RPT.BALLOON.ENTRY-005** A reading shall carry uncertainty class UEC-0 unless the cited gauge links an uncertainty model accepted by `UNC.INST`.
- **REQ-RPT.BALLOON.ENTRY-006** Typed reading values shall be parsed by a bounded decimal parser and treated as untrusted content; anything that is not a decimal number with an allowed unit shall be rejected with error `report.reading_invalid`.
- **REQ-RPT.BALLOON.ENTRY-007** If the principal entering a reading is an agent, then the system shall refuse it with error `report.reading_by_agent_forbidden` [proposed reading of C5 and REQ-FID-021; to be confirmed].
- **REQ-RPT.BALLOON.ENTRY-008** When the user voids a reading, the system shall record the reason and exclude the reading from reports and statistics, while keeping the original journal entry.

## Interfaces

- **Offers:** `report.enter_reading@1` (W2, human only), `report.void_reading@1` (W2), `report.list_reading@1` (R0); type `ManualReading`.
- **Consumes:** gauge queries (`RPT.BALLOON.GAUGE`), characteristic and result creation (`GDT.CHAR`), decision rules (`UNC.DEC`), principal checks (`CORE.BUS`).

## Decomposition hints

PARSE (decimal and unit parsing), CHECK (gauge, calibration and resolution checks), RECORD (ResultRecord creation and voiding), MULTI (multi-part entry grid).

## Open questions

- Resolved: CONSTITUTION §3 (D-031) now makes manual readings `human_only`. The children declare `report.enter_reading@1`, `report.void_reading@1` and `report.enter_series_reading@1` with `human_only: true`, so the bus refuses agent and script principals and MCP projects them as request-only; REQ-RPT.BALLOON.ENTRY-007 is met by that bus refusal.
- Gauge records are owned by `UNC.INST.LIB` (agreed at review); `RPT.BALLOON.GAUGE` only references them.
- **Cross-domain:** `GDT.CHAR.RECORD` needs a `DIRECT_READING` variant of its DMT-row and duplicate-result rules (raised in `RPT.BALLOON.ENTRY.RECORD`).

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| RPT.BALLOON.ENTRY.RECORD | Single manual reading entry, voiding and listing (leaf) | 001 002 003 004 005 006 007 008 | none |
| RPT.BALLOON.ENTRY.SERIES | Multi-part reading series entry (leaf) | 001 007 | none |
