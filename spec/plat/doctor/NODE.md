---
id: PLAT.DOCTOR
title: fid doctor environment diagnostics
level: 2
parent: PLAT
kind: subsystem
status: split
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
- **REQ-PLAT.DOCTOR-009** When the user opts in, the system shall send crash reports and user-initiated issue reports (optionally with a redacted journal excerpt and the doctor report) to the project's issue intake, and shall send nothing without explicit per-report consent.

## Interfaces

- **Offers:** `doctor.run_check@1` (R0; CLI `fid doctor run-check`, MCP `doctor_run_check`); `doctor.show_report@1` (R0; CLI `fid doctor show-report`, MCP `doctor_show_report`); the doctor report JSON schema.
- **Consumes:** `PLAT.MATRIX` matrix file; `PLAT.HW` profile file; `PLAT.GPU` detection result type and known-bad list; `PLAT.XHOST` SDK platform table (optional checks); AUTO generated CLI and MCP surfaces; CORE registry.

## Decomposition hints

PROBE (environment probes), LOOKUP (tier and profile verdicts), SCHEMA (report schema and rendering), CMD (commands and exit codes), PRIV (redaction for shareable reports).

## Open questions

- The bare `fid doctor` form is now allowed by CONSTITUTION §3.1 (D-007) when generated from the registry. `PLAT.DOCTOR.CMD` declares it with a `cli_bare` flag on `doctor.run_check@1`; AUTO's CLI generator must support that flag (cross-domain, not yet in AUTO's spec).
- Whether R0 doctor runs are journaled follows CORE; `PLAT.DOCTOR.CMD` names the events and makes its journal criterion conditional.
- Driver-host checks may cross a machine boundary: **D-023** (raised by the XHOST split) blocks part of `PLAT.DOCTOR.HOSTCHK`.

## Split rationale

The split follows the doctor's **pipeline**: PROBE observes the machine (the only non-deterministic step), LOOKUP decides (pure), REPORT formats and redacts (pure), CMD wires them into registry commands with exit codes and time limits, and HOSTCHK adds opt-in checks that depend on later subsystems. Keeping observation separate from decision lets every verdict be tested from JSON snapshots with no hardware. Four children are leaves now; HOSTCHK waits for DEPLOY and XHOST.

Alternatives considered:
- **Split by item** (OS / CPU / RAM / GPU). Rejected: each item needs the same probe, verdict and rendering plumbing; the pipeline split shares it.
- **Merge REPORT into CMD.** Rejected: redaction and the schema are reused by `show-report` and by bug-report tooling, and are easier to test as pure functions.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| PLAT.DOCTOR.PROBE | Environment probes | 001 006 | none |
| PLAT.DOCTOR.LOOKUP | Tier and hardware-profile verdicts | 001 002 005 | none |
| PLAT.DOCTOR.REPORT | Doctor report schema, text rendering and redaction | 003 008 | none |
| PLAT.DOCTOR.CMD | fid doctor run-check and show-report commands | 001 003 004 005 007 | none |
| PLAT.DOCTOR.HOSTCHK | Optional driver-host and install-integrity checks | 003 006 007 | none |
| PLAT.DOCTOR.FEEDBK | Opt-in crash and issue reporting | 009 | none |
