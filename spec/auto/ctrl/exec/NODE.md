---
id: AUTO.CTRL.EXEC
title: Command listing, execution and jobs over the channel
level: 3
parent: AUTO.CTRL
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-AUTO.CTRL-005, REQ-AUTO.CTRL-006, REQ-AUTO.CTRL-007, REQ-AUTO.CTRL-010]
covers: [SA.MP.02]
depends_on: [AUTO.CTRL.AUTH, AUTO.SURF.ROOTS, AUTO.SURF.JOBS, CORE.BUS, CORE.JOB]
side_effect: R0
determinism: N
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# AUTO.CTRL.EXEC: Command listing, execution and jobs over the channel

## Purpose

The command operations of the control channel: list commands with an enabled flag and a reason when disabled, execute a command by ID, and list or cancel jobs. Every execution runs under an automation principal of kind `agent`, so the bus applies exactly the limits an MCP agent has, and raw paths are refused before anything reaches the bus. This is the external-control surface used by MCP bridge mode, the CLI app target and third-party SDK clients.

## Scope

**In:**
- Ops `commands.list`, `command.execute`, `jobs.list`, `job.cancel`
- Enabled flag and disabled reason from the bus's precondition evaluation, without executing
- Principal binding: kind `agent`, identity from the authenticated connection and the client's declared label
- Raw-path refusal through `AUTO.SURF.ROOTS`
- Acceptance messages and per-request outcomes so clients can classify `not_run` and `no_reply`

**Out:**
- UI inspection, input and screenshots (`AUTO.CTRL.UIOPS`)
- Action traces (`AUTO.CTRL.TRACE`)
- Class enforcement itself (`CORE.BUS`)

## Requirements

- **REQ-AUTO.CTRL.EXEC-001** `commands.list` shall return every command the session may invoke with its enabled flag, and for each disabled one the failed precondition or class rule as a typed reason.
- **REQ-AUTO.CTRL.EXEC-002** `command.execute` shall invoke the command through the bus with principal kind `agent`, so M1, S1, W3 and A0 are refused and X1 and D1 become requests exactly as for an MCP agent.
- **REQ-AUTO.CTRL.EXEC-003** If an `execute` request carries a raw value for a `path_like` parameter, then the server shall reply `raw_path_refused` before contacting the bus.
- **REQ-AUTO.CTRL.EXEC-004** For every `execute`, the server shall send an acceptance event with the job ID as soon as the bus accepts the command, and then exactly one final envelope.
- **REQ-AUTO.CTRL.EXEC-005** `jobs.list` shall list jobs of the session's principal, and `job.cancel` shall cancel only those, replying `policy_refused` for any other job.
- **REQ-AUTO.CTRL.EXEC-006** If the client declares a principal kind, role or approval in a request, then the server shall ignore it.

## Acceptance criteria

- **AC-1** Given the test app with fixture manifest `fixtures/manifest/min-classes.json` and an authenticated client, when `commands.list` runs, then every command appears, the M1 command is disabled with reason `class_not_allowed`, and a W2 command whose precondition fails is disabled with that precondition's name.
- **AC-2** Given `command.execute` of the M1 fixture command, then the reply is `failed` with `policy_refused` and the journal is unchanged.
- **AC-3** Given the X1 fixture command, then the reply is `request_filed` with a request ID.
- **AC-4** (error path) Given a raw path `C:\x.stl` for a `path_like` parameter, then `raw_path_refused` and the bus fake records no call.
- **AC-5** Given a long command, then the client receives an acceptance event with job ID before the final envelope.
- **AC-6** Given another principal's job ID, when `job.cancel` runs, then `policy_refused`.

## Design

- **Registry command(s):** none of its own; dispatches registry commands; uses `job.list_jobs@1`, `job.cancel_job@1`.
- **Data structures:** `CommandListing { command: CommandRef, enabled: bool, reason: Option<DisabledReason> }`.
- **Algorithm:** precondition evaluation through a bus dry check (no commit); execution via the `AUTO.SURF.JOBS` tracker on the server side; acceptance event emitted on bus acceptance.
- **Errors:** `policy_refused`, `raw_path_refused`, `not_found`, envelope outcomes from the bus.
- **Determinism:** N.
- **Performance budget:** `commands.list` for 5,000 commands < 50 ms [unverified until measured].
- **Crate:** `fid-ctrl`

## Test fixtures

In-process test app with `fixtures/manifest/min-classes.json`, fake bus, fake jobs.

## Build prompt

> In crate `fid-ctrl`, implement the operations `commands.list`, `command.execute`, `jobs.list` and `job.cancel` for authenticated connections. Bind every execution to principal kind `agent` (identity from the connection and client label), ignore any principal, role or approval fields the client sends, refuse raw values for `path_like` parameters with `raw_path_refused` before calling the bus, and let the bus enforce classes (M1, S1, W3, A0 refused; X1, D1 filed as requests). `commands.list` reports enabled flags and typed disabled reasons without committing. Send an acceptance event with the job ID as soon as the bus accepts, then one final envelope. Jobs: list and cancel only the session principal's jobs. Acceptance: the six cases in the spec. Constraints: no panic, no `unsafe`, deny `unwrap`/`expect`/`panic`/`todo`/`unimplemented`/`unreachable`; envelope from `fid-surface`.

## Open questions

- **Revise for D-035 (accepted 2026-11-17):** commands triggered through the control channel (including injected UI input) carry the channel's principal; see CONSTITUTION §3 injected-input rule.
- Binding control-channel clients as `agent` means their W2 commits go to an agent branch (REQ-CORE.BUS-006). That is the parent's intent (same limits as MCP agents) but may surprise SDK users scripting a running app; humans who want working-branch commits use the CLI or Python (see D-012).

## Children

_Leaf: none._
