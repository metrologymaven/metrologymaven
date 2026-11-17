---
id: AUTO.PROG.INTERP
title: Interpreter, mark-and-execute and debugger
level: 3
parent: AUTO.PROG
kind: module
status: open
phase: P1b
size: XL
owners: []
claimed_by:
satisfies: [REQ-AUTO.PROG-005, REQ-AUTO.PROG-007, REQ-AUTO.PROG-008, REQ-AUTO.PROG-010, REQ-AUTO.PROG-011, REQ-AUTO.PROG-012, REQ-AUTO.PROG-013]
covers: [PCD.PRG.09, PW.AUT.02]
depends_on: [AUTO.PROG.CHECK, AUTO.SURF.JOBS, CORE.BUS, CORE.JOB]
cli: [fid program debug-*]
mcp: [program_debug_*]
journal: [program.debug_*]
updated: 2026-11-16
---

# AUTO.PROG.INTERP: Interpreter, mark-and-execute and debugger

## Purpose

Executes a checked Command Program step by step: evaluates expressions, follows control flow, sends each `invoke` through the bus under the run's principal, records operator responses as run inputs, stops on iteration or depth limits, and does all of this so that the same program, binding, operator responses and command results always yield the same sequence of invocations. It also runs a marked subset of steps and offers a debugger (breakpoints, single step, variable inspection) without changing the program.

## Scope

**In:**
- Execution engine over FMT and FLOW semantics; run context (variables, call stack, principal)
- Invocation through the bus with idempotency keys; classes refused or turned into requests exactly as on other surfaces
- Journaling operator responses as run inputs (class-N inputs for replay)
- Iteration-limit and call-depth stops with typed errors, journaled
- Mark-and-execute with up-front dependency check
- Debugger commands `program.debug_*` (breakpoints by step ID, step, continue, inspect)
- Interpreter API for `AUTO.RUN`, `AUTO.SCRIPTED` and `CMM`

**Out:**
- Run bindings, run records and batches (`AUTO.RUN`)
- Motion approval inside a run (`CMM.EXEC`)

## Requirements

- **REQ-AUTO.PROG.INTERP-001** Given the same program digest, binding, operator responses and command results, the interpreter shall issue a byte-identical sequence of invocations (command, major, canonical arguments).
- **REQ-AUTO.PROG.INTERP-002** The interpreter shall send every `invoke` through the bus under the run's principal and shall continue, stop or wait on the returned outcome as the program's error handling declares.
- **REQ-AUTO.PROG.INTERP-003** If a loop reaches its `max_iterations` or the call depth exceeds 64 [unverified], then the interpreter shall stop the run with `iteration_limit` or `depth_limit` and journal the stop.
- **REQ-AUTO.PROG.INTERP-004** When an operator step is answered, the interpreter shall validate the answer with `AUTO.PROG.OPER` and journal it as an input of the run before continuing.
- **REQ-AUTO.PROG.INTERP-005** When a marked subset is executed, the interpreter shall refuse before running anything if a marked step reads a variable that no marked or already-executed step defines.
- **REQ-AUTO.PROG.INTERP-006** The debugger shall pause before any step with a breakpoint, allow single-stepping and show variable values, and shall never change the program document.
- **REQ-AUTO.PROG.INTERP-007** Values computed by expressions during a run shall never be written into a ResultRecord as values of record.

## Interfaces

- **Offers:** `Interpreter::start(program, binding, principal) -> RunHandle`; `program.debug_*` commands; events for `AUTO.RUN` (step started, finished, waiting for operator).
- **Consumes:** `AUTO.PROG.CHECK` (a program must pass before it runs); CORE bus, jobs, journal; `AUTO.SURF.JOBS` tracker.

## Decomposition hints

ENGINE (execution and run context), BUS (invocation and outcome handling), OPIN (operator inputs), LIMITS (stops), MARK (mark-and-execute), DEBUG (debugger commands).

## Open questions

- Error-handling syntax in programs (`on_error: stop | continue | goto`) is not yet in FMT; proposed as an optional field on `invoke`, which would be an additive FMT change.
- Debugger command classes: breakpoints and stepping change session state only; proposed W1 (debug session is a draft-scope state).

## Children

_None yet: status `open`._
