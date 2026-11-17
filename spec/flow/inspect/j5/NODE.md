---
id: FLOW.INSPECT.J5
title: J5 kiosk production run of a released scan program
level: 3
parent: FLOW.INSPECT
kind: task
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-FLOW.INSPECT-001, REQ-FLOW.INSPECT-005, REQ-FLOW.INSPECT-006, REQ-FLOW.INSPECT-009]
covers: [JOB.ACC.05]
depends_on: [FLOW.INSPECT.DATA, VIZ.KIOSK.SESSION, VIZ.KIOSK.PROGPICK, VIZ.KIOSK.STEPS, VIZ.KIOSK.CYCLE, AUTO.RUN.PLAY, AUTO.REC.LIFE, CORE.RPL.RECON]
uses_artifacts: [FLOW.INSPECT.J1, FLOW.PERSONA.SCHEMA, FLOW.RUNNER.PKG, ECO.DATA.CAT]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# FLOW.INSPECT.J5: J5 kiosk production run of a released scan program

## Purpose

Owns acceptance job `JOB.ACC.05`: a shop-floor operator in kiosk mode runs the released J1 scan program on 10 serials, meets a wrong-file and a missing-file case resolved by declared operator decisions, and survives a forced process kill mid-cycle with no committed result lost. It owns the journey map, packages, checks and targets.

## Scope

**In:**
- Journey map `JOB.ACC.05`
- Gating package `j5-unassisted` (10 serials, two fault cases, forced kill) and non-gating `j5-assisted` (an agent watches status read-only and may file a hold request; no other agent action)
- Lock-down check: no editing command reachable from the kiosk screen
- Time target per part

**Out:**
- The released program (recorded in `FLOW.INSPECT.J1`, released through `AUTO.REC.LIFE`)
- Kiosk screens and lock-down (owned by `VIZ.KIOSK`)
- Kill-and-restart harness (owned by `FLOW.RUNNER.RELIAB`)

## Requirements

- **REQ-FLOW.INSPECT.J5-001** The J5 journey map shall start from entering kiosk mode (`view.enter_kiosk@1`, a human administrator step), pick the released program and serial, run each cycle in operator playback, and end with the end-of-cycle action, naming the operator decisions for the wrong-file and missing-file cases.
- **REQ-FLOW.INSPECT.J5-002** The `j5-unassisted` package shall run 10 serials, include one wrong-file case and one missing-file case each resolved by a declared operator decision recorded in the journal, and be declared gating.
- **REQ-FLOW.INSPECT.J5-003** The package shall declare a `no_edit_reachable` check that, at every kiosk step, the accessible actions include no command of side-effect class W2 or higher other than those the released program declares as operator steps.
- **REQ-FLOW.INSPECT.J5-004** The package shall declare a forced kill after the commit of serial 6's results and a check that all results committed before the kill are present after restart.
- **REQ-FLOW.INSPECT.J5-005** The package shall declare an operator-time target of under 90 seconds per part [unverified].
- **REQ-FLOW.INSPECT.J5-006** The `j5-assisted` package shall be declared non-gating and shall contain no agent command other than R0 status reads and `run:hold` requests.

## Acceptance criteria

- **AC-1** Given the J5 map, when `fid accept check-journeys` runs, then it validates and the kiosk-entry step is marked human-only.
- **AC-2** Given the J5 dataset and packages, when `fid accept run-suite --job JOB.ACC.05 --json` runs, then the replay outcome is `IDENTICAL`, 10 serials have committed results, and the wrong-file and missing-file serials end with the declared operator decision and no result.
- **AC-3** (forced kill) In the same run, the runner kills the process after serial 6 commits; after restart, the results of serials 1 to 6 are present and the run resumes at serial 7.
- **AC-4** (error path) Given a test build that exposes an edit command on the kiosk screen, when the suite runs, then `no_edit_reachable` fails naming the command.

## Design

- **Registry command(s):** none; content under `acceptance/jobs/j5/`.
- **Data structures:** packages per `acceptance-job@1` with `kill_after` set to the commit of serial 6; checks `replay_outcome`, `result_count`, `operator_decision_equals`, `no_edit_reachable`, `committed_survive_kill`.
- **Algorithm:** journal recorded by a person on the reference build operating the kiosk; the released program is the J1 recorded program released by a second human.
- **Errors:** none at runtime; failures are runner findings.
- **Determinism:** D0 for results; operator time is observed (N).
- **Performance budget:** machine time per serial under 60 s on the minimum-profile reference machine [unverified].
- **Crate:** none (content).

## Test fixtures

J5 dataset from `FLOW.INSPECT.DATA`; the released program from `FLOW.INSPECT.J1`.

## Build prompt

> Create `acceptance/jobs/j5/` with `map.json` (`journey-map@1`, `JOB.ACC.05`: view.enter_kiosk@1 human-only, program pick, serial entry, operator playback per cycle, end-of-cycle action; operator decisions for wrong-file and missing-file) and two `acceptance-job@1` packages: `j5-unassisted` (gating; 10 serials; wrong-file and missing-file cases; `kill_after` at serial 6 commit; checks replay_outcome, result_count = 8, operator_decision_equals, no_edit_reachable, committed_survive_kill; operator target 90 s per part) and `j5-assisted` (tracked; agent limited to R0 reads and run:hold). Use the J1 recorded program released by a second human. Verify AC-1..AC-4 of spec node FLOW.INSPECT.J5 with `fid accept check-journeys` and `fid accept run-suite`.

## Open questions

- Entering kiosk mode is A0 (`VIZ.KIOSK.SESSION`); the map treats it as an administrator setup step before the operator's timed task.

## Children

_Leaf: none._
