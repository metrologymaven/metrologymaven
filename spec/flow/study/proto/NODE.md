---
id: FLOW.STUDY.PROTO
title: Timing and usability study protocol and task list
level: 3
parent: FLOW.STUDY
kind: task
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-FLOW.STUDY-001, REQ-FLOW.STUDY-002, REQ-FLOW.STUDY-005]
covers: [JOB.USE.02, JOB.USE.03]
depends_on: []
uses_artifacts: [FLOW.PERSONA.MAPS]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# FLOW.STUDY.PROTO: Timing and usability study protocol and task list

## Purpose

The published protocol for design-partner sessions: how many partners and sessions, what counts as task start and end, how the incumbent baseline is taken, how unassisted first-time success and System Usability Scale scores are measured, and what an observer records. It also ships the machine-readable task list that ties each studied task to its job ID and its journaled start and end events.

## Scope

**In:**
- Protocol document `docs/study/protocol.md` (versioned) in project words
- Task list `acceptance/study/tasks.json` (`study-tasks@1`): job ID, start event, end event, target reference
- Usability measures: first-time success on J1, J2 and J3 after one tutorial (bar 80 % [unverified]); SUS (bar 70 [unverified])
- Observer sheet fields (matching the record format)

**Out:**
- Record format and import (owned by `FLOW.STUDY.RECORD`)
- Public claims (owned by `FLOW.STUDY.CLAIM`)
- Wider usability research (owned by `UX`)

## Requirements

- **REQ-FLOW.STUDY.PROTO-001** The protocol shall require at least 3 partners and at least 2 sessions per partner per studied release, and define the incumbent-baseline procedure as the partner timing its own licensed tool on the same task and data.
- **REQ-FLOW.STUDY.PROTO-002** The task list shall define, for each studied job, the start event as the journaled file-open command and the end event as the journaled export of the released report, by command ID.
- **REQ-FLOW.STUDY.PROTO-003** The protocol shall define first-time success as completing the task unassisted with no observer help after one tutorial, measured on J1, J2 and J3 against the 80 % bar [unverified], and SUS as the standard ten-item questionnaire score against the 70 bar [unverified].
- **REQ-FLOW.STUDY.PROTO-004** The protocol shall require that part data, drawings and partner identity stay at the partner site and that only the record fields of `FLOW.STUDY.RECORD` leave it.
- **REQ-FLOW.STUDY.PROTO-005** Each task-list entry's job ID shall have a journey map, checked by a test against the embedded maps.

## Acceptance criteria

- **AC-1** Given `tasks.json`, when `cargo xtask study-tasks-check` runs, then every job ID has a map and every start and end command ID has the form `domain.verb_object@N`.
- **AC-2** (error path) Given a task with job `JOB.ACC.99`, then the check exits 1 with `TASK-NO-MAP`.
- **AC-3** Given the protocol document, when the docs link check runs, then it references the record format version used by `FLOW.STUDY.RECORD`.

## Design

- **Registry command(s):** none; repository check `cargo xtask study-tasks-check`.
- **Data structures:** `StudyTask { job_id, start_command, end_command, target_ref: package variant }`.
- **Algorithm:** document written by maintainers; the check validates the task list.
- **Errors:** `TASK-SCHEMA`, `TASK-NO-MAP`.
- **Determinism:** D0.
- **Performance budget:** none.
- **Crate:** `fid-accept` (xtask wrapper).

## Test fixtures

The shipped task list and journey maps.

## Build prompt

> Write `docs/study/protocol.md` (original words): partners >= 3, sessions >= 2 per partner per release, start = journaled file-open, end = journaled export of the released report, observer stopwatch plus journal timestamps, incumbent baseline by the partner on its own licensed tool and data (internal only), first-time success on J1-J3 after one tutorial (bar 80 %), SUS (bar 70), data-stays-on-site rule. Add `acceptance/study/tasks.json` (`study-tasks@1`: job_id, start_command, end_command, target_ref) and `cargo xtask study-tasks-check` in crate `fid-accept` (TASK-SCHEMA, TASK-NO-MAP). Verify AC-1..AC-3 of spec node FLOW.STUDY.PROTO.

## Open questions

- The bars are low-confidence targets from doc 10; maintainers confirm them after the first sessions.

## Children

_Leaf: none._
