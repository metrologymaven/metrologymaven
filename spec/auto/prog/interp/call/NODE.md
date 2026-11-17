---
id: AUTO.PROG.INTERP.CALL
title: Step invocation through the bus and outcome handling
level: 4
parent: AUTO.PROG.INTERP
kind: task
status: leaf
phase: P1b
size: S
owners: []
claimed_by:
satisfies: [REQ-AUTO.PROG.INTERP-002, REQ-AUTO.PROG.INTERP-007]
covers: []
depends_on: [AUTO.PROG.FMT, AUTO.PROG.EXPR.EVAL, AUTO.SURF.JOBS, CORE.BUS]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# AUTO.PROG.INTERP.CALL: Step invocation through the bus and outcome handling

## Purpose

Turns one `invoke` step, with its arguments evaluated, into a bus invocation under the run's principal, and turns the bus outcome into what the engine does next. It is the only place where a program touches state, so it is where the "same rules as every other surface" promise (REQ-AUTO.PROG-011) is kept: the bus decides refusals and requests, and this module never retries a write that may have committed.

## Scope

**In:**
- Building the invocation: command ID, major, canonical arguments, principal, target branch, idempotency key derived from run ID and step key
- Runtime guard against expression values passed to value-of-record parameters
- Outcome mapping: `committed` / `computed` → bind the result variable; `request_filed` → stop with `request_filed` and the request ID; `refused`, `failed` → stop with the code; `not_run` → resend once with the same key; `no_reply` → stop with `no_reply` and the job ID
- Long-running commands through the `AUTO.SURF.JOBS` tracker

**Out:**
- Class enforcement, branch routing and request filing (`CORE.BUS`)
- Control flow and hooks (`AUTO.PROG.INTERP.ENGINE`)

## Requirements

- **REQ-AUTO.PROG.INTERP.CALL-001** The module shall invoke every `invoke` step through the bus with the run's principal and target branch, and shall never call a command handler directly.
- **REQ-AUTO.PROG.INTERP.CALL-002** The idempotency key of an invocation shall be derived deterministically from the run ID and the step key, so a resend of the same step in the same run cannot commit twice.
- **REQ-AUTO.PROG.INTERP.CALL-003** If an argument value tagged `not_of_record` is about to be passed to a parameter the manifest declares `value_of_record`, then the module shall stop the run with `expression_value_of_record` and invoke nothing.
- **REQ-AUTO.PROG.INTERP.CALL-004** When the bus returns `request_filed`, the module shall stop the run with outcome `request_filed` and the request ID, and shall not wait for or perform the requested action.
- **REQ-AUTO.PROG.INTERP.CALL-005** When the bus returns `refused` or `failed`, the module shall stop the run with that outcome and code.
- **REQ-AUTO.PROG.INTERP.CALL-006** When the tracker classifies an invocation as `not_run`, the module shall resend it once with the same idempotency key; when it classifies it as `no_reply`, the module shall stop the run with `no_reply` and the job ID and shall not resend.
- **REQ-AUTO.PROG.INTERP.CALL-007** When the bus returns `committed` or `computed`, the module shall bind the result to the step's result variable, if any, with the tag `of_record` only for fields the manifest result schema marks as values of record.

## Acceptance criteria

- **AC-1** Given the fake bus and step `demo.set_offset@1` in run `R` at step key `(S, [2])`, when invoked, then the bus receives principal, branch and idempotency key `H(R, S, [2])`; running the same again yields the same key.
- **AC-2** (error path) Given an argument computed by an expression for a `value_of_record` parameter, then the run stops with `expression_value_of_record` and the fake bus receives nothing.
- **AC-3** Given the fake bus answering `request_filed` with ID `Q1`, then the run stops with outcome `request_filed` and `Q1`.
- **AC-4** Given the fake tracker classifying the first send as `not_run` and the second as `committed`, then exactly two sends with the same key occur and the run continues; given `no_reply`, then exactly one send and the run stops with `no_reply` and the job ID.
- **AC-5** Given a `committed` result for `demo.measure_thing@1` whose schema marks `value` as a value of record, then the bound variable's `value` field is tagged `of_record` and its other fields `not_of_record`.

## Design

- **Registry command(s):** none.
- **Data structures:** `Invocation { command, major, args: CanonicalJson, principal, branch, idem_key: [u8; 32] }`; `StepOutcome = Bound(Value) | Stop(StopCode, Detail)`.
- **Algorithm:** `idem_key = SHA-256("fid-prog-idem" || run_id || step_id || iteration_path)`; argument guard from the manifest parameter flags; submit through the `AUTO.SURF.JOBS` tracker; map outcomes as in Scope.
- **Errors:** `expression_value_of_record`, plus stop codes passed through from the bus.
- **Determinism:** D0 for the invocation content; network outcomes are inputs.
- **Performance budget:** < 5 µs per invocation excluding the bus [unverified until measured].
- **Crate:** `fid-program` (module `interp::call`).

## Test fixtures

Fake bus and fake tracker from `fid-surface` tests; `fixtures/manifest/prog-demo.json` with a `value_of_record` parameter and a result schema with one value-of-record field.

## Build prompt

> In crate `fid-program`, add module `interp::call`. For each `invoke` step with evaluated arguments, build an invocation (command, major, canonical args, run principal, target branch) with idempotency key SHA-256("fid-prog-idem" ‖ run ID ‖ step ID ‖ iteration path) and submit it through the `fid_surface` tracker, never calling a handler directly. Before sending, stop with `expression_value_of_record` if a `not_of_record` value goes to a parameter declared `value_of_record`. Outcomes: committed/computed → bind result (fields marked value of record in the result schema keep `of_record`); request_filed → stop with the request ID; refused/failed → stop with code; not_run → resend once with the same key; no_reply → stop with job ID, never resend. Acceptance: AC-1..AC-5 with the fake bus and tracker. Constraints: no panic, no `unsafe`, deny `unwrap`/`expect`/`panic`/`todo`/`unimplemented`/`unreachable`; the bus is the only enforcement point for classes (CONSTITUTION §3); CONSTITUTION C1.

## Open questions

- Per-step error handling (`on_error: stop | continue | goto <label>`) is not in FMT; this leaf always stops. Adding it is an additive FMT change plus an `error` edge in `AUTO.PROG.FLOW`.

## Children

_Leaf: none._
