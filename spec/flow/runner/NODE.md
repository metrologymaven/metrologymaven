---
id: FLOW.RUNNER
title: Golden-journal acceptance runner, pass rules and GA gates
level: 2
parent: FLOW
kind: subsystem
status: open
phase: P0
size: XL
owners: []
claimed_by:
satisfies: [REQ-FLOW-002, REQ-FLOW-003, REQ-FLOW-004]
covers: [JOB.USE.04]
depends_on: [FLOW.STUDY, CORE.RPL, CORE.JRN, CORE.BUS, ECO.DATA, ECO.CI, PLAT.HW, VIZ.CAPTURE]
cli: [fid accept run-suite, fid accept show-report, fid accept evaluate-gate]
mcp: [accept_run_suite, accept_show_report, accept_evaluate_gate]
journal: [accept.*]
updated: 2026-11-16
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
- Reference-machine hosting is D-014 (open); until it is decided, nightly machine times come from hosted runners and are labelled as such.
- A per-app GA gate presumes per-app milestones; D-017 (open) decides how they relate to phases.

## Children

_None yet: status `open`._
