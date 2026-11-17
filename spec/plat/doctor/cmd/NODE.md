---
id: PLAT.DOCTOR.CMD
title: fid doctor run-check and show-report commands
level: 3
parent: PLAT.DOCTOR
kind: feature
status: leaf
phase: P0
size: M
owners: []
claimed_by:
satisfies: [REQ-PLAT.DOCTOR-001, REQ-PLAT.DOCTOR-003, REQ-PLAT.DOCTOR-004, REQ-PLAT.DOCTOR-005, REQ-PLAT.DOCTOR-007]
covers: []
depends_on: [PLAT.DOCTOR.PROBE, PLAT.DOCTOR.LOOKUP, PLAT.DOCTOR.REPORT, CORE, AUTO]
side_effect: R0
determinism: N
cli: [fid doctor, fid doctor run-check, fid doctor show-report]
mcp: [doctor_run_check, doctor_show_report]
journal: [doctor.run_check@1, doctor.show_report@1]
updated: 2026-11-16
---

# PLAT.DOCTOR.CMD: `fid doctor` commands

## Purpose

Exposes the doctor to users, IT departments, scripts and agents as two registry commands: `doctor.run_check@1` (probe, assess, report) and `doctor.show_report@1` (render a report someone else produced). Both are R0, offline and bounded in time. The bare `fid doctor` form runs the check, as CONSTITUTION §3.1 allows for a single-verb command generated from the registry.

## Scope

**In:**
- Registry declarations of both commands, their parameters and results
- Orchestration: probe → assess → redact → render, under an overall deadline
- Exit codes from the assessment outcome
- Declaring the bare `fid doctor` form in the registry entry for `doctor.run_check@1`

**Out** (and which node owns it instead):
- Probes, verdicts and report format (`PLAT.DOCTOR.PROBE`, `PLAT.DOCTOR.LOOKUP`, `PLAT.DOCTOR.REPORT`)
- Generating the CLI and MCP surfaces (`AUTO`)
- Journaling mechanics (`CORE`)
- Optional driver-host and install checks (`PLAT.DOCTOR.HOSTCHK`)
- `fid doctor run-benchmark` (`PLAT.HW.CMD`)

## Requirements

- **REQ-PLAT.DOCTOR.CMD-001** When `fid doctor run-check` or bare `fid doctor` runs, the command shall probe the environment, assess it against the installed matrix and profiles for the build's phase, and print the report.
- **REQ-PLAT.DOCTOR.CMD-002** When run with `--json`, the command shall print exactly one report JSON document on stdout and nothing else on stdout.
- **REQ-PLAT.DOCTOR.CMD-003** Both commands shall be declared with side-effect class R0, shall write no file and shall make no network request.
- **REQ-PLAT.DOCTOR.CMD-004** The `run-check` command shall exit with the assessment outcome's code: 0 meets recommended, 1 meets minimum, 2 below minimum, 3 unsupported platform, 4 indeterminate; usage errors shall exit 64 and internal errors 70.
- **REQ-PLAT.DOCTOR.CMD-005** The `run-check` command shall return within 10 seconds on the minimum hardware profile, marking any probe still running at an 8-second overall deadline as `unknown(timeout)` [unverified: target to be measured].
- **REQ-PLAT.DOCTOR.CMD-006** When `--include-identifying` is passed on the CLI, the report shall not be redacted; the MCP projection shall not offer this parameter.
- **REQ-PLAT.DOCTOR.CMD-007** When `fid doctor show-report` is given a report document (CLI: a file argument or `-` for stdin; MCP: the document as a JSON parameter), the command shall validate it and print its text rendering, or with `--json` the normalised JSON.
- **REQ-PLAT.DOCTOR.CMD-008** If the matrix or profile file shipped with the install is missing or invalid, then `run-check` shall still produce a report with the affected verdicts `indeterminate` and the reason, and exit 4.

## Acceptance criteria

- **AC-1** Given a `FakeEnv` loaded from `fixtures/plat/doctor/env-win11-discrete.json` and the fixture matrix and profiles, when `doctor.run_check@1` runs with `json = true` through the command bus, then stdout equals `fixtures/plat/doctor/report-win11.json` and the exit code is 0.
- **AC-2** Given `env-ubuntu-vm-software.json`, when run, then the exit code is 2; given `env-freebsd.json`, 3; given `env-unknown-ram.json`, 4.
- **AC-3** Given a network-deny test hook and a file-system write monitor on the test process, when `run-check` runs against the real environment on a Tier 1 CI runner, then zero connection attempts and zero file writes are observed, and the command returns in under 10 s.
- **AC-4** Given a `FakeEnv` whose GPU probe sleeps 20 s, when `run-check` runs, then it returns within 10 s with GPU `unknown(timeout)`.
- **AC-5** Given the generated surfaces, when the CLI help and the MCP tool list are inspected, then `fid doctor`, `fid doctor run-check`, `fid doctor show-report`, `doctor_run_check` and `doctor_show_report` exist; `doctor_run_check` has no `include_identifying` parameter.
- **AC-6** Given `fixtures/plat/doctor/report-win11.json` piped to `fid doctor show-report -`, when run, then stdout equals `fixtures/plat/doctor/report-win11.txt`; given `report-schema99.json`, it exits 64 with `UnsupportedSchema`.
- **AC-7** Given an install with the matrix file removed, when `run-check` runs, then the report lists tier `unknown(no_matrix)` and exit code is 4.
- **AC-8** Given a completed `run-check`, when the session journal is read, then it holds one `doctor.run_check@1` event whose result digest equals the SHA-256 of the report JSON, if CORE journals R0 commands.

## Design

- **Registry command(s):**
  - `doctor.run_check@1`: R0; params `include_identifying: bool` (CLI only; `mcp: false` on the parameter), `json: bool` (output mode, from AUTO's common `--json`); result `DoctorReport`; `cli_bare: true` so the generator also emits `fid doctor` (CONSTITUTION §3.1).
  - `doctor.show_report@1`: R0; param `report: DoctorReportDocument` (CLI: path or `-`; MCP: inline JSON object, never a path); result rendered text or normalised JSON.
- **Data structures:** no new types beyond `fid-doctor`'s; `Outcome::exit_code()` from LOOKUP.
- **Algorithm:** run PROBE with a 2 s per-probe and 8 s overall deadline; load installed `support-matrix.toml` and `hw-profiles.toml` from the install directory (errors become `indeterminate` reasons); `assess`; `redact` unless `include_identifying`; render; return. Phase is a build-time constant.
- **Errors:** `UsageError` (64), `Internal` (70); everything else is reported inside the report.
- **Determinism:** N for `run_check` (observes the machine; D0 given a `FakeEnv`); D0 for `show_report`.
- **Journal:** one event per command execution named by the command ID, with the result digest, if CORE's policy journals R0 commands.
- **Performance budget:** < 10 s on the minimum profile [unverified].
- **Crate:** `fid-doctor` (module `cmd`), registering with the CORE registry.

## Test fixtures

All fixtures from `PLAT.DOCTOR.PROBE`, `LOOKUP` and `REPORT` under `fixtures/plat/doctor/`, plus `fixtures/plat/matrix/valid.toml` and `fixtures/plat/hw/profiles-valid.toml`.

## Build prompt

> In crate `fid-doctor`, add module `cmd` registering two registry commands with the CORE registry (CLI and MCP are generated by AUTO; do not hand-write either):
> - `doctor.run_check@1` (R0, `cli_bare: true` so `fid doctor` also runs it, as CONSTITUTION §3.1 allows): probe (`fid-doctor::probe`, 2 s per probe, 8 s overall), load the installed `platform/support-matrix.toml` and `platform/hw-profiles.toml` (missing or invalid → `indeterminate` with reason, exit 4), assess (`lookup`), redact unless `--include-identifying` (CLI-only parameter, not projected to MCP), render text or `--json`. Exit codes: 0 meets recommended, 1 meets minimum, 2 below minimum, 3 unsupported platform, 4 indeterminate, 64 usage, 70 internal.
> - `doctor.show_report@1` (R0): take a report document (CLI path or `-`; MCP inline JSON), parse with `report::parse`, print text or normalised JSON.
>
> Both make no network request and write no file (CONSTITUTION C7, §3 R0). Acceptance tests: the eight criteria above with fixtures in `fixtures/plat/doctor/`. Constraints: CONSTITUTION §8 (no panic, no `unwrap`, no `unsafe`), C6 (rendered report strings are data). If AUTO's generator does not yet support `cli_bare`, ship `run-check` and open an issue against AUTO. Commit: `feat(fid-doctor): fid doctor commands`.

## Open questions

- Bare `fid doctor` needs AUTO's CLI generator to honour a registry-declared single-verb form (CONSTITUTION §3.1); not yet in AUTO's spec.
- Whether R0 runs are journaled follows CORE; AC-8 is conditional on that rule.

## Children

_Leaf: none._
