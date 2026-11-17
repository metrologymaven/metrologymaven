---
id: IND.GAPFL.SENSOR
title: Gap and flush from profile sensors
level: 3
parent: IND.GAPFL
kind: module
status: open
phase: P3
size: L
owners: []
claimed_by:
satisfies: [REQ-IND.GAPFL-005]
covers: [GOM.SPE.04, PW.IND.02]
depends_on: [IND.GAPFL.SECT, HAL]
cli: [fid pack evaluate-gapflush-reading]
mcp: [pack_evaluate_gapflush_reading]
journal: [pack.evaluate_gapflush_reading@1]
updated: 2026-11-16
---

# IND.GAPFL.SENSOR: Gap and flush from profile sensors

## Purpose

Evaluates gap and flush from line-profile sensor (gap-gun style) readings, applying exactly the same definitions as scan sections, and records whether the value is the device's own reading or computed from its profile points.

## Scope

**In:**
- Accepting a HAL profile reading (profile points and, where the device reports it, its own gap and flush values)
- Computing gap and flush from profile points through the shared definition evaluation of `IND.GAPFL.SECT`
- Recording evaluation basis `POINTS` (computed from profile) or `DIRECT_READING` (device-reported value)
- `pack.evaluate_gapflush_reading@1` (R1)

**Out** (and which node owns it instead):
- Device drivers and acquisition (owned by `HAL`)
- Edge detection and definition evaluation logic (owned by `IND.GAPFL.SECT`)

## Requirements

- **REQ-IND.GAPFL.SENSOR-001** When evaluated from a profile-sensor reading with profile points, the pack shall compute gap and flush with the same definition evaluation as for scan sections and record evaluation basis `POINTS`.
- **REQ-IND.GAPFL.SENSOR-002** When the device reports its own gap and flush values, the pack shall store them as a separate result with evaluation basis `DIRECT_READING` and shall not mix them with computed values.
- **REQ-IND.GAPFL.SENSOR-003** The pack shall obtain readings only through HAL session commands and shall never issue device I/O itself.
- **REQ-IND.GAPFL.SENSOR-004** If the device's coordinate frame for the profile is not declared, then the pack shall refuse to compute from profile points with a typed error.

## Interfaces

- **Offers:** `pack.evaluate_gapflush_reading@1` (R1).
- **Consumes:** HAL profile device readings and simulator; `IND.GAPFL.SECT` definition evaluation.

## Decomposition hints

Likely one leaf once HAL's profile device class is specified.

## Open questions

- Profile-sensor devices are not yet catalogued in `seeds/devices.md` [unverified]; HAL may need a device row.

## Children

_None yet: status `open`._
