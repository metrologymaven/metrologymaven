---
id: VIZ.KIOSK.CYCLE.ESCAL
title: Failed-step decisions and supervisor escalation
level: 4
parent: VIZ.KIOSK.CYCLE
kind: feature
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-VIZ.KIOSK.CYCLE-001, REQ-VIZ.KIOSK.CYCLE-002]
covers: []
depends_on: [VIZ.KIOSK.SESSION, AUTO.RUN.PLAY.STATE, AUTO.RUN.PLAY.SESSION, CORE.BUS.PRIN, CORE.STO.SET]
uses_artifacts: []
side_effect: W2
determinism: D0
cli: [fid view resolve-kiosk-escalation]
mcp: [view_resolve_kiosk_escalation]
journal: [view.resolve_kiosk_escalation@1]
updated: 2026-11-17
---

# VIZ.KIOSK.CYCLE.ESCAL: Failed-step decisions and supervisor escalation

## Purpose

What the kiosk does when a run ends `failed` or a step fails. It offers the operator only the decisions the program declares for that step. The program format has no per-step error declarations yet (`AUTO.PROG.INTERP.CALL` stops the run on any failure), so today the declared set is always empty and every failure blocks the cycle until a supervisor resolves it. A supervisor is a person listed in the admin setting `kiosk.supervisors` who confirms their presence through the OS user-presence check of `CORE.BUS.PRIN`; they choose **restart cycle** (same program, variant and serial) or **abandon cycle**, recorded by `view.resolve_kiosk_escalation@1`.

## Scope

**In:** failure screen (failed step title, reason code, no stack traces); decision list from the program's declarations (empty until AUTO adds them); the blocked state; supervisor sign-in through OS user presence; `view.resolve_kiosk_escalation@1` (W2, `human_only: true`); restart by starting the next operator run; the admin setting `kiosk.supervisors` (machine scope, list of OS account identifiers).
**Out:** run failure semantics (`AUTO.PROG.INTERP`, `AUTO.RUN`); starting runs (`AUTO.RUN.PLAY.SESSION`); summary screen (`VIZ.KIOSK.CYCLE.SUMMARY`); motion approval (`CMM`).

## Requirements

- **REQ-VIZ.KIOSK.CYCLE.ESCAL-001** If a step fails, then the kiosk shall offer the operator only the decisions the program declares for that step, and shall offer none when the program declares none.
- **REQ-VIZ.KIOSK.CYCLE.ESCAL-002** If a failed step declares no decision, then the kiosk shall block the cycle, offer only "call supervisor" and keep blocking across restarts until the escalation is resolved.
- **REQ-VIZ.KIOSK.CYCLE.ESCAL-003** `view.resolve_kiosk_escalation@1` shall be `human_only: true` and shall commit only when the confirming person's OS account is listed in `kiosk.supervisors` and the OS user-presence check for that account succeeded in this call; otherwise it shall fail with `SupervisorRequired` and journal nothing.
- **REQ-VIZ.KIOSK.CYCLE.ESCAL-004** The command shall journal the run ID, failed step key, resolution (`restart_cycle` or `abandon_cycle`), supervisor commitment and a note of at most 500 characters.
- **REQ-VIZ.KIOSK.CYCLE.ESCAL-005** When the resolution is `restart_cycle`, the kiosk shall start a new operator run with the same program, variant and serial through `program.start_operator_run@1`; when `abandon_cycle`, it shall return to the program chooser.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/viz/kiosk-station` with a program whose step 2 fails, when the cycle runs, then the failure screen offers no operator decision and only "call supervisor".
- **AC-2** Given a fake OS presence provider confirming account `sup1` listed in `kiosk.supervisors`, when restart is chosen, then one `view.resolve_kiosk_escalation@1` entry is journaled and a new operator run starts with the same serial.
- **AC-3** (error path) Given account `op1` not in the list, then `SupervisorRequired` and nothing is journaled; the cycle stays blocked.
- **AC-4** Given the app restarted while blocked, then the kiosk comes back to the blocked screen.
- **AC-5** (agent) Given an MCP call, then it becomes a pending request and resolves nothing.

## Design

- **Registry command(s):** `view.resolve_kiosk_escalation@1`, W2, `human_only: true`, D0; params `{ run: RunId, step: StepKey, resolution: RestartCycle | AbandonCycle, note?: String (≤500) }`; result `{ entry, next_run?: RunId }`.
- **Data structures:** `EscalationState { run, step, reason_code, blocked_since }` persisted in the kiosk-session record (`VIZ.KIOSK.SESSION`).
- **Algorithm:** watch run state; on `failed`, look up declared decisions (none in FMT today); block; on supervisor action, request OS presence for the named account through `CORE.BUS.PRIN`, check the setting, commit, then act.
- **Errors:** `SupervisorRequired`, `NotBlocked`, `UnknownRun`.
- **Determinism:** D0.
- **Performance budget:** n/a (human-paced).
- **Crate:** `fid-kiosk` (module `cycle::escal`).

## Test fixtures

`fixtures/viz/kiosk-station` extended with a failing step; fake OS presence provider in the test crate.

## Build prompt

> In crate `fid-kiosk`, add module `cycle::escal`. When the operator run fails, show the failed step and reason code, offer only program-declared decisions (none exist in the program format today), and otherwise block with "call supervisor", persisting the blocked state across restarts. Add registry command `view.resolve_kiosk_escalation@1` (W2, `human_only: true`; CLI `fid view resolve-kiosk-escalation`, MCP request-only) that commits only when the confirming person's OS account is in the admin machine setting `kiosk.supervisors` (declare it) and the OS user-presence check (CORE.BUS.PRIN) succeeded; journal run, step, resolution and note; on `restart_cycle` start the next operator run with the same program, variant and serial via `program.start_operator_run@1`; on `abandon_cycle` return to the chooser. Tests AC-1..AC-5. Rules: no panics or `unsafe`; deny unwrap/expect/panic/todo/unimplemented/unreachable.

## Open questions

- AUTO has no per-step error declarations (`AUTO.PROG.INTERP.CALL` and `AUTO.PROG.FLOW` note `on_error` as a possible additive FMT change); until it exists, REQ-001 is satisfied by an empty set and every failure escalates [cross-domain].
- `AUTO.RUN.PLAY.SESSION`'s channel allow-list would refuse this command from the kiosk channel; it must be added there or issued on a separate supervisor channel [cross-domain].
- Confirming a second person's OS account inside an operator's desktop session may not be possible on every OS [unverified]; PLAT should confirm.

## Children

_Leaf: none._
