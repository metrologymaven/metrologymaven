---
id: CMM.EXEC.RECORD
title: Run records and temperature compensation inputs
level: 3
parent: CMM.EXEC
kind: module
status: open
phase: P2
size: L
owners: []
claimed_by:
satisfies: [REQ-CMM.EXEC-019, REQ-CMM.EXEC-020]
covers: []
depends_on: [HAL.XSCR, UNC, CMM.EXEC.DRIVE]
cli: [fid cmm record-temperature, fid cmm get-run, fid cmm list-runs]
mcp: [cmm_get_run, cmm_list_runs]
journal: [cmm.record_temperature@1, cmm.record_run@1]
updated: 2026-11-16
---

# CMM.EXEC.RECORD: Run records and temperature compensation inputs

## Purpose

Captures what a run was and under which conditions: temperature compensation inputs (part, machine and scale temperatures with source and timestamp, and material expansion coefficients), recorded before or during the run and passed to the controller or kernel; and the run record journaled when a run completes, aborts or faults, referencing the ticket, the I/O transcript digest, the final state and any validator or controller error.

## Scope

**In:**
- `TemperatureInputs` from Environment devices or manual entry, with source, timestamp and expansion coefficients
- Part-temperature window check and the out-of-window mark
- Hand-off of temperature inputs to the controller (when compensation is server-owned) or to the kernel
- `RunRecord` and its journaling at completion, abort or fault
- `cmm.record_temperature@1` (W2), `cmm.get_run@1` and `cmm.list_runs@1` (R0)

**Out** (and which node owns it instead):
- Thermal uncertainty contribution (owned by `UNC`)
- I/O transcripts (owned by `HAL.XSCR`)
- Run execution (owned by `CMM.EXEC.DRIVE`)

## Requirements

- **REQ-CMM.EXEC.RECORD-001** The system shall record part, machine and scale temperatures in kelvin with source (device ID or manual principal) and timestamp, and material expansion coefficients per part, in each run record.
- **REQ-CMM.EXEC.RECORD-002** If the part temperature at run start lies outside the configured window, then the system shall mark the run `TEMPERATURE_OUT_OF_WINDOW` in its record.
- **REQ-CMM.EXEC.RECORD-003** If a temperature is non-finite or below 0 K, then the system shall reject it with `TEMPERATURE_INVALID`.
- **REQ-CMM.EXEC.RECORD-004** When a run completes, aborts or faults, the system shall journal one run record referencing the ticket, the I/O transcript digest, the final state, the temperature inputs and any validator or controller error.
- **REQ-CMM.EXEC.RECORD-005** The system shall serve run records read-only over CLI and MCP (`cmm_get_run`, `cmm_list_runs`).

## Interfaces

- **Offers:** `cmm.record_temperature@1` (W2), `cmm.get_run@1`, `cmm.list_runs@1` (R0); RunRecord, TemperatureInputs types.
- **Consumes:** `HAL.XSCR` transcript digests; HAL Environment devices; `UNC` thermal input format; `CMM.EXEC.DRIVE` final state.
- **Journal:** `cmm.record_temperature@1`, `cmm.record_run@1`.

## Decomposition hints

THERM (temperature inputs and window), RUNREC (run record), QUERY (read-only queries).

## Open questions

- Whether temperature compensation is applied by the controller or by the kernel depends on the controller (`compensation_owner` in the ticket); both paths must be recorded.

## Children

_None yet: status `open`._
