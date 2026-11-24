---
id: AUTO.RUN.EXEC.BRANCH
title: Run branch and Run entity
level: 4
parent: AUTO.RUN.EXEC
kind: task
status: leaf
phase: P1b
size: S
owners: []
claimed_by:
built_in: [fid-run]
built_pr: 573
satisfies: [REQ-AUTO.RUN.EXEC-001]
covers: []
depends_on: [AUTO.RUN.BIND, CORE.JRN.BR, CORE.PRJ.CONT]
side_effect: W2
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# AUTO.RUN.EXEC.BRANCH: Run branch and Run entity

## Purpose

Gives each run its own place: before the first step, it creates a journal branch for the run, forked from the caller's working branch (the agent's branch for an agent principal), writes the project's Run entity from the `RunRecord` that `AUTO.RUN.BIND` prepared, and returns a `RunTarget` that routes every commit of the run to that branch. It has no registry command of its own; it issues the CORE commands `journal.create_branch@1` and `project.create_run@1` (both W2) through the bus under the run's principal.

## Scope

**In:**
- Branch naming `run/<run_id>` and fork point (caller's current working branch head)
- Run entity write through `project.create_run@1` with the `RunRecord` fields and the branch reference
- `RunTarget` routing handle passed to the interpreter context
- Finalising the Run entity's outcome field (`succeeded`, `failed`, `cancelled`) through the same entity

**Out** (and which node owns it instead):
- Branch storage, naming rules and promotion (`CORE.JRN.BR`); Run entity storage (`CORE.PRJ.CONT`)
- Record schema and lifecycle checks (`AUTO.RUN.BIND`)
- Running steps and deciding the outcome (`AUTO.RUN.EXEC.CMD`)

## Requirements

- **REQ-AUTO.RUN.EXEC.BRANCH-001** When a run starts, the system shall create journal branch `run/<run_id>` forked from the head of the calling principal's current working branch, before any step of the run is invoked.
- **REQ-AUTO.RUN.EXEC.BRANCH-002** When the branch exists, the system shall write the Run entity with `project.create_run@1`, holding every `RunRecord` field and the branch reference, as the first entry on the run branch.
- **REQ-AUTO.RUN.EXEC.BRANCH-003** The returned `RunTarget` shall route every commit made under the run to branch `run/<run_id>`, and if a commit would land on any other branch, then it shall be refused with `wrong_branch`.
- **REQ-AUTO.RUN.EXEC.BRANCH-004** If branch creation or the Run entity write fails, then the system shall return `run_setup_failed` with the cause and shall leave no Run entity without a branch.
- **REQ-AUTO.RUN.EXEC.BRANCH-005** When a run ends, the system shall write its final outcome to the Run entity on the run branch exactly once, and a second finalisation shall be refused with `already_final`.

## Acceptance criteria

- **AC-1** Given fixture project `fixtures/projects/bracket-runs/` and a prepared plan for variant `B-120`, when `open_run_target(plan, principal)` runs for a human principal on branch `main`, then branch `run/<run_id>` exists forked from `main`'s head and its first entry is `project.create_run@1` with the record's program digest and input digests.
- **AC-2** Given an agent principal working on branch `agent/a1`, then the run branch forks from `agent/a1`'s head, not from `main`.
- **AC-3** (error path) Given a test bus that fails `project.create_run@1`, then `run_setup_failed` is returned and no Run entity exists on any branch.
- **AC-4** (error path) Given a `RunTarget`, when a test commit is routed to `main`, then it is refused with `wrong_branch`.
- **AC-5** Given a finalised run, when finalisation is called again, then `already_final` and the entity is unchanged.

## Design

- **Registry command(s):** none of its own; issues `journal.create_branch@1` and `project.create_run@1` (both W2) through the bus.
- **Data structures:** `RunTarget { run_id, branch: BranchRef, fork_point: EntryHash, run_entity: EntityId }`; `open_run_target(plan: &RunPlan, principal) -> Result<RunTarget, ExecError>`; `finalise(target, outcome) -> Result<EntryHash, ExecError>`.
- **Algorithm:** read the principal's working branch head → create branch → write the Run entity → return the target. On a failed entity write, the empty branch is left (branches are append-only) but carries no Run entity, and the error names it.
- **Errors:** `run_setup_failed`, `wrong_branch`, `already_final`.
- **Determinism:** D0 apart from the run ID and branch name derived from it.
- **Performance budget:** run setup < 20 ms excluding input hashing [unverified until measured].
- **Crate:** `fid-run` (module `exec::branch`)

## Test fixtures

`fixtures/projects/bracket-runs/` and `fixtures/program/family/bracket.json` (shared, seeded generator, project-authored); a failing test bus from the CORE test harness.

## Build prompt

> In crate `fid-run`, add module `exec::branch` with `open_run_target(plan, principal)` and `finalise(target, outcome)`. Opening a run: find the head of the calling principal's current working branch (an agent's own branch for agents), create journal branch `run/<run_id>` from it with `journal.create_branch@1`, then write the Run entity with `project.create_run@1` holding every `RunRecord` field from `AUTO.RUN.BIND` plus the branch reference, as the branch's first entry. Return a `RunTarget` that routes all commits of the run to that branch and refuses any other target with `wrong_branch`. Failures return `run_setup_failed` and leave no Run entity. `finalise` writes the outcome (`succeeded`, `failed`, `cancelled`) once; a second call returns `already_final`. All writes go through the bus under the run's principal. Acceptance: AC-1..AC-5 in the spec. Constraints: CONSTITUTION §8 (no panic, no `unsafe`; deny `unwrap`, `expect`, `panic`, `todo`, `unimplemented`, `unreachable`); ordered collections only; no direct journal writes outside the bus.

## Open questions

- Build notes (fid-run, module `exec::branch`): the bus is reached through the port trait `RunBus` (working branch, branch head, Run entity read, `execute` of a `fid_bus::Invocation` on a named branch), which `AUTO.RUN.EXEC.CMD` wires to `fid_bus::Bus`. `journal.create_branch@1` and `project.create_run@1` carry `expected_head` = the fork point, so the branch starts at exactly the head read and the Run entity is its first entry or nothing. `finalise` reads the entity back and commits with `expected_head` = the head read, so a racing second finalisation is refused by the bus. AC-1..AC-5 are tested against a test bus with real plans (BIND harness); the fixture project `fixtures/projects/bracket-runs/` was not created.
- Parameters (open, obligation on `CORE.PRJ.CONT.HIER`, not edited from here): this module sends `project.create_run@1` `{"record": <run record with branch>}`, but the built command takes `{id, label, plan, revision}` (a Run container under a Plan), which cannot hold the record's fields, and the `RunPlan` names no Plan container. No command sets a Run entity's outcome; this module names `project.finalise_run@1` `{"outcome", "run"}`. CORE must define both before the production wiring can succeed.
- Agents (open, obligation on `CORE.JRN.BR.NAME` and `CORE.BUS.ROUTE`): `journal.create_branch@1` refuses agent callers and the bus refuses agent commits outside `agent/`, so against today's bus an agent's run setup fails closed with `run_setup_failed` (AC-2 is met on the test bus only). A rule for agent run branches is needed.
- Run ledger (AUTO.RUN.BIND): the ledger admits a run only from a `program.run@1` entry carrying `RunStarted` on `run/<run_id>`. With this module the run branch's first entry is the Run entity and that entry comes later; `AUTO.RUN.EXEC.CMD` must commit it on the run branch through the `RunTarget`.

## Children

_Leaf: none._
