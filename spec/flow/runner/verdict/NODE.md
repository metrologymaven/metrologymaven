---
id: FLOW.RUNNER.VERDICT
title: Nightly acceptance report, show command and trend history
level: 3
parent: FLOW.RUNNER
kind: feature
status: leaf
phase: P0
size: S
owners: []
claimed_by:
satisfies: [REQ-FLOW.RUNNER-005, REQ-FLOW.RUNNER-010]
covers: []
depends_on: [FLOW.RUNNER.PKG, CORE.JRN.CANON]
uses_artifacts: []
side_effect: R0
determinism: D0
cli: [fid accept show-report]
mcp: [accept_show_report]
journal: [accept.show_report@1]
built_in: [fid-accept]
built_pr: 440
updated: 2026-11-22
---

# FLOW.RUNNER.VERDICT: Nightly acceptance report, show command and trend history

## Purpose

Defines the nightly report `acceptance-report@1`: one digest-identified document per run listing every package, its verdict and the reason for each failure, with machine times per reference machine and the reliability section. It also keeps the trend history (one line per package per night) and provides the read command to show a report or a trend.

## Scope

**In:**
- `acceptance-report@1` format and its digest
- Trend history file (append-only, one record per package per run)
- `accept.show_report@1` (R0)

**Out:**
- Producing the report (owned by `FLOW.RUNNER.NIGHTLY`)
- Gate evaluation (owned by `FLOW.RUNNER.GATE`)
- Publishing with CI results (CI workflow in `ECO.CI.WF`)

## Requirements

- **REQ-FLOW.RUNNER.VERDICT-001** A report shall hold build ID, manifest digest, suite digest, run start and end times, reference machine IDs with their host, and per package: job, variant, gating flag, assistance, replay outcome, each check's outcome, machine time per machine, and a verdict of `pass`, `fail` or `not_replayable` with failure reasons.
- **REQ-FLOW.RUNNER.VERDICT-002** The report shall mark assisted and tracked packages as `non_gating` in a field separate from the verdict, so that their results are reported but distinguishable.
- **REQ-FLOW.RUNNER.VERDICT-003** The report digest shall be the SHA-256 of its RFC 8785 form, and the report shall contain no user names, host paths or partner data.
- **REQ-FLOW.RUNNER.VERDICT-004** When `fid accept show-report` runs with a report digest, the system shall return that report, and with `--trend <job>` and `--last N` (N at most 365) the last N trend records of that job.

## Acceptance criteria

- **AC-1** Given fixture report `fixtures/flow/verdict/r1.json`, when re-serialised, then bytes and digest are unchanged.
- **AC-2** Given a trend file with 40 nights, when `fid accept show-report --trend JOB.ACC.02 --last 30 --json` runs, then 30 records are returned newest first.
- **AC-3** (error path) Given an unknown digest, then the command fails with `not_found`.
- **AC-4** (error path) Given a report fixture containing a host path, then validation fails with `VERDICT-PII`.

## Design

- **Registry command(s):** `accept.show_report@1`, R0; params `digest?`, `trend?: job_id`, `last?: u16`. Reports and trend are read from the project's acceptance store directory, not from caller paths.
- **Data structures:** `AcceptanceReport`, `PackageResult`, `TrendRecord { night, job, variant, verdict, machine_s }`.
- **Algorithm:** canonical serialisation; PII lint (path-like strings, `@` addresses) before accepting a report.
- **Errors:** `not_found`, `VERDICT-SCHEMA`, `VERDICT-PII`.
- **Determinism:** D0.
- **Performance budget:** show under 100 ms [unverified].
- **Crate:** `fid-accept` module `report`.

## Test fixtures

Synthetic reports and trend files under `fixtures/flow/verdict/`.

## Build prompt

> In crate `fid-accept`, add module `report` with `acceptance-report@1` (build ID, manifest digest, suite digest, times, reference machines with host, per-package job, variant, gating, assistance, non_gating flag, replay outcome, check outcomes, machine time per machine, verdict pass|fail|not_replayable, reasons, reliability section) and an append-only trend file. Digest = SHA-256 of RFC 8785 form; reject reports with path-like or address-like strings (`VERDICT-PII`). Command `accept.show_report@1` (R0; CLI `fid accept show-report [--digest] [--trend <job> --last N]`; MCP `accept_show_report`). Tests AC-1..AC-4 of spec node FLOW.RUNNER.VERDICT. No panic, no `unwrap`.

## Build notes (2026-11-23)

Built in `fid-accept` module `report` (PR #440). Decisions and deviations from the Design above:

- **Store location.** The store is never a caller path. `fid-accept show-report` refuses `--store` and resolves the project's acceptance store itself: `<project>/acceptance/store`, where `<project>` is the current directory or its nearest ancestor holding `acceptance/suite.json` (the registered suite of FLOW.RUNNER.PKG). The store directory must be a real directory, not a link; with no project or no store the command fails `not_found`. The MCP params (`digest`, `trend`, `last`) carry no path. The library type `AcceptanceStore::new(root)` stays for the producer (`FLOW.RUNNER.NIGHTLY`) and tests.
- **TrendRecord carries the report digest (deviation).** `TrendRecord { night, job, variant, verdict, machine_s, report }`: `report` is the digest of the stored report the record was derived from. `night` is the UTC date of the report's `started_at`; `machine_s` is the package's largest machine time over the reference machines. Appending refuses a record whose report is not stored or does not hold the same (job, variant, verdict, machine_s, night), and `show-report --trend` checks every returned record the same way, so an edited record fails `VERDICT-SCHEMA`.
- **Append-only, checked on append and on load.** A record whose night is earlier than the record before it, or a second record for the same (night, job, variant), fails `VERDICT-SCHEMA`.
- **Verdict evidence (REQ-001).** `pass` requires a replay outcome of `identical` or `equivalent`, at least one check with every check `pass`, and machine times for exactly the declared machines. `not_replayable` requires the replay outcome `not_replayable`, and that replay outcome requires the `not_replayable` verdict. Anything else fails `VERDICT-SCHEMA`.
- **Timestamps** are compared as parsed time values (fractional seconds included), not as strings.
- **Schemas.** Machine times in the result schema are quantity kind `time` in `s`. Tests validate real `show_report` output (report and trend) against `result_schema` and inputs against `params_schema` with the bus validator.

## Open questions

- None.

## Children

_Leaf: none._
