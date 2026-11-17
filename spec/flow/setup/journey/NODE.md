---
id: FLOW.SETUP.JOURNEY
title: Setup journey map, golden setup journal and time-to-first-project target
level: 3
parent: FLOW.SETUP
kind: task
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-FLOW.SETUP-010, REQ-FLOW.SETUP-011]
covers: [JOB.USE.01]
depends_on: [FLOW.SETUP.WIZARD, FLOW.SETUP.RULEGATE, FLOW.SETUP.SAMPLE]
uses_artifacts: [FLOW.PERSONA.SCHEMA, FLOW.RUNNER.PKG, FLOW.STUDY.PROTO]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# FLOW.SETUP.JOURNEY: Setup journey map, golden setup journal and time-to-first-project target

## Purpose

Owns the first-run journey as an acceptance artifact: the setup journey map, a golden journal of a full Lean-profile setup through to a created first project, a Regulated-profile variant, and the time-to-first-project targets that the partner study measures (`JOB.USE.01`).

## Scope

**In:**
- Journey map `JOB.USE.01` (`kind: setup`)
- Packages `setup-lean` (tracked) and `setup-regulated` (tracked) in `acceptance-job@1`
- Targets: under 10 minutes (Lean) and under 20 minutes (Regulated) to a created first project [unverified]
- The study task definition for partners (start: first app launch; end: first project created)

**Out:**
- Wizard behaviour (owned by `FLOW.SETUP.WIZARD`, `FLOW.SETUP.STEPS`, `FLOW.SETUP.RULEGATE`, `FLOW.SETUP.SAMPLE`)
- Study sessions and medians (owned by `FLOW.STUDY`)

## Requirements

- **REQ-FLOW.SETUP.JOURNEY-001** The setup journey map shall list wizard steps 1 to 8 in order with their bound commands and owning nodes, the step 4 decision (choose a rule or acknowledge the skip), and the hand-off to the first tutorial.
- **REQ-FLOW.SETUP.JOURNEY-002** The `setup-lean` package shall replay a full Lean-profile setup from a fresh settings store to a created first project, with checks that every step is done, kept-default or acknowledged-skip and that the wizard state is `completed`.
- **REQ-FLOW.SETUP.JOURNEY-003** The packages shall declare `task_s` targets of 600 s (Lean) and 1 200 s (Regulated), both with `verified: false` until study data exists.
- **REQ-FLOW.SETUP.JOURNEY-004** The study task definition shall start at the first app launch on a fresh install and end at the journaled creation of the first project.

## Acceptance criteria

- **AC-1** Given the setup map, when `fid accept check-journeys` runs, then it validates and steps 2 to 7 are human-only.
- **AC-2** Given the `setup-lean` package, when `fid accept run-suite --job JOB.USE.01 --json` runs, then replay is `IDENTICAL` and both checks pass.
- **AC-3** (error path) Given a copy whose journal skips step 4 without acknowledgement, then replay fails at step 5's `complete_step` with `setup.step_order`.

## Design

- **Registry command(s):** none; content under `acceptance/jobs/setup/`.
- **Data structures:** map per `journey-map@1`; packages per `acceptance-job@1` with checks `replay_outcome`, `records_equal` (wizard state), `result_count`.
- **Algorithm:** recorded by a person on a fresh settings store.
- **Errors:** none at runtime.
- **Determinism:** D0; times observed (N).
- **Performance budget:** none beyond targets.
- **Crate:** none (content).

## Test fixtures

Fresh settings store fixture; the Report sample dataset.

## Build prompt

> Create `acceptance/jobs/setup/` with `map.json` (`journey-map@1`, kind setup, subject `JOB.USE.01`: wizard steps 1..8 with bound commands, step 4 decision, hand-off to the first tutorial) and tracked packages `setup-lean` (task_s 600, unverified) and `setup-regulated` (task_s 1200, unverified), recorded from a fresh settings store to a created first project; checks replay_outcome, records_equal on wizard state `completed`, result_count. Add the study task definition (start first launch, end first project created) to the study task list. Verify AC-1..AC-3 of spec node FLOW.SETUP.JOURNEY.

## Open questions

- `JOB.USE.01` is a usability bar, not an acceptance job, so the packages are tracked; making setup time gating is a maintainer decision.

## Children

_Leaf: none._
