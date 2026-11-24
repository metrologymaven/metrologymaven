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

- Partial build (maintainer decision, PR 573): `fid-run` module `exec::branch` implements the library half; the leaf is not marked built. Open acceptance criteria:
  - AC-1: no fixture project `fixtures/projects/bracket-runs/` (tests use the AUTO.RUN.BIND harness and a test bus), and no production `project.create_run@1` that carries the record (below).
  - AC-2: met on the test bus only; the rule for agent run branches is missing (below).
  - AC-5: `project.finalise_run@1` does not exist, so finalisation runs only on the test bus.
  - REQ-002 is partly met: the Run entity write is built and ordered as the branch's first entry, but no CORE command stores the record's fields.
- Build notes: the bus is reached through the port trait `RunBus` (working branch, branch head = last entry on the branch or none, Run entity read, `execute` of a `fid_bus::Invocation` on a named branch). The adapter (`AUTO.RUN.EXEC.CMD`) must refuse a call whose channel-derived principal differs from the one passed, route to exactly the named branch and give the call a view of that branch. `journal.create_branch@1` carries `expected_head` = the fork point; `project.create_run@1` carries `expected_head` = `GENESIS_PREV`, as `fid_bus::Bus` guards the first entry of an empty branch. `finalise` reads the run branch head first, then the entity, and commits with that head; this refuses a racing finalisation only as far as the bus enforces `expected_head` and the finalise handler refuses a non-pending entity, neither of which is authoritative until the fold below exists. `RunTarget::commit` refuses every `journal.*` command and both Run entity writes (`command_not_allowed`).
- Parameters (open, obligation on `CORE.PRJ.CONT.HIER`, not edited from here): this module sends `project.create_run@1` `{"record": <run record with branch>}`, but the built command takes `{id, label, plan, revision}` (a Run container under a Plan), which cannot hold the record's fields, and the `RunPlan` names no Plan container. No command sets a Run entity's outcome; this module names `project.finalise_run@1` `{"outcome", "run"}`.
- Run entity fold (open, obligation on `CORE.PRJ.CONT`, REQ-CORE.RPL.RECON.FOLD-008): the fold of Run entities must bind every event to the entry's verified origin. It must check that `record.principal` is the header principal; that `record.branch` is the entry's branch and is `run/<run_id>`; that the `create_run` entry is the first entry on that branch; and that `finalise_run` comes only from the run's principal, on that branch, from `pending`, with a compare-and-swap on the branch head at the commit stage. Their effects must be reserved in `fid_registry::RESERVED_EFFECTS`, and the fold must ship forged-event oracle cases.
- Agents (open, obligation on `CORE.JRN.BR.NAME` and `CORE.BUS.ROUTE`): `journal.create_branch@1` refuses agent callers and the bus refuses agent commits outside `agent/`, so against today's bus an agent's run setup fails closed with `run_setup_failed`. A rule for agent run branches is needed.
- Run ledger (AUTO.RUN.BIND): the ledger admits a run only from a `program.run@1` entry carrying `RunStarted` on `run/<run_id>`. With this module the run branch's first entry is the Run entity and that entry comes later; `AUTO.RUN.EXEC.CMD` must commit it on the run branch through the `RunTarget`.

## Children

_Leaf: none._
