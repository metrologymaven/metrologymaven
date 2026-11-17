---
id: VIZ.KIOSK.STEPS.LOCK
title: Kiosk step host, response allow-list and step sequencing
level: 4
parent: VIZ.KIOSK.STEPS
kind: task
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-VIZ.KIOSK.STEPS-001, REQ-VIZ.KIOSK.STEPS-002, REQ-VIZ.KIOSK.STEPS-005]
covers: []
depends_on: [VIZ.KIOSK.SESSION, AUTO.RUN.PLAY.STATE, AUTO.RUN.PLAY.SESSION, AUTO.RUN.PLAY.ANSWER]
uses_artifacts: []
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# VIZ.KIOSK.STEPS.LOCK: Kiosk step host, response allow-list and step sequencing

## Purpose

The screen that hosts the current operator step. It reads the run state, shows exactly one renderer for the step the run is waiting on plus Pause and Cancel, and nothing else. The UI-side allow-list mirrors the engine-side operator session filter (`AUTO.RUN.PLAY.SESSION`): the kiosk can only emit `program.answer_step@1` for the current step, `program.pause_run@1`, `program.resume_run@1` and `job.cancel_job@1` for the session's own job. The next step is shown only after the answer has been committed and the run state has moved on.

## Scope

**In:** step host layout (prompt area, renderer area, Pause and Cancel); renderer dispatch by response type to `VIZ.KIOSK.STEPS.FORM` or `VIZ.KIOSK.STEPS.PICK`; the UI emit filter; sequencing on `program.get_run_state@1`; the "waiting for program" state between steps; the physical target-size audit for every kiosk control.
**Out:** field and pick renderers (`VIZ.KIOSK.STEPS.FORM`, `VIZ.KIOSK.STEPS.PICK`); status display (`VIZ.KIOSK.STATUS`); failures and escalation (`VIZ.KIOSK.CYCLE.ESCAL`); the engine-side filter (`AUTO.RUN.PLAY.SESSION`).

## Requirements

- **REQ-VIZ.KIOSK.STEPS.LOCK-001** While a step is active, the kiosk UI shall be able to emit only `program.answer_step@1` for that run, step ID and iteration path, `program.pause_run@1`, `program.resume_run@1` and `job.cancel_job@1` for the session's own job, and the emit filter shall refuse any other command with `not_in_step` before it reaches the bus.
- **REQ-VIZ.KIOSK.STEPS.LOCK-002** The host shall show the next step only after `program.answer_step@1` returned `accepted` with an input entry and `program.get_run_state@1` reports a different current step or a final state.
- **REQ-VIZ.KIOSK.STEPS.LOCK-003** Every interactive control of the kiosk step screens shall measure at least 15 mm in both dimensions on the reference kiosk display, computed from the display's reported physical DPI [unverified reference display].
- **REQ-VIZ.KIOSK.STEPS.LOCK-004** If the display's physical DPI is unknown, then the host shall use the kiosk station's configured DPI setting and show a warning in the admin status, never assume 96 DPI silently.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/viz/kiosk-station` with a released program of four operator steps, when the UI test harness walks the run, then each screen shows one renderer plus Pause and Cancel, and the sequence of emitted commands is exactly four answers plus none else.
- **AC-2** (error path) Given the harness injecting a `view.set_colormap@1` emit from the kiosk surface, then the filter refuses it with `not_in_step` and the bus receives nothing.
- **AC-3** Given an answer accepted but the run state not yet advanced, then the host shows "waiting for program" and not the next step.
- **AC-4** Given the target-size audit at DPI values 96, 141 and 220, then every control is at least 15 mm; given unknown DPI and the station setting, then the warning appears.

## Design

- **Registry command(s):** none owned; invokes `program.answer_step@1`, `program.get_run_state@1`, `program.pause_run@1`, `program.resume_run@1`, `job.cancel_job@1`.
- **Data structures:** `EmitFilter { run_id, step_key, job_id }` with `admit(command, params) -> Result<(), NotInStep>`; `HostState { Waiting, Step(descriptor), Paused, Final }`.
- **Algorithm:** subscribe to run events, poll `get_run_state` on events; dispatch descriptor fields to renderers; size controls as `ceil(15 mm × dpi / 25.4)` px.
- **Errors:** `not_in_step` (UI-internal).
- **Determinism:** D0 (state machine).
- **Performance budget:** next step shown < 200 ms after state change [unverified].
- **Crate:** `fid-kiosk` (module `steps::host`).

## Test fixtures

`fixtures/viz/kiosk-station` (shared with VIZ.KIOSK.SESSION), extended with a four-step operator program.

## Build prompt

> In crate `fid-kiosk`, add module `steps::host`: the kiosk screen for the current operator step. Read `program.get_run_state@1`; when `waiting_operator`, show exactly one renderer for the descriptor (`steps::form` or `steps::pick`) plus Pause and Cancel. A UI emit filter admits only `program.answer_step@1` for the current run/step/iteration, `program.pause_run@1`, `program.resume_run@1` and `job.cancel_job@1` for the session's job, refusing anything else with `not_in_step` before the bus. Show the next step only after the answer is accepted and the run state moves on. Size every control to ≥ 15 mm from the display's physical DPI (station setting fallback with a warning). Tests AC-1..AC-4. Rules: no panics or `unsafe`; deny unwrap/expect/panic/todo/unimplemented/unreachable; kiosk shows only what the program declares (REQ-VIZ-008).

## Open questions

- `AUTO.RUN.PLAY.SESSION`'s engine-side allow-list does not include `view.get_kiosk_status@1` or the escalation command of `VIZ.KIOSK.CYCLE.ESCAL`; kiosk reads and escalation must either use another channel or be added to that list [cross-domain].

## Children

_Leaf: none._
