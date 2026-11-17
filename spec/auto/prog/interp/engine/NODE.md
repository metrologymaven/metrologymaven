---
id: AUTO.PROG.INTERP.ENGINE
title: Execution engine, run context, limits and step hooks
level: 4
parent: AUTO.PROG.INTERP
kind: task
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-AUTO.PROG.INTERP-001, REQ-AUTO.PROG.INTERP-003, REQ-AUTO.PROG.INTERP-007]
covers: []
depends_on: [AUTO.PROG.INTERP.CALL, AUTO.PROG.CHECK, AUTO.PROG.FLOW, AUTO.PROG.EXPR.EVAL]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# AUTO.PROG.INTERP.ENGINE: Execution engine, run context, limits and step hooks

## Purpose

The core of the interpreter: walks a checked program along the control-flow graph that `AUTO.PROG.FLOW` defines, evaluates expressions with `AUTO.PROG.EXPR.EVAL`, keeps the run context (variables, call stack, iteration counters, executed steps), hands each `invoke` to `AUTO.PROG.INTERP.CALL`, enforces iteration and call-depth limits, and calls the pre-step and post-step hooks through which hosts (`AUTO.RUN.EXEC`, the debugger, operator playback) hold or stop a run at a step boundary. Given the same inputs it always issues the same invocation sequence.

## Scope

**In:**
- `Interpreter::start(program, binding, principal, target, hooks) -> RunHandle` and the step loop
- Run context: scoped variables, call stack, loop counters, `executed: BTreeSet<StepKey>`
- `StepKey { step_id, iteration_path }` identifying each dynamic execution of a step
- Limits: `max_iterations` per loop, call depth 64 [unverified]
- `StepHook` trait (`before_step`, `after_step`) and its `Allow | Stop | Hold` decisions
- `RunEvent` stream (step started, step finished, waiting_operator, stopped, ended) for hosts to journal and display
- Refusing to start a program whose check has errors

**Out:**
- Bus invocation and outcome handling (`AUTO.PROG.INTERP.CALL`)
- Operator steps (`AUTO.PROG.INTERP.OPIN`), marked subsets (`AUTO.PROG.INTERP.MARK`), the debugger (`AUTO.PROG.INTERP.DEBUG`)
- The `program.run@1` command, run branch and run record (`AUTO.RUN.EXEC`)

## Requirements

- **REQ-AUTO.PROG.INTERP.ENGINE-001** If `check_program` reports any error for the program and manifest, then `Interpreter::start` shall refuse with `program_check_failed` and execute no step.
- **REQ-AUTO.PROG.INTERP.ENGINE-002** Given the same program digest, binding, operator answers and command results, the engine shall issue a byte-identical sequence of invocations (command, major, canonical arguments, step key).
- **REQ-AUTO.PROG.INTERP.ENGINE-003** If a loop is about to start iteration `max_iterations + 1`, then the engine shall stop the run with `iteration_limit` naming the loop step, and emit a `stopped` event before any further step.
- **REQ-AUTO.PROG.INTERP.ENGINE-004** If a call would make the call depth exceed 64 [unverified], then the engine shall stop the run with `depth_limit` naming the call step, and emit a `stopped` event.
- **REQ-AUTO.PROG.INTERP.ENGINE-005** Before every `invoke` and `operator` step, the engine shall call every `before_step` hook in registration order with the step key, command ID and class; on `Stop` it shall stop the run with the hook's code, and on `Hold` it shall wait without executing the step until the host releases or cancels it.
- **REQ-AUTO.PROG.INTERP.ENGINE-006** After every committed `invoke`, the engine shall call every `after_step` hook with the step key and the step's outcome.
- **REQ-AUTO.PROG.INTERP.ENGINE-007** Values produced by expressions shall keep their `not_of_record` tag in the run context and in events, and the engine shall never place them in a ResultRecord.
- **REQ-AUTO.PROG.INTERP.ENGINE-008** When the host's cancel token is set, the engine shall start no further step and end the run with `cancelled` after the current step finishes or aborts.

## Acceptance criteria

- **AC-1** Given fixture program `fixtures/program/interp/loops-calls.json`, binding `default.json` and the scripted fake bus `fake-results.json`, when run twice, then both invocation logs are byte-identical and equal `expected-invocations.json`.
- **AC-2** (error path) Given a loop with `max_iterations: 3` whose condition stays true, then the run stops with `iteration_limit` at the loop step after exactly three iterations, and the event stream ends with `stopped`.
- **AC-3** (error path) Given a subroutine calling itself without a base case, then the run stops with `depth_limit` at call depth 65 and no 65th call is made.
- **AC-4** Given a hook that returns `Hold` before step S and is released after 100 ms, then S executes once after the release; given `Stop("gate_x")`, then S never executes and the run ends `stopped` with `gate_x`.
- **AC-5** (error path) Given a program whose check reports `unknown_command`, then `start` refuses with `program_check_failed` and the fake bus receives nothing.
- **AC-6** Given cancellation while step 3 of 10 runs, then step 3 completes, steps 4 to 10 never start and the run ends `cancelled`.

## Design

- **Registry command(s):** none; library used by `AUTO.RUN.EXEC.CMD`, `AUTO.PROG.INTERP.MARK` and `AUTO.PROG.INTERP.DEBUG`.
- **Data structures:** `RunContext { vars: ScopedVars, stack: Vec<Frame>, counters: BTreeMap<StepKey, u32>, executed: BTreeSet<StepKey> }`; `StepKey { step_id: Uuid, iteration_path: Vec<u32> }`; `trait StepHook { fn before_step(&mut self, key: &StepKey, info: &StepInfo) -> HookDecision; fn after_step(&mut self, key: &StepKey, outcome: &StepOutcome); }`; `HookDecision = Allow | Stop(Code, Detail) | Hold(Reason)`; `RunEvent`; `RunEnd = Completed | Stopped(Code, StepKey) | Cancelled | Failed(Code, StepKey)`.
- **Algorithm:** explicit-stack walk over FLOW's CFG (no host recursion); condition and argument evaluation via `expr::eval`; invoke steps delegated to `CALL`; operator steps delegated to the operator handler supplied by `OPIN`; limits checked before each loop iteration and call.
- **Errors:** `program_check_failed`, `iteration_limit`, `depth_limit`, `eval_error` (propagated), hook stop codes.
- **Determinism:** D0 for the invocation sequence given the same inputs; wall-clock waits (holds) do not affect it.
- **Performance budget:** engine overhead < 20 µs per step excluding the bus [unverified until measured].
- **Crate:** `fid-program` (module `interp::engine`).

## Test fixtures

Programs under `fixtures/program/interp/` with bindings, scripted fake-bus results and expected invocation logs (project-authored, CC-BY-4.0); fake hooks.

## Build prompt

> In crate `fid-program`, add module `interp::engine` with `Interpreter::start(program, binding, principal, target, hooks, cancel) -> RunHandle`. Refuse with `program_check_failed` if `check_program` has errors. Walk FLOW's control-flow graph with an explicit stack; evaluate expressions with `expr::eval` (values keep `not_of_record`); delegate `invoke` to `interp::call` and `operator` steps to the operator handler. Keep `StepKey { step_id, iteration_path }`, scoped variables, call stack, loop counters and the executed set. Before every invoke and operator step call each `StepHook::before_step` in order (`Allow`, `Stop(code)` → stop run, `Hold` → wait until released or cancelled); after each committed invoke call `after_step`. Stop with `iteration_limit` before iteration `max_iterations + 1` and with `depth_limit` beyond call depth 64; emit `RunEvent`s (started, finished, waiting_operator, stopped, ended). On cancel, start no further step. Same inputs must yield a byte-identical invocation sequence. Acceptance: AC-1..AC-6 with `fixtures/program/interp/`. Constraints: no panic, no `unsafe`, deny `unwrap`/`expect`/`panic`/`todo`/`unimplemented`/`unreachable`; `BTreeMap`/`BTreeSet` only; never write expression values into a ResultRecord (CONSTITUTION C1).

## Open questions

- `AUTO.RUN.EXEC.RETGT` defines a `StepGate` trait with the same shape; `AUTO.RUN.EXEC.CMD` should adapt its gate chain to this `StepHook` (the interpreter cannot depend on `AUTO.RUN`, which depends on it).
- Journaling of stops is done by the hosting command (`program.run@1`, `program.execute_marked@1`, the debugger) from the `stopped` event; the engine itself writes nothing.

## Children

_Leaf: none._
