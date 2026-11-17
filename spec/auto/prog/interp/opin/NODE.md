---
id: AUTO.PROG.INTERP.OPIN
title: Operator steps: waiting, operator input and run-input journaling
level: 4
parent: AUTO.PROG.INTERP
kind: task
status: leaf
phase: P1b
size: S
owners: []
claimed_by:
satisfies: [REQ-AUTO.PROG.INTERP-004]
covers: []
depends_on: [AUTO.PROG.INTERP.ENGINE, AUTO.PROG.OPER, CORE.JRN]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# AUTO.PROG.INTERP.OPIN: Operator steps: waiting, operator input and run-input journaling

## Purpose

What the interpreter does at an `operator` step: pause in `waiting_operator`, publish the step's descriptor, accept exactly one validated answer through `RunHandle::submit_operator_input`, journal that answer as an input of the run on the run's branch, bind its fields to the step's variables and continue. In exact replay the journaled answer is used instead of waiting. The registry command an operator uses to answer is `program.answer_step@1`, owned by `AUTO.RUN.PLAY.ANSWER`; this node is the interpreter side it calls.

## Scope

**In:**
- The engine's handler for `operator` steps and the `waiting_operator` event (run ID, step key, descriptor)
- `RunHandle::submit_operator_input(step_key, ValidAnswer) -> Result<EntryHash, OperatorInputError>`: atomic match against the waiting step, re-validation, journaling, binding, resume
- Journaling the answer as a run input through the run target's journal writer (supplied by the host, for example `AUTO.RUN.EXEC.CMD`)
- Replay: feeding journaled answers without waiting

**Out:**
- The `program.answer_step@1` command, its principal rule (`human_only`) and its field-error reply (`AUTO.RUN.PLAY.ANSWER`)
- Descriptor schema and validation rules (`AUTO.PROG.OPER`)
- Rendering prompts (`VIZ.KIOSK.STEPS`, `AUTO.PY.DLG` for scripts)

## Requirements

- **REQ-AUTO.PROG.INTERP.OPIN-001** When the engine reaches an `operator` step, the handler shall emit one `waiting_operator` event carrying the run ID, step key and descriptor, and execute no further step until an answer is accepted or the run is cancelled.
- **REQ-AUTO.PROG.INTERP.OPIN-002** If `submit_operator_input` names a step key other than the one the run is waiting on, or arrives after an answer was already accepted for that wait, then it shall fail with `not_waiting` and change nothing.
- **REQ-AUTO.PROG.INTERP.OPIN-003** The handler shall re-validate the answer with `AUTO.PROG.OPER` before accepting it, and if validation fails, then it shall fail with `answer_invalid`, journal nothing and keep waiting.
- **REQ-AUTO.PROG.INTERP.OPIN-004** When an answer is accepted, the handler shall journal one run-input record on the run's branch holding the run ID, step key, canonical answer (authored decimal strings and units, never binary floats) and the answering principal, before the next step starts, and return its entry hash.
- **REQ-AUTO.PROG.INTERP.OPIN-005** After journaling, the handler shall bind each answer field to the step's declared variables, with text fields as untrusted strings, and resume the run.
- **REQ-AUTO.PROG.INTERP.OPIN-006** When a run is replayed in exact mode, the handler shall take each answer from its journaled run-input record, emit no `waiting_operator` event and bind the same values.

## Acceptance criteria

- **AC-1** Given fixture program `fixtures/program/operator/temp-check-run.json` (step `t1`, quantity in °C, bounds 18 to 22) running on the fake bus with a fake run journal, when the run reaches `t1`, then exactly one `waiting_operator` event with the descriptor is emitted and no later step runs.
- **AC-2** Given a valid answer `20.0 °C` submitted for `t1`, then one run-input record with `"20.0"` and `°C` is journaled, its hash is returned, variable `temp` is bound and the next step runs.
- **AC-3** (error path) Given a submission for step `t2` while waiting at `t1`, or a second submission after acceptance, then `not_waiting` and the journal is unchanged.
- **AC-4** (error path) Given an answer `22.01 °C` that bypassed command validation, then `answer_invalid`, nothing journaled, still waiting.
- **AC-5** (replay) Given the journal from AC-2, when replayed in exact mode, then no `waiting_operator` event is emitted, `temp` is bound to the same value and the replay outcome is `IDENTICAL`.

## Design

- **Registry command(s):** none (the answering command is `program.answer_step@1` in `AUTO.RUN.PLAY.ANSWER`).
- **Data structures:** `Waiting { key: StepKey, descriptor: OperatorDescriptor, accepted: bool }` behind a mutex in the run handle; `RunInput { run, key, answer: CanonicalJson, principal }`; `trait RunJournal { fn append_run_input(&self, input: &RunInput) -> Result<EntryHash, JournalError>; }` supplied by the host.
- **Algorithm:** at an operator step, store `Waiting` and emit the event; `submit_operator_input` compares and sets `accepted` atomically, re-validates, appends the run input, binds fields, signals the engine; in replay, read the next run-input record for the step key.
- **Errors:** `not_waiting`, `answer_invalid`, `journal_failed`.
- **Determinism:** D0 given the journaled answers.
- **Performance budget:** accept-to-resume < 5 ms excluding the journal commit [unverified until measured].
- **Crate:** `fid-program` (module `interp::operator`).

## Test fixtures

`fixtures/program/operator/temp-check-run.json` (shared with `AUTO.RUN.PLAY.ANSWER`); fake bus and fake `RunJournal`.

## Build prompt

> In crate `fid-program`, add module `interp::operator`: the engine's handler for `operator` steps. On reaching one, store the waiting step key and descriptor and emit `waiting_operator { run, step_key, descriptor }`; run nothing else until an answer is accepted or the run is cancelled. Expose `RunHandle::submit_operator_input(step_key, ValidAnswer) -> Result<EntryHash, _>`: atomically check the run waits at exactly that key and has not accepted an answer (`not_waiting` otherwise); re-validate with `oper::validate` (`answer_invalid`, nothing journaled); append one run-input record (run, step key, canonical answer with authored decimal strings and units, principal) through the host-supplied `RunJournal` before the next step; bind fields (text as `Untrusted<String>`); resume. In exact replay, read answers from the journaled run-input records and never emit `waiting_operator`. The command `program.answer_step@1` (AUTO.RUN.PLAY.ANSWER) calls this API. Acceptance: AC-1..AC-5. Constraints: no panic, no `unsafe`, deny `unwrap`/`expect`/`panic`/`todo`/`unimplemented`/`unreachable`; C6; no binary floating point for answers.

## Open questions

- Answer commands are human-only by `AUTO.RUN.PLAY.ANSWER` (D-031, manual readings), so unattended agent-driven runs cannot pass operator steps; a later descriptor flag could allow scripts to acknowledge pure notices.

## Children

_Leaf: none._
