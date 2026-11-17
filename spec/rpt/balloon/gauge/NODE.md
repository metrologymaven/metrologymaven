---
id: RPT.BALLOON.GAUGE
title: Gauge register
level: 3
parent: RPT.BALLOON
kind: module
status: open
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-RPT.BALLOON-006, REQ-RPT.BALLOON-007, REQ-RPT.BALLOON-008]
covers: []
depends_on: [CORE.STO, UNC.INST]
cli: [fid report add-gauge, fid report update-gauge, fid report retire-gauge, fid report list-gauge]
mcp: [report_add_gauge, report_update_gauge, report_retire_gauge, report_list_gauge]
journal: [report.add_gauge@1, report.update_gauge@1, report.retire_gauge@1]
updated: 2026-11-16
---

# RPT.BALLOON.GAUGE: Gauge register

## Purpose

The register of hand gauges cited by manual readings: gauge ID, type, resolution, measuring range, calibration due date and an optional uncertainty model accepted by UNC. Manual-reading entry (`RPT.BALLOON.ENTRY`) checks readings against it. It is a register of instruments as assets, not a device driver.

## Scope

**In:**
- Gauge record: ID, type (caliper, micrometer, height gauge, pin, thread gauge, other), resolution with unit, range, calibration due date and certificate reference, status (active or retired)
- Optional link to an uncertainty model in `UNC.INST`
- Queries used by reading entry: resolution, due date, uncertainty model

**Out** (and which node owns it instead):
- Wired, wireless and keyboard-wedge gauge input (`HAL.DRV`)
- Uncertainty models (`UNC.INST`)
- Reading entry (`RPT.BALLOON.ENTRY`)

## Requirements

- **REQ-RPT.BALLOON.GAUGE-001** A gauge record shall hold a unique gauge ID, type, resolution with unit, measuring range, calibration due date and certificate reference.
- **REQ-RPT.BALLOON.GAUGE-002** Each change to a gauge record shall be a journaled W2 command, and a retired gauge shall stay in the register so past readings still resolve.
- **REQ-RPT.BALLOON.GAUGE-003** The register shall answer, for a gauge and a date, whether calibration was overdue on that date.
- **REQ-RPT.BALLOON.GAUGE-004** The register shall answer the maximum number of decimal places supported by a gauge's resolution in a given unit.
- **REQ-RPT.BALLOON.GAUGE-005** A gauge record may link an uncertainty model only if `UNC.INST` accepts it; otherwise the gauge's readings shall carry UEC-0.

## Interfaces

- **Offers:** `report.add_gauge@1`, `report.update_gauge@1`, `report.retire_gauge@1` (W2), `report.list_gauge@1` (R0); type `Gauge`; query functions for `RPT.BALLOON.ENTRY`.
- **Consumes:** storage (`CORE.STO`), uncertainty models (`UNC.INST`).

## Decomposition hints

Likely one leaf (register and commands) once the overlap below is settled.

## Open questions

- **Cross-domain overlap:** `HAL.DRV` journals gauge identity, resolution and calibration reference for driver-connected gauges. One gauge register should serve both hand entry and driver input; which domain owns it (RPT here, or HAL) needs agreement between RPT and HAL maintainers.
- The `*-gauge` commands widen the parent's original `*-balloon|*-reading` patterns; the parent patterns were extended.

## Children

_None yet: status `open`._
