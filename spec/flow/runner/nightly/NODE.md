---
id: FLOW.RUNNER.NIGHTLY
title: Nightly suite runner command
level: 3
parent: FLOW.RUNNER
kind: feature
status: leaf
phase: P0
size: M
owners: []
claimed_by:
satisfies: [REQ-FLOW.RUNNER-002, REQ-FLOW.RUNNER-003, REQ-FLOW.RUNNER-004, REQ-FLOW.RUNNER-010]
covers: []
depends_on: [FLOW.RUNNER.PKG, FLOW.RUNNER.UNASSIST, FLOW.RUNNER.RELIAB, FLOW.RUNNER.CHKVAL, FLOW.RUNNER.CHKUI, FLOW.RUNNER.VERDICT, CORE.RPL.EXEC, CORE.RPL.CLASS, ECO.DATA.FETCH]
uses_artifacts: [ECO.CI.WF, PLAT.HW.REFMACH]
side_effect: R1
determinism: D1
cli: [fid accept run-suite]
mcp: [accept_run_suite]
journal: [accept.run_suite@1]
updated: 2026-11-17
---

# FLOW.RUNNER.NIGHTLY: Nightly suite runner command

## Purpose

`fid accept run-suite`: replays every registered package (or a filtered set), verifies dataset digests, runs the unassisted check, forced-kill harness and quality checks, measures machine time, and emits the nightly report. CI runs it every night on each available reference machine and publishes the report with the CI results.

## Scope

**In:**
- `accept.run_suite@1` (R1) orchestration
- Dataset fetch by digest and digest verification
- Exact replay of each journal in a scratch project and replay-outcome capture
- Machine-time capture per package (wall clock from first to last replayed entry)
- Assembly of the `acceptance-report@1` document
- The nightly CI job definition that calls it on each reference machine

**Out:**
- Package format, checks, reliability and report format (owned by sibling leaves)
- Replay engine (owned by `CORE.RPL`)
- Reference machines (owned by `PLAT.HW.REFMACH`; hosting per D-014)

## Requirements

- **REQ-FLOW.RUNNER.NIGHTLY-001** When `fid accept run-suite` runs, the system shall validate every selected package, fetch its datasets by digest, replay its journal in exact mode on a scratch copy, and record the replay outcome, each check's outcome and the machine time.
- **REQ-FLOW.RUNNER.NIGHTLY-002** If a dataset's or journal's digest does not match the package, then the runner shall mark that package `not_replayable`, skip its checks and report no pass for it.
- **REQ-FLOW.RUNNER.NIGHTLY-003** If a package is declared unassisted and the unassisted check reports any violation, then the runner shall set the package verdict to `fail` regardless of other outcomes.
- **REQ-FLOW.RUNNER.NIGHTLY-004** A package verdict shall be `pass` only if the replay outcome is `IDENTICAL` or `EQUIVALENT` and every declared check returns `pass`.
- **REQ-FLOW.RUNNER.NIGHTLY-005** The runner shall emit one `acceptance-report@1` per run containing every selected package, and the nightly CI job shall run it on each reference machine available to CI and publish the reports with the CI results.
- **REQ-FLOW.RUNNER.NIGHTLY-006** The command shall run in a recording session so that the command and its report digest are written to the Activity Log (D-031), and shall change no package, target or gating flag.

## Acceptance criteria

- **AC-1** Given fixture suite `fixtures/flow/nightly/mini/` (3 packages: pass, check-fail, assisted-tracked), when `fid accept run-suite --json` runs, then verdicts are pass, fail and pass with `non_gating` on the third, and the report validates.
- **AC-2** (error path) Given a package whose dataset digest is altered, then that package is `not_replayable` and has no check outcomes.
- **AC-3** (error path) Given an unassisted package with an agent entry, then its verdict is `fail` with reason `agent_in_unassisted`.
- **AC-4** Given the same suite run twice on one machine, then verdicts and check outcomes are identical (machine times may differ).

## Design

- **Registry command(s):** `accept.run_suite@1`, R1; params `job?: JobId`, `variant?`, `machine_label?`. Result: the report and its digest. It reads the suite index embedded in or installed with the build; no raw paths.
- **Data structures:** uses `JobPackage`, `CheckOutcome`, `AcceptanceReport`.
- **Algorithm:** for each package in suite-index order: validate; fetch and verify datasets; create a scratch project; if `kill_after`, run under the RELIAB harness; else exact replay through `CORE.RPL.EXEC`; classify with `CORE.RPL.CLASS`; run UNASSIST; run checks; time with a monotonic clock; assemble report.
- **Errors:** `RUN-SUITE-INDEX` (index invalid: command fails); per-package errors become verdict reasons.
- **Determinism:** D1 (verdicts are deterministic; machine time is observed and per platform).
- **Performance budget:** launch-1 suite under 2 hours on the minimum-profile reference machine [unverified].
- **Crate:** `fid-accept`; CI job in the reusable workflow set.

## Test fixtures

Mini suite under `fixtures/flow/nightly/` with synthetic journals and data.

## Build prompt

> In crate `fid-accept`, add `accept.run_suite@1` (R1; CLI `fid accept run-suite [--job] [--variant] [--machine-label]`; MCP `accept_run_suite`). For each package in the suite index: validate (package module), fetch datasets by digest (dataset fetch library), mark `not_replayable` on any digest mismatch, replay in exact mode on a scratch project (or through the forced-kill harness when `kill_after` is set), classify the outcome, run the unassisted check (fail on any violation), run value and UI checks, time with a monotonic clock, and assemble an `acceptance-report@1`. Verdict pass only if outcome IDENTICAL or EQUIVALENT and all checks pass. Run inside a recording session so the Activity Log holds the report digest. Add a nightly CI job that runs it on each available reference machine and uploads the reports. Tests AC-1..AC-4 of spec node FLOW.RUNNER.NIGHTLY. No panic, no `unwrap`; never modify packages.

## Open questions

- REQ-FLOW.RUNNER-010 says the report is "journaled"; with R1 commands this is read as the Activity Log entry required by D-031 plus the report digest, not a Record Journal entry.
- Until D-014's reference machines exist, times come from hosted runners and are labelled with `machine_label` accordingly.

## Children

_Leaf: none._
