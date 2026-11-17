---
id: AUTO.RUN.PLAY.SESSION
title: Operator session start and command allow-list
level: 4
parent: AUTO.RUN.PLAY
kind: feature
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-AUTO.RUN.PLAY-001, REQ-AUTO.RUN.PLAY-002, REQ-AUTO.RUN.PLAY-003]
covers: [GOM.AUT.06, PW.AUT.04]
depends_on: [AUTO.RUN.EXEC.CMD, AUTO.RUN.BIND, CORE.BUS, CORE.JOB.SCHED]
side_effect: W2
determinism: N
cli: [fid program start-operator-run]
mcp: [program_start_operator_run]
journal: [program.start_operator_run@1]
updated: 2026-11-17
---

# AUTO.RUN.PLAY.SESSION: Operator session start and command allow-list

## Purpose

`program.start_operator_run@1` starts a released program for a shop-floor operator and opens an **operator session** bound to the calling channel (the kiosk GUI, or a CLI session upgraded to `human` under D-012). While the session is open, the only commands that channel may issue are the operator's: list released programs, start the next operator run, answer the current step, read run state, pause, resume and cancel. Anything else from that channel is refused with `not_in_program`, in the engine, so no client (kiosk screens, control channel, a second UI) can bypass the restriction. The program's own steps still run through the bus under the run's principal and are not filtered.

## Scope

**In:**
- `program.start_operator_run@1` (W2, `human_only: true`): released program only (no trial, no deprecation override), variant or binding, input handles, optional serial (untrusted text, at most 64 characters)
- `OperatorSession { session_id, channel, principal, run_id, job_id }`, one per channel at a time
- The channel-scoped allow-list filter: command IDs `program.list_operator_programs@1`, `program.start_operator_run@1` (only after the previous run ended), `program.answer_step@1`, `program.get_run_state@1`, `program.pause_run@1`, `program.resume_run@1`, `job.cancel_job@1` (only for the session's own job), `job.show_status@1`
- Closing the session: when kiosk mode exits, or when the channel closes

**Out** (and which node owns it instead):
- Kiosk enter and exit, admin rules, kiosk screens (`VIZ.KIOSK.SESSION`, `VIZ.KIOSK.STEPS`)
- Program listing (`AUTO.RUN.PLAY.SELECT`); answers (`AUTO.RUN.PLAY.ANSWER`); state, pause, resume (`AUTO.RUN.PLAY.STATE`)
- Run execution (`AUTO.RUN.EXEC.CMD`); lifecycle and binding checks (`AUTO.RUN.BIND`)
- Generic bus policy and principals (`CORE.BUS`)

## Requirements

- **REQ-AUTO.RUN.PLAY.SESSION-001** `program.start_operator_run@1` shall be declared `human_only: true`, so the bus refuses agent and script principals and MCP projects it as request-only.
- **REQ-AUTO.RUN.PLAY.SESSION-002** The command shall call `prepare_run` with `trial` and `override_deprecated` both false, and if the program is not in state `released`, then it shall refuse with `not_released` or `deprecated` and start nothing.
- **REQ-AUTO.RUN.PLAY.SESSION-003** When the plan is accepted, the command shall open an operator session bound to the calling channel, run `execute_run` in a CORE job of class `interactive`, and return session ID, run ID and job ID.
- **REQ-AUTO.RUN.PLAY.SESSION-004** While an operator session is open on a channel, the bus shall accept from that channel only the allow-listed command IDs, and shall refuse any other command with `not_in_program` before execution and without journaling it to the Record Journal.
- **REQ-AUTO.RUN.PLAY.SESSION-005** If `job.cancel_job@1` arrives on the session's channel for a job other than the session's current run job, then it shall be refused with `not_in_program`.
- **REQ-AUTO.RUN.PLAY.SESSION-006** If `program.start_operator_run@1` arrives on a channel whose session has a run that has not ended, then it shall be refused with `run_in_progress`.
- **REQ-AUTO.RUN.PLAY.SESSION-007** The allow-list filter shall apply only to calls arriving on the session's channel and shall not filter the commands the interpreter invokes for the run's own steps.
- **REQ-AUTO.RUN.PLAY.SESSION-008** A serial number, when given, shall be at most 64 characters, typed as untrusted text and recorded in the run record's binding as authored.

## Acceptance criteria

- **AC-1** Given fixture project `fixtures/projects/kiosk-programs/` and a human GUI test channel, when `program.start_operator_run@1` runs for `bracket` variant `B-120`, then it returns session, run and job IDs, one `program.start_operator_run@1` entry is journaled, and the run executes to its first operator step.
- **AC-2** (error path) Given program `flange` (registered), then `not_released`; given `hinge` (deprecated), then `deprecated`; nothing is journaled in either case.
- **AC-3** (error path) Given the open session of AC-1, when the same channel invokes `align.run_bestfit@1`, then it is refused with `not_in_program`, nothing is journaled, and the run continues.
- **AC-4** Given the open session, when the channel invokes `program.get_run_state@1`, then it is accepted.
- **AC-5** (error path) Given the open session, when the channel cancels another principal's job, then `not_in_program`; when it cancels its own run job, then the run ends `cancelled`.
- **AC-6** (error path) Given an MCP (agent) channel or a non-upgraded CLI (script) channel, when `program.start_operator_run@1` is invoked, then the bus refuses it (request-only for agents, `human_required` for script).
- **AC-7** Given the run of AC-1, whose steps invoke `scan.compare_to_cad@1`, then those step commands are executed and journaled on the run branch although they are not on the allow-list.
- **AC-8** (error path) Given a 65-character serial, then `serial_too_long`.

## Design

- **Registry command(s):** `program.start_operator_run@1`, W2, `human_only: true` (MCP request-only); params `program: ProgramRef`, `variant: String | binding: Binding`, input handles, `serial: Option<String>`; result `{ session_id, run_id, job_id }`.
- **Data structures:** `OperatorSession { session_id: Uuid (v7), channel: ChannelId, principal, run_id: Option<RunId>, job_id: Option<JobId> }`; `ALLOW_LIST: [CommandRef; 8]` (constant); `SessionFilter` implementing the bus's channel filter hook: `fn admit(channel, command, params) -> Admit | Refuse(code)`.
- **Algorithm:** start: check no running run in the channel's session → `prepare_run(no trial, no override)` → create or reuse the session for the channel → submit job running `execute_run` → return IDs. Filter: if the call's channel has a session, admit only allow-listed IDs (with the own-job rule for cancel and the run-ended rule for start). Calls from the interpreter carry the run's job context rather than the session channel, so they are not filtered.
- **Errors:** `not_released`, `deprecated`, `binding_invalid`, `run_in_progress`, `not_in_program`, `serial_too_long`.
- **Determinism:** N (session and job timing); the filter decision itself is a pure function of channel, session and command ID.
- **Performance budget:** filter decision < 5 µs per call [unverified until measured].
- **Crate:** `fid-run` (module `play::session`)

## Test fixtures

`fixtures/projects/kiosk-programs/` (shared with `AUTO.RUN.PLAY.SELECT`); GUI, MCP and CLI test channels from the CORE bus test harness. Project-authored.

## Build prompt

> In crate `fid-run`, add module `play::session` with registry command `program.start_operator_run@1` (W2, `human_only: true`; CLI `fid program start-operator-run`, MCP `program_start_operator_run`, which the projection makes request-only). It calls `prepare_run` with no trial and no deprecation override (refusing `not_released` or `deprecated`), refuses `run_in_progress` if the channel's session has a run that has not ended, validates an optional serial (≤ 64 characters, untrusted text, stored in the binding as authored), opens or reuses an `OperatorSession` bound to the calling channel, runs `execute_run` (from `exec::cmd`) in a CORE job of class `interactive`, and returns session, run and job IDs. Implement `SessionFilter` for the bus's channel filter hook: while a channel has an operator session, admit only `program.list_operator_programs@1`, `program.start_operator_run@1`, `program.answer_step@1`, `program.get_run_state@1`, `program.pause_run@1`, `program.resume_run@1`, `job.show_status@1` and `job.cancel_job@1` (own run job only); refuse everything else with `not_in_program` before execution and without a Record Journal entry. Never filter commands the interpreter invokes for the run's steps (they carry the run's job context, not the session channel). Close the session when the channel closes or kiosk mode exits. Acceptance: AC-1..AC-8 in the spec. Constraints: CONSTITUTION §8 (no panic, no `unsafe`; deny `unwrap`, `expect`, `panic`, `todo`, `unimplemented`, `unreachable`); C6 untrusted text; agents never adopt or perform human-only commands (D-031).

## Open questions

- Needs a channel-scoped command filter hook in the bus (`CORE.BUS.POL`, open; cross-domain): `Policy::authorise` must consult registered channel filters before running a command. Recorded here; not edited from this node.
- `VIZ.KIOSK.SESSION` should close the operator session on `view.exit_kiosk@1`; it depends on `AUTO.RUN` already.

## Children

_Leaf: none._
