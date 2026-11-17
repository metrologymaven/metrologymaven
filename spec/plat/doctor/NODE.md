---
id: PLAT.DOCTOR
title: fid doctor environment diagnostics
level: 2
parent: PLAT
kind: subsystem
status: open
phase: P0
size: L
owners: []
claimed_by:
satisfies: [REQ-PLAT-007]
covers: []
depends_on: [PLAT.MATRIX, PLAT.GPU, PLAT.HW, CORE, AUTO]
cli: [fid doctor run-check, fid doctor show-report]
mcp: [doctor_run_check, doctor_show_report]
journal: []
updated: 2026-11-16
---

# PLAT.DOCTOR: Environment diagnostics

## Purpose

`fid doctor` is the self-check that tells a user, an IT department or an agent whether a machine can run the platform. It gathers OS, architecture, GPU backend, driver versions, memory and storage, looks them up in the support matrix and hardware profiles, and reports the tier and whether the minimum profile is met, in human-readable and JSON form. It is read-only and works offline.

## Scope

**In:**
- Environment probes: OS and version, architecture, CPU, RAM, free storage, display, GPU adapter, backend and driver version, fallback state
- Lookup of the probed platform in the support matrix (tier) and the hardware profiles (minimum and recommended verdicts)
- Optional checks for configured driver hosts and offline-install integrity
- The report schema and its human-readable rendering
- `fid doctor run-check` and `fid doctor show-report` commands and their MCP projections
- Exit codes that let scripts gate on the result

**Out** (and which node owns it instead):
- The support matrix data (`PLAT.MATRIX`)
- Backend selection policy and known-bad drivers (`PLAT.GPU`)
- Hardware profiles and the benchmark command (`PLAT.HW`)
- CLI and MCP generation (`AUTO`)
- Driver host protocol health beyond reachability (`HAL`)

## Requirements

- **REQ-PLAT.DOCTOR-001** When a user runs `fid doctor run-check`, the command shall report OS and version, architecture, GPU adapter, GPU backend, GPU driver version, total and available memory, and the support tier of the platform.
- **REQ-PLAT.DOCTOR-002** The report shall state for each hardware profile whether the machine meets it, and name each item that falls short.
- **REQ-PLAT.DOCTOR-003** When run with `--json`, the command shall emit the report in a versioned JSON schema, and without it shall emit the same content as human-readable text.
- **REQ-PLAT.DOCTOR-004** The doctor commands shall be side-effect class R0 and shall make no network request.
- **REQ-PLAT.DOCTOR-005** The command shall exit with distinct codes for meets-recommended, meets-minimum, below-minimum and unsupported-platform.
- **REQ-PLAT.DOCTOR-006** If a probe fails, then the report shall mark that item as unknown with the reason and the command shall still complete.
- **REQ-PLAT.DOCTOR-007** The command shall complete within 10 seconds on the minimum hardware profile, excluding optional driver-host checks [unverified: target to be measured].
- **REQ-PLAT.DOCTOR-008** The report shall contain no user names, host names or file paths outside the install directory unless the user passes an explicit flag, so it can be shared in a bug report.

## Interfaces

- **Offers:** `doctor.run_check@1` (R0; CLI `fid doctor run-check`, MCP `doctor_run_check`); `doctor.show_report@1` (R0; CLI `fid doctor show-report`, MCP `doctor_show_report`); the doctor report JSON schema.
- **Consumes:** `PLAT.MATRIX` matrix file; `PLAT.HW` profile file; `PLAT.GPU` detection result type and known-bad list; `PLAT.XHOST` SDK platform table (optional checks); AUTO generated CLI and MCP surfaces; CORE registry.

## Decomposition hints

PROBE (environment probes), LOOKUP (tier and profile verdicts), SCHEMA (report schema and rendering), CMD (commands and exit codes), PRIV (redaction for shareable reports).

## Open questions

- `fid doctor` does not follow the `fid <domain> <verb-object>` form if called bare; the parent pattern `fid doctor *` is kept and a bare `fid doctor` alias would be an AUTO question. No hand-written aliases are allowed (CONSTITUTION §3), so the bare form may need a decision.
- Whether R0 doctor runs are journaled follows CORE; left empty here.

## Children

_None yet: status `open`._
