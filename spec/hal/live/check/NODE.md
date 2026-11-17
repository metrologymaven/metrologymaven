---
id: HAL.LIVE.CHECK
title: Instrument field checks and health checks
level: 3
parent: HAL.LIVE
kind: module
status: open
phase: P3
size: L
owners: []
claimed_by:
satisfies: [REQ-HAL.LIVE-006]
covers: [SA.INS.07]
depends_on: [HAL.LIVE.PROBE, HAL.SIM.PORT, KRN]
cli: [fid device run-field-check, fid device show-field-check]
mcp: [device_run_field_check, device_show_field_check]
journal: [device.run_field_check@1, device.show_field_check@1]
updated: 2026-11-16
---

# HAL.LIVE.CHECK: Instrument field checks and health checks

## Purpose

Recordable field and health checks for portable instruments, such as a tracker two-face check or an arm single-point articulation check: a check procedure guides the measurements, the kernel evaluates them, and the result is recorded as a field check with pass or fail against configured limits. Checks are never presented as acceptance or verification tests.

## Scope

**In:**
- Check procedure definitions (data): ID, version, device class, steps, evaluation operator, limits source
- `device.run_field_check@1` (D1) and `device.show_field_check@1` (R0)
- Kernel evaluation of check measurements and pass or fail against limits
- Labelling every result as a field check
- Linking the latest check to the device's calibration-state references (interim check)

**Out** (and which node owns it instead):
- ISO 10360 acceptance and reverification tests (not offered)
- CMM artefact checks (owned by `HAL.SIM.CONF` for conformance and `CMM` for machine checks)
- Calibration records (owned by `TRUST` and device calibration state)

## Requirements

- **REQ-HAL.LIVE.CHECK-001** When a field check runs, the system shall record the measured values, the procedure ID and version, the limits used and a pass or fail result.
- **REQ-HAL.LIVE.CHECK-002** Every field-check result shall be labelled as a field check, and no output, report or command name shall present it as an acceptance or verification test.
- **REQ-HAL.LIVE.CHECK-003** Check evaluation shall be done by a kernel operator, so the result is a deterministic function of the recorded measurements.
- **REQ-HAL.LIVE.CHECK-004** A passed field check shall be referenced as the device's latest interim check in its calibration-state references.

## Interfaces

- **Offers:** check procedures; the commands; interim-check references for stamps.
- **Consumes:** `HAL.LIVE.PROBE`; `KRN` evaluation operators; `HAL.SIM.PORT` for tests.

## Decomposition hints

PROC (procedure format), TWOFACE (tracker two-face check), SPAT (arm single-point articulation check), RESULT (recording and labelling; likely a leaf).

## Open questions

- Procedures may follow ISO 10360 interim-check ideas; symbols and procedures must be confirmed by a domain expert [unverified], and no standards text may be stored.

## Children

_None yet: status `open`._
