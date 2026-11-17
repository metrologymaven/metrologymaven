---
id: AUTO.RUN.PLAY.ANSWER
title: Operator step answers
level: 4
parent: AUTO.RUN.PLAY
kind: feature
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-AUTO.RUN.PLAY-001, REQ-AUTO.RUN.PLAY-004]
covers: []
depends_on: [AUTO.PROG.OPER, AUTO.PROG.INTERP, AUTO.RUN.EXEC.CMD]
side_effect: W2
determinism: D0
cli: [fid program answer-step]
mcp: [program_answer_step]
journal: [program.answer_step@1]
updated: 2026-11-17
---

# AUTO.RUN.PLAY.ANSWER: Operator step answers

## Purpose

`program.answer_step@1` is how an operator answers the step a run is waiting on: acknowledge an instruction, confirm, enter a number or a quantity in the declared unit, type text, choose, or pick an element. It checks that the named step is the one the run is waiting on, validates the answer with the operator descriptor (`AUTO.PROG.OPER`), returns every field error if the answer is invalid (the run keeps waiting), and otherwise hands the valid answer to the interpreter, which journals it as a run input and continues. Operator answers are manual readings, so the command is human-only (D-031).

## Scope

**In:**
- `program.answer_step@1` (W2, `human_only: true`)
- Matching the answer to the waiting step by run ID, step ID and iteration path
- Validation with `validate(descriptor, answer)`; complete field error list
- Hand-off of the valid answer to the interpreter's operator-input entry

**Out** (and which node owns it instead):
- Descriptor schema and validation rules (`AUTO.PROG.OPER`)
- Journaling the answer as a run input and continuing (`AUTO.PROG.INTERP`)
- Rendering steps (`VIZ.KIOSK.STEPS`); session allow-list (`AUTO.RUN.PLAY.SESSION`)

## Requirements

- **REQ-AUTO.RUN.PLAY.ANSWER-001** `program.answer_step@1` shall be declared `human_only: true`.
- **REQ-AUTO.RUN.PLAY.ANSWER-002** If the run is not waiting for an operator answer at the named step ID and iteration path, then the command shall refuse with `not_waiting` and change nothing.
- **REQ-AUTO.RUN.PLAY.ANSWER-003** The command shall validate the answer against the waiting step's descriptor, and if validation fails, then it shall return every field error with field name and code, keep the run waiting and journal nothing.
- **REQ-AUTO.RUN.PLAY.ANSWER-004** When the answer is valid, the command shall pass the validated answer to the interpreter, which journals it as an input of the run on the run branch before the next step starts.
- **REQ-AUTO.RUN.PLAY.ANSWER-005** Numeric and quantity answers shall be passed as authored decimal strings with their unit, never converted to binary floating point by this command.
- **REQ-AUTO.RUN.PLAY.ANSWER-006** If two answers for the same waiting step arrive, then the first valid one shall be accepted and the second refused with `not_waiting`.

## Acceptance criteria

- **AC-1** Given fixture program `fixtures/program/operator/temp-check-run.json` (step `t1`, quantity field kind temperature, unit `°C`, min `18`, max `22`) waiting at `t1`, when `program.answer_step@1` is sent with `20.0 °C` by a human principal, then it succeeds, the run branch holds the answer as a run input with value `20.0` and unit `°C`, and the run continues.
- **AC-2** (error path) Given the same wait, when `22.01 °C` is sent, then the result lists `above_max` for the field, nothing is journaled, and `program.get_run_state@1` still shows the run waiting at `t1`.
- **AC-3** (error path) Given a three-field form step and three invalid fields, then three errors in field order.
- **AC-4** (error path) Given an answer naming step `t2` while the run waits at `t1`, then `not_waiting`.
- **AC-5** (error path) Given an agent principal, then the bus does not execute the command (request-only); given a non-upgraded script principal, then `human_required`.
- **AC-6** Given two valid answers sent concurrently, then exactly one is journaled and the other gets `not_waiting`.

## Design

- **Registry command(s):** `program.answer_step@1`, W2, `human_only: true` (MCP request-only); params `run: RunId`, `step_id: StepId`, `iteration_path: Vec<u32>`, `answer: AnswerValues` (field name → authored string plus optional unit); result `{ accepted: true, input_entry: EntryHash } | { accepted: false, errors: Vec<FieldError> }`.
- **Data structures:** uses `OperatorDescriptor`, `ValidAnswer`, `FieldError` from `AUTO.PROG.OPER`; `WaitingStep { run_id, key: StepKey, descriptor }` read from the run's `waiting_operator` event.
- **Algorithm:** look up the run's current waiting step (atomic compare on run ID and step key) → `validate` → on success, `RunHandle::submit_operator_input(key, valid_answer)` (interpreter journals it) and clear the wait; on failure return the errors.
- **Errors:** `run_not_found`, `not_waiting`, `answer_invalid` (with field errors), `human_required` (from the bus).
- **Determinism:** D0: the accept or refuse decision and the journaled value are a pure function of the descriptor and the answer.
- **Performance budget:** < 5 ms per answer excluding journal commit [unverified until measured].
- **Crate:** `fid-run` (module `play::answer`)

## Test fixtures

New `fixtures/program/operator/temp-check-run.json` and a three-field form program (project-authored), reusing descriptors of `fixtures/operator/temp-check.json` from `AUTO.PROG.OPER`.

## Build prompt

> In crate `fid-run`, add module `play::answer` with registry command `program.answer_step@1` (W2, `human_only: true`; CLI `fid program answer-step`, MCP `program_answer_step`, request-only by projection). Params: run ID, step ID, iteration path, answer values as authored strings with optional units. Refuse with `not_waiting` unless the run is currently waiting for an operator answer at exactly that step key (atomic check, so of two concurrent answers only the first valid one is accepted). Validate with `fid_program::operator::validate` against the waiting step's descriptor; on failure return every field error in field order, keep the run waiting and journal nothing. On success pass the `ValidAnswer` to the interpreter's `RunHandle::submit_operator_input`, which journals it as a run input on the run branch before the next step. Never convert numeric answers to floating point. Acceptance: AC-1..AC-6 in the spec. Constraints: CONSTITUTION §8 (no panic, no `unsafe`; deny `unwrap`, `expect`, `panic`, `todo`, `unimplemented`, `unreachable`); C6 (text answers stay untrusted); manual readings are human-only (D-031).

## Open questions

- Requires `AUTO.PROG.INTERP` (open) to expose `RunHandle::submit_operator_input(step_key, ValidAnswer)` and a `waiting_operator` event carrying the descriptor; its REQ-AUTO.PROG.INTERP-004 already says the interpreter journals the validated answer.

## Children

_Leaf: none._
