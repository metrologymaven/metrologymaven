---
id: FLOW.INSPECT.J7GF
title: J7 gap and flush extension (launch-2)
level: 3
parent: FLOW.INSPECT
kind: task
status: leaf
phase: P3
size: S
owners: []
claimed_by:
satisfies: [REQ-FLOW.INSPECT-001, REQ-FLOW.INSPECT-007, REQ-FLOW.INSPECT-009]
covers: [JOB.ACC.07]
depends_on: [FLOW.INSPECT.J7, IND.GAPFL.DEF, IND.GAPFL.SECT, IND.GAPFL.TPL]
uses_artifacts: [FLOW.PERSONA.SCHEMA, FLOW.RUNNER.PKG, ECO.DATA.CAT, IND.GAPFL.DATA]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# FLOW.INSPECT.J7GF: J7 gap and flush extension (launch-2)

## Purpose

Completes acceptance job `JOB.ACC.07` with its 10 gap and flush points once the `IND.GAPFL` pack exists (P3). It extends the J7 journey map and adds the full-scope packages; the launch-1 packages of `FLOW.INSPECT.J7` stay unchanged.

## Scope

**In:**
- Gap/flush steps appended to the J7 journey map as a map revision
- Packages `j7full-unassisted` (gating for launch-2), `j7full-repeat` and `j7full-assisted` (non-gating)
- Gap/flush checks against the J7 truth table and `IND.GAPFL.DATA` reference cases

**Out:**
- Sections and thickness (owned by `FLOW.INSPECT.J7`)
- Gap/flush analysis (owned by `IND.GAPFL`)

## Requirements

- **REQ-FLOW.INSPECT.J7GF-001** The J7 journey map revision shall add, after the thickness step, 10 gap and flush evaluations (`pack.evaluate_gapflush@1`) and the gap/flush report template step, each with its owning node.
- **REQ-FLOW.INSPECT.J7GF-002** The `j7full-unassisted` package shall declare `value_within_u` checks for all 10 gap and 10 flush values against the J7 truth table, in addition to every check of `j7-unassisted`.
- **REQ-FLOW.INSPECT.J7GF-003** The packages shall declare time targets of under 20 minutes for `j7full-unassisted` and under 2 minutes for `j7full-repeat` [unverified].
- **REQ-FLOW.INSPECT.J7GF-004** The `j7full-unassisted` and `j7full-repeat` packages shall be declared gating for the launch-2 milestone only, and `j7full-assisted` non-gating.

## Acceptance criteria

- **AC-1** Given the revised map, when `fid accept check-journeys --phase P3` runs, then it validates.
- **AC-2** Given the J7 dataset and packages, when `fid accept run-suite --job JOB.ACC.07 --json` runs on a build with the gap/flush pack, then all 20 gap/flush checks and all J7 checks pass.
- **AC-3** (error path) Given a truth table with one flush value altered by 0.5 mm, then exactly that check fails.

## Design

- **Registry command(s):** none; content under `acceptance/jobs/j7/full/`.
- **Data structures:** packages with `milestones: [launch-2]`; checks as in J7 plus `value_within_u` on gap/flush.
- **Algorithm:** recorded by a person on a build with the pack enabled.
- **Errors:** none at runtime.
- **Determinism:** D0 for values; times observed (N).
- **Performance budget:** as J7 [unverified].
- **Crate:** none (content).

## Test fixtures

J7 dataset from `FLOW.INSPECT.DATA` (its gap/flush truth); reference cases from `IND.GAPFL.DATA`.

## Build prompt

> When the gap and flush pack is available, add `acceptance/jobs/j7/full/`: revise the J7 `journey-map@1` to append 10 pack.evaluate_gapflush@1 steps and the gap/flush report template step, and create packages `j7full-unassisted` (gating, launch-2, 20 min), `j7full-repeat` (gating, launch-2, 2 min), `j7full-assisted` (tracked). Checks: all checks of j7-unassisted plus value_within_u on 10 gap and 10 flush truths. Verify AC-1..AC-3 of spec node FLOW.INSPECT.J7GF.

## Open questions

- Milestone `launch-1` should list `FLOW.INSPECT.J7GF` under `exclude`, since it depends on `IND.GAPFL` (excluded); this is an edit to `milestones/launch-1.md` for maintainers.

## Children

_Leaf: none._
