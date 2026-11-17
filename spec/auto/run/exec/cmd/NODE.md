---
id: AUTO.RUN.EXEC.CMD
title: The program.run command, job, progress and cancellation
level: 4
parent: AUTO.RUN.EXEC
kind: feature
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-AUTO.RUN.EXEC-003, REQ-AUTO.RUN.EXEC-004]
covers: [PW.AUT.05]
depends_on: [AUTO.RUN.BIND, AUTO.RUN.EXEC.BRANCH, AUTO.RUN.EXEC.RETGT, AUTO.PROG.INTERP, CORE.JOB.SCHED, CORE.JOB.PROG]
side_effect: W2
determinism: N
cli: [fid program run]
mcp: [program_run]
journal: [program.run@1]
updated: 2026-11-17
---

# AUTO.RUN.EXEC.CMD: The program.run command, job, progress and cancellation

## Purpose

`program.run@1` re-runs a released program on new data. It calls `prepare_run` (`AUTO.RUN.BIND`), opens the run's branch and Run entity (`AUTO.RUN.EXEC.BRANCH`), starts the interpreter (`AUTO.PROG.INTERP`) under the run's principal as one CORE job, passes every step through the chain of `StepGate`s (the re-target guard always, plus gates other nodes add), publishes progress by step, honours cancellation at step boundaries, and finalises the run's outcome. The same flow is offered as the library function `execute_run`, which `AUTO.RUN.BATCH` and `AUTO.RUN.PLAY.SESSION` call inside their own jobs.

## Scope

**In:**
- `program.run@1` (W2), submitted as a CORE job of class `interactive`
- `execute_run(plan, ctx) -> RunOutcome` library entry (no job of its own)
- Gate chain: `RetargetGuard` first, then extra gates in registration order; `Hold` blocks at the step boundary until released or cancelled
- Default M1 gate: refuse any step whose command class is M1 with `motion_unavailable`, until `AUTO.RUN.EXEC.APPR` registers its gate
- Progress per step; cancellation at step boundaries; outcome finalisation
- `RunEvent` stream: `step_started`, `step_finished`, `waiting_operator`, `held`, `run_finished`

**Out** (and which node owns it instead):
- Lifecycle and binding checks (`AUTO.RUN.BIND`); branch and Run entity (`AUTO.RUN.EXEC.BRANCH`); the re-target rule (`AUTO.RUN.EXEC.RETGT`)
- Motion approval waits (`AUTO.RUN.EXEC.APPR`); operator sessions, answers and pause (`AUTO.RUN.PLAY`)
- Batches (`AUTO.RUN.BATCH`); run comparison (`AUTO.RUN.CMP`)
- Step semantics and journaling of step commands (`AUTO.PROG.INTERP`)

## Requirements

- **REQ-AUTO.RUN.EXEC.CMD-001** When `program.run@1` is invoked, the system shall call `prepare_run`, and if it refuses, then the command shall fail with that error, start no job and journal nothing.
- **REQ-AUTO.RUN.EXEC.CMD-002** When the plan is accepted, the system shall submit one CORE job of class `interactive`, return its job ID at once, open the run target, and start the interpreter with the run's principal and the run target.
- **REQ-AUTO.RUN.EXEC.CMD-003** Before each step, the system shall evaluate the gate chain in order, starting with the re-target guard, and shall invoke the step only if every gate returns `Allow`.
- **REQ-AUTO.RUN.EXEC.CMD-004** If a gate returns `Stop`, then the system shall stop the run before that step and finalise the run as `failed` with the gate's error code and detail.
- **REQ-AUTO.RUN.EXEC.CMD-005** If no registered gate handles a step whose command class is M1, then the system shall stop the run with `motion_unavailable`.
- **REQ-AUTO.RUN.EXEC.CMD-006** While a run executes, the system shall publish progress with fraction `step_index / step_count` and a stage naming the step index, step count and step ID, at every step start.
- **REQ-AUTO.RUN.EXEC.CMD-007** When the run's job is cancelled, the system shall let the current step finish or abort as the interpreter allows, start no further step, keep every committed step journaled and finalise the run as `cancelled`.
- **REQ-AUTO.RUN.EXEC.CMD-008** When the interpreter ends without stop or cancellation, the system shall finalise the run as `succeeded` if no step ended in an unhandled error and as `failed` otherwise.
- **REQ-AUTO.RUN.EXEC.CMD-009** The system shall emit one `RunEvent` per step start, step finish, operator wait, hold and run end, each carrying run ID, step ID and iteration path.
- **REQ-AUTO.RUN.EXEC.CMD-010** After each step commits, the system shall pass the step result to every gate's `after_step`, and before finalising a run as `succeeded` it shall evaluate every gate's `before_end` and apply `Hold` or `Stop` as for a step.

## Acceptance criteria

- **AC-1** Given released program `fixtures/program/family/bracket.json` and variant `B-120` with scan `fixtures/scans/bracket-120.ply`, when `fid program run --program bracket --variant B-120 --scan <handle> --json` runs, then it returns a job ID, the job succeeds, the run branch holds the program's step entries, and the Run entity's outcome is `succeeded`.
- **AC-2** Given the same run observed through `fid job show-status <job> --json`, then progress was published at each of the program's 12 steps with stages `step 1/12 <id>` .. `step 12/12 <id>`.
- **AC-3** Given program `fixtures/program/retarget/manual-pick.json` bound to `bracket-120-shifted.ply`, then the run stops before `s4` with `not_retargetable` and the Run entity's outcome is `failed`.
- **AC-4** Given a program whose step 5 invokes an M1 test command and no APPR gate registered, then the run stops before step 5 with `motion_unavailable`.
- **AC-5** Given a run of the 12-step program cancelled with `fid job cancel-job <job>` while step 4 runs, then steps 1..4 stay journaled on the run branch, no step 5 entry exists, and the outcome is `cancelled`.
- **AC-6** (error path) Given a registered (unreleased) program and an agent principal, then `not_released`, no job, nothing journaled.
- **AC-7** Given the run of AC-1 repeated on the same inputs, then `program.compare_runs@1` reports every step `IDENTICAL`.

## Design

- **Registry command(s):** `program.run@1`, W2 (MCP `program_run`, named; agent runs fork from the agent branch); params `program: ProgramRef`, `binding: Binding | variant: String`, input handles, `trial: bool = false`, `override_deprecated: bool = false`; result `{ run_id, job_id }` immediately, final `{ run_id, outcome, failed_step?, error? }` through the job.
- **Data structures:** `RunCtx { principal, target: RunTarget, gates: Vec<Box<dyn StepGate>>, progress: ProgressSink, cancel: CancelToken, events: RunEventSink }`; `RunOutcome { run_id, outcome: Succeeded|Failed|Cancelled, failed_step: Option<StepKey>, error: Option<ErrorCode> }`; `RunEvent { kind, run_id, step: Option<StepKey> }`. `StepGate`, `GateDecision` re-exported from `exec::retarget`.
- **Algorithm:** `prepare_run` → submit job → in the job: `open_run_target` → `Interpreter::start(program, binding, principal, target, pre_step_hook)` where the pre-step hook runs the gate chain (and the M1 default) and checks the cancel token, and a post-step hook calls each gate's `after_step`; before a `succeeded` finalisation each gate's `before_end` is evaluated → on interpreter end, `finalise`. `Hold` waits on a condition that the holding gate releases, while still observing the cancel token.
- **Errors:** propagated from BIND (`not_released`, `deprecated`, `binding_invalid`, `input_unreadable`), BRANCH (`run_setup_failed`); stops `not_retargetable`, `motion_unavailable`, gate codes; `interpreter_error`.
- **Determinism:** N for orchestration (job timing, cancellation point). Given identical inputs and no cancellation, the sequence of step invocations is the interpreter's (REQ-AUTO.PROG.INTERP-001).
- **Performance budget:** per-step orchestration overhead < 1 ms beyond the step itself [unverified until measured].
- **Crate:** `fid-run` (module `exec::cmd`)

## Test fixtures

`fixtures/program/family/bracket.json` (12 steps), `fixtures/program/retarget/manual-pick.json`, new `fixtures/program/motion/m1-step5.json` (uses a test command registered as M1 in the test registry), scans from the seeded synthetic generator; all project-authored.

## Build prompt

> In crate `fid-run`, add module `exec::cmd` with registry command `program.run@1` (W2; CLI `fid program run`, MCP `program_run`) and library function `execute_run(plan, ctx) -> RunOutcome`. The command calls `prepare_run` (refusal fails the command with no job and no journal), submits one CORE job of class `interactive` and returns `{run_id, job_id}` at once. Inside the job, `execute_run` opens the run target (`exec::branch::open_run_target`), starts `Interpreter::start` under the run's principal with a pre-step hook, and finalises the outcome with `exec::branch::finalise`. The hook evaluates the gate chain in order (the `RetargetGuard` from `exec::retarget` first, then registered gates): `Allow` continues, `Stop(code)` ends the run `failed` with that code, `Hold` waits at the boundary while watching the cancel token. After each committed step call every gate's `after_step` with the step result; before finalising `succeeded`, evaluate every gate's `before_end` (Hold or Stop as for a step). If no gate handles a step whose command class is M1, stop with `motion_unavailable`. Publish progress `step_index/step_count` with stage `step i/n <step_id>` at each step start; emit `RunEvent`s (step_started, step_finished, waiting_operator, held, run_finished) with run ID, step ID and iteration path. On cancellation start no further step, keep committed steps, finalise `cancelled`. Re-export `StepGate` and `GateDecision`. Acceptance: AC-1..AC-7 in the spec. Constraints: CONSTITUTION §8 (no panic, no `unsafe`; deny `unwrap`, `expect`, `panic`, `todo`, `unimplemented`, `unreachable`); ordered collections only; agents never trigger motion (D-008); no value of record computed here (C1).

## Open questions

- Requires the pre-step hook on `AUTO.PROG.INTERP` (open): `Interpreter::start` must accept a hook called before every `invoke` with step ID, iteration path and command class. Recorded on the parent; to be added when INTERP is split.

## Children

_Leaf: none._
