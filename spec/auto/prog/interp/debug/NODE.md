---
id: AUTO.PROG.INTERP.DEBUG
title: Debug sessions, breakpoints, stepping and inspection
level: 4
parent: AUTO.PROG.INTERP
kind: feature
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-AUTO.PROG.INTERP-006]
covers: [PW.AUT.02]
depends_on: [AUTO.PROG.INTERP.ENGINE, AUTO.SURF.JOBS, CORE.JOB]
side_effect: [W1, W1, W1, W1, R0, W1]
determinism: N
cli: [fid program debug-start, fid program debug-set-breakpoints, fid program debug-step, fid program debug-continue, fid program debug-inspect, fid program debug-stop]
mcp: [program_debug_start, program_debug_set_breakpoints, program_debug_step, program_debug_continue, program_debug_inspect, program_debug_stop]
journal: [program.debug_start@1, program.debug_set_breakpoints@1, program.debug_step@1, program.debug_continue@1, program.debug_inspect@1, program.debug_stop@1]
updated: 2026-11-17
---

# AUTO.PROG.INTERP.DEBUG: Debug sessions, breakpoints, stepping and inspection

## Purpose

The program debugger: start a debug session on a draft or registered program, set breakpoints by step ID, single-step or continue, inspect variables and the call stack, and stop. A debug session is a run of the engine with a breakpoint hook; the debug commands change only session state (W1) and never the program document, while each `invoke` the program executes goes through the bus with its own class and journal entry. Sessions also hold the "already executed" state that mark-and-execute uses.

## Scope

**In:**
- Six commands `program.debug_*` and the debug-session store (in memory, per app process)
- A breakpoint `StepHook` that returns `Hold` at breakpoint steps and after a single step
- Inspection: variables with types and `not_of_record` tags, call stack, current step key, executed step keys
- Session limits (count per principal, idle timeout)

**Out:**
- Execution and hooks (`AUTO.PROG.INTERP.ENGINE`); marked execution (`AUTO.PROG.INTERP.MARK`)
- Debugger widgets (`VIZ`)

## Requirements

- **REQ-AUTO.PROG.INTERP.DEBUG-001** `program.debug_start@1` shall create a debug session over a program that passes check, with a binding, under the caller's principal, paused before the first step, and return a session ID.
- **REQ-AUTO.PROG.INTERP.DEBUG-002** While a session runs, the engine shall pause before any step whose ID is in the session's breakpoint set and report the paused step key.
- **REQ-AUTO.PROG.INTERP.DEBUG-003** `program.debug_step@1` shall execute exactly one step (entering a call as one step into the subroutine) and pause again; `program.debug_continue@1` shall run until the next breakpoint, a stop or the end.
- **REQ-AUTO.PROG.INTERP.DEBUG-004** `program.debug_inspect@1` shall return every variable in scope with value, type and `of_record` tag, the call stack and the current step key, and shall change nothing.
- **REQ-AUTO.PROG.INTERP.DEBUG-005** No debug command shall change the program document or its digest.
- **REQ-AUTO.PROG.INTERP.DEBUG-006** Debug commands shall act only on sessions created by the same principal, and shall fail with `session_not_found` for any other session ID.
- **REQ-AUTO.PROG.INTERP.DEBUG-007** If a session is idle for 30 minutes [unverified] or a principal already holds 4 sessions [unverified], then the oldest idle session shall be stopped or the start refused with `too_many_sessions`.

## Acceptance criteria

- **AC-1** Given fixture program `fixtures/program/interp/loops-calls.json` and the fake bus, when a session starts, a breakpoint is set on step `S5` and `debug-continue` runs, then the session pauses at `S5` with the expected step key, and the fake bus saw exactly the invocations before `S5`.
- **AC-2** Given a paused session, when `debug-step` runs three times, then exactly three steps execute and `debug-inspect` returns the variables in `expected-vars-after-3.json`.
- **AC-3** Given the whole session, then the program digest before and after is identical.
- **AC-4** (error path) Given a session started by principal A, when principal B calls `debug-step` on it, then `session_not_found`.
- **AC-5** (error path) Given a program failing check, then `debug-start` fails with `program_check_failed`.
- **AC-6** Given an MCP agent session, when it starts a debug session and steps over a W2 invoke, then the commit lands on its agent branch, as for any agent W2 call.

## Design

- **Registry command(s):** `program.debug_start@1` (W1; params `program`, `binding`), `program.debug_set_breakpoints@1` (W1; `session`, `step_ids`), `program.debug_step@1` (W1; `session`), `program.debug_continue@1` (W1; `session`), `program.debug_inspect@1` (R0; `session`), `program.debug_stop@1` (W1; `session`). W1 because they mutate only draft-scope session state; the steps the program executes are journaled by their own commands.
- **Data structures:** `DebugSession { id, principal, run: RunHandle, breakpoints: BTreeSet<StepId>, mode: Paused(StepKey) | Running | Ended(RunEnd), last_used }`.
- **Algorithm:** start engine with a `BreakpointHook` that returns `Hold` at breakpoints or after one step in step mode; commands release the hold; inspection reads a snapshot of the run context.
- **Errors:** `program_check_failed`, `session_not_found`, `too_many_sessions`, `session_ended`.
- **Determinism:** N (interactive pacing); the invocation sequence is the engine's (D0).
- **Performance budget:** pause-to-reply < 20 ms [unverified until measured].
- **Crate:** `fid-program` (module `interp::debug`).

## Test fixtures

`fixtures/program/interp/loops-calls.json` with `expected-vars-after-3.json`; fake bus with two principals and an MCP agent principal.

## Build prompt

> In crate `fid-program`, add module `interp::debug` with six registry commands: `program.debug_start@1` (W1: check the program, start the engine under the caller's principal with a breakpoint hook, paused before the first step, return session ID), `program.debug_set_breakpoints@1` (W1), `program.debug_step@1` (W1: execute exactly one step, a call counts as stepping into the subroutine), `program.debug_continue@1` (W1: run to next breakpoint, stop or end), `program.debug_inspect@1` (R0: variables with value, type and of-record tag, call stack, current step key), `program.debug_stop@1` (W1). Sessions are owned by their creating principal (`session_not_found` otherwise), at most 4 per principal, idle timeout 30 min. Debug commands never modify the program document; executed invokes go through the bus with their own classes. CLI `fid program debug-*`, MCP `program_debug_*`. Acceptance: AC-1..AC-6. Constraints: no panic, no `unsafe`, deny `unwrap`/`expect`/`panic`/`todo`/`unimplemented`/`unreachable`; R0 inspect journals only to the Activity Log (D-031).

## Open questions

- The target's question on debug command classes is answered: W1 (draft-scope session state), inspect R0.

## Children

_Leaf: none._
