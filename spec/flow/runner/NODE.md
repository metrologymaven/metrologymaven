---
id: FLOW.RUNNER
title: Golden-journal acceptance runner, pass rules and GA gates
level: 2
parent: FLOW
kind: subsystem
status: split
phase: P0
size: XL
owners: []
claimed_by:
satisfies: [REQ-FLOW-002, REQ-FLOW-003, REQ-FLOW-004]
covers: [JOB.USE.04]
depends_on: [CORE.RPL, CORE.JRN, CORE.BUS, ECO.DATA, AUTO.CTRL, VIZ.SHELL]
uses_artifacts: [FLOW.STUDY, ECO.CI, PLAT.HW, PLAT.DOCTOR, VIZ.CAPTURE, UNC.ORS]
cli: [fid accept run-suite, fid accept show-report, fid accept evaluate-gate]
mcp: [accept_run_suite, accept_show_report, accept_evaluate_gate]
journal: [accept.*]
updated: 2026-11-17
---

# FLOW.RUNNER: Golden-journal acceptance runner, pass rules and GA gates

## Purpose

Owns the machinery that turns the job families' golden journals into a nightly verdict: the golden-journal package format (journal, datasets by digest, declared targets, gating flag, variant kind), the runner that replays every job, captures correctness and machine time, and the pass-rule engine that combines nightly results with design-partner timing to give each app a GA gate status for release. It also owns the reliability bar: crash-free sessions and zero committed-command loss, measured by forced-kill scenarios in the suite and by opt-in telemetry.

## Scope

**In:**
- The acceptance-job package format: job ID, app, gating or tracked, unassisted or assisted, golden journal, dataset digests, quality checks, time targets
- The nightly runner: replay, correctness checks, machine-time capture, result report
- Unassisted enforcement: a gating run fails if any agent principal acts in it
- Pass rules: per-job machine verdict, and the gate rule combining study medians
- Per-app GA gate status consumed by the release process
- Reliability bar `JOB.USE.04`: forced-kill checks and the crash-free-session rate from opt-in telemetry
- Trend history of nightly results

**Out** (and which node owns it instead):
- The jobs, their maps, data and targets (owned by `FLOW.INSPECT`, `FLOW.REPORT`, `FLOW.PROBE`, `FLOW.NETWORK`, `FLOW.SETUP`)
- Design-partner timing sessions and their data (owned by `FLOW.STUDY`)
- The replay engine (owned by `CORE.RPL`) and CI infrastructure (owned by `ECO.CI`)
- Reference machines (owned by `PLAT.HW`; hosting is D-014)
- Blocking a release on the gate (owned by `ECO.REL`)
- Telemetry transport and consent (owned by `PLAT` and `ENT` per their scopes; FLOW only consumes opt-in aggregates)

## Requirements

- **REQ-FLOW.RUNNER-001** The acceptance-job package format shall declare job ID, owning app, gating or tracked, unassisted or assisted, golden journal digest, dataset digests, quality checks and time targets, and the runner shall reject a package that omits any of them.
- **REQ-FLOW.RUNNER-002** When the nightly CI run starts, the runner shall replay every registered acceptance-job package on each reference machine available to CI and record, per job, the replay outcome, each quality check's result and the machine time.
- **REQ-FLOW.RUNNER-003** If a dataset's digest does not match the package, then the runner shall mark that job NOT_REPLAYABLE and not report a pass.
- **REQ-FLOW.RUNNER-004** If a package declared unassisted contains any command by an agent principal, then the runner shall fail that job regardless of its other results.
- **REQ-FLOW.RUNNER-005** The runner shall exclude assisted and tracked packages from every GA gate while still reporting their results.
- **REQ-FLOW.RUNNER-006** When evaluating a GA gate for an app, the system shall pass a gating job only if its latest nightly run passes every quality check and the design-partner median task time across at least 3 partners meets the target with no partner above 1.5 times the target.
- **REQ-FLOW.RUNNER-007** The system shall report an app's GA gate as failed while any gating job of that app fails, lacks a nightly run newer than 7 days, or lacks study data from at least 3 partners.
- **REQ-FLOW.RUNNER-008** The nightly suite shall include a forced process kill after a committed command in at least one job per app, and shall fail if any committed command is missing from the journal after restart.
- **REQ-FLOW.RUNNER-009** Where users have opted in to crash telemetry, the runner report shall state the crash-free session rate per app against the > 99.5 % bar, and shall state "no data" rather than a rate when there are none.
- **REQ-FLOW.RUNNER-010** Each nightly report shall be a journaled, digest-identified artefact that lists every job, its verdict and the reason for any failure, and shall be published with the CI results.
- **REQ-FLOW.RUNNER-011** Runner and gate commands shall be R0 or R1, and no runner command shall change a gating flag or a target; those change only by editing the job package under review.

## Interfaces

- **Offers:** the package format; `accept.run_suite@1` (R1), `accept.show_report@1` (R0), `accept.evaluate_gate@1` (R1); the per-app GA gate status for `ECO.REL`; the nightly report.
- **Consumes:** replay from `CORE.RPL`; journal and principal attribution from `CORE.JRN` and `CORE.BUS`; datasets from `ECO.DATA`; CI from `ECO.CI`; reference machines from `PLAT.HW`; presentation-track timing from `VIZ.CAPTURE`; partner timing records from `FLOW.STUDY`.

## Decomposition hints

PKG (package format and validation), NIGHTLY (runner and report), UNASSIST (agent-principal check), GATE (pass rules and per-app gate), RELIAB (forced-kill checks and crash-free rate), TREND (history).

## Open questions

- The parent names `fid accept run`; CONSTITUTION §3 gives verb-object CLI names, so this node uses `fid accept run-suite`.
- Reference-machine hosting follows D-014 (accepted: a cheap minimum-profile machine plus a sponsored recommended-profile machine, hosts published with results); until they exist, nightly times come from hosted runners and are labelled as such.
- Per-app gates follow D-017 (accepted, option A): an app's 1.0 needs every phase of its core workflow; the gate is evaluated per app and per milestone, and packages list the milestones they gate.
- REQ-010's "journaled" report is read under D-031: `accept.run_suite@1` is R1, so it writes an Activity Log entry with the report digest inside a recording session, not a Record Journal entry.

## Split rationale

The dividing principle is **format, checks, orchestration, reporting and gate**. The package format (PKG) is the contract every job family writes to. Checks split by what they read: stored values through R0 commands (CHKVAL), the UI and presentation track (CHKUI), principal attribution (UNASSIST) and crash survival (RELIAB, which also owns the `JOB.USE.04` crash-free rate). NIGHTLY orchestrates them into the report defined by VERDICT, and GATE combines reports with partner study aggregates. Study records are read as an artifact, so RUNNER no longer has a code dependency on STUDY.

Alternatives considered:
- **One checks child.** Rejected: UI checks need the control channel and accessibility tree (P1a), value checks need only the bus (P0); separating them keeps P0 work unblocked.
- **Trend history as its own child.** Rejected: it is an append-only file with one read command and fits VERDICT.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| [FLOW.RUNNER.PKG](pkg/NODE.md) | Acceptance-job package format and validator | 001 011 | |
| [FLOW.RUNNER.UNASSIST](unassist/NODE.md) | Unassisted-run principal check | 004 | |
| [FLOW.RUNNER.RELIAB](reliab/NODE.md) | Forced-kill harness and crash-free session rate | 008 009 | JOB.USE.04 |
| [FLOW.RUNNER.CHKVAL](chkval/NODE.md) | Value and record quality-check evaluators | 002 | |
| [FLOW.RUNNER.CHKUI](chkui/NODE.md) | UI and presentation-track quality-check evaluators | 002 | |
| [FLOW.RUNNER.VERDICT](verdict/NODE.md) | Nightly acceptance report, show command and trend history | 005 010 | |
| [FLOW.RUNNER.NIGHTLY](nightly/NODE.md) | Nightly suite runner command | 002 003 004 010 | |
| [FLOW.RUNNER.GATE](gate/NODE.md) | Pass rules and per-app GA gate evaluation | 005 006 007 011 | |
