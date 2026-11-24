---
id: AUTO.CLI.TARGET.APP
title: Running-app target over the control channel
level: 4
parent: AUTO.CLI.TARGET
kind: task
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
built_in: [fid-cli]
satisfies: [REQ-AUTO.CLI.TARGET-002, REQ-AUTO.CLI.TARGET-004]
covers: []
depends_on: [AUTO.CLI.TARGET.SELECT, AUTO.CTRL.CLIENT, AUTO.SURF.JOBS]
side_effect: R0
determinism: N
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# AUTO.CLI.TARGET.APP: Running-app target over the control channel

## Purpose

Runs a `fid` command inside a running app: find and authenticate to it with the control-channel client, send the invocation, and render the app's envelope unchanged. It is a thin adapter so that `--app` and `--project` differ only in where the command executes.

## Scope

**In:**
- Adapting `AUTO.CTRL.CLIENT` to the CLI's `TargetTransport`
- The 5 s reachability limit and its `not_run` exit
- Passing `not_run` / `no_reply` classifications from the client to the CLI exit codes
- `--confirm-human` on the app target: asking the running app to show its confirmation prompt

**Out:**
- Discovery, handshake and multiplexing (`AUTO.CTRL.CLIENT`)
- Server-side principal binding and the in-app confirmation UI (`AUTO.CTRL.EXEC`, `CORE.BUS.PRIN`)
- Target choice (`AUTO.CLI.TARGET.SELECT`)

## Requirements

- **REQ-AUTO.CLI.TARGET.APP-001** When the app target is selected, the CLI shall send the command through an authenticated `AUTO.CTRL.CLIENT` connection and render the returned envelope without changing any field.
- **REQ-AUTO.CLI.TARGET.APP-002** If no app is reachable and authenticated within 5 s [unverified] of start, then the CLI shall exit 6 with `not_run` and `app_not_found` or `server_auth_failed` as the reason.
- **REQ-AUTO.CLI.TARGET.APP-003** When the connection drops after the command was sent, the CLI shall exit 6 for a `not_run` classification and 7 with the job ID for a `no_reply` classification.
- **REQ-AUTO.CLI.TARGET.APP-004** When `--confirm-human` is given with the app target, the CLI shall request the running app's confirmation prompt before sending the command, and if the person declines or the app does not support it, then it shall exit 3 with `human_confirmation_failed` and send nothing.

## Acceptance criteria

- **AC-1** Given the test app running with fixture project `fixtures/projects/min.fid` open, when `fid --app demo show-thing --json` runs, then the envelope equals, field for field, the one the same command returns under the headless target except for timing fields declared volatile by `AUTO.SURF.ENV`.
- **AC-2** (error path) Given no app, when `fid --app demo show-thing` runs, then it exits 6 with reason `app_not_found` within 6 s.
- **AC-3** Given a fake server that accepts the invocation and then drops the connection, then exit 7 and the output names the job ID; given one that drops before acceptance, exit 6.
- **AC-4** (error path) Given `--confirm-human` and a test app whose confirmation is declined, then exit 3 with `human_confirmation_failed` and the fake server receives no invocation.

## Design

- **Registry command(s):** none.
- **Data structures:** `AppTransport { client: CtrlClient }` implementing `TargetTransport`.
- **Algorithm:** connect with a 5 s deadline; optional confirmation request; send; await envelope; map client classifications to exit codes 6 and 7 through `AUTO.CLI.OUT`.
- **Errors:** `app_not_found`, `server_auth_failed` (both exit 6), `human_confirmation_failed` (exit 3); everything else passes through from the envelope.
- **Determinism:** N (network timing).
- **Performance budget:** adapter overhead < 1 ms per command on top of the client [unverified until measured].
- **Crate:** `fid-cli`

## Test fixtures

The `AUTO.CTRL` test app and fake server; `fixtures/projects/min.fid`.

## Build prompt

> In crate `fid-cli`, implement `AppTransport`, a `TargetTransport` backed by `fid_ctrl_client::CtrlClient`. Connect and authenticate with a 5 s deadline; on failure exit 6 with reason `app_not_found` or `server_auth_failed`. If `--confirm-human` is set, ask the app for its human-confirmation prompt first and exit 3 `human_confirmation_failed` if declined or unsupported, sending nothing. Send the invocation, render the returned envelope unchanged, and map the client's disconnect classification to exit 6 (`not_run`) or 7 (`no_reply`, with job ID). Acceptance: AC-1..AC-4 with the control-channel test app and fake server. Constraints: no panic, no `unsafe`, deny `unwrap`/`expect`/`panic`/`todo`/`unimplemented`/`unreachable`; connect only via the discovery file to loopback.

## Open questions

- `AUTO.CTRL.EXEC` currently binds every control-channel client as principal `agent`, while `CORE.BUS.PRIN` (REQ-003) and D-012 treat CLI sessions as `script` with an in-app confirmation upgrade to `human`. The CLI app target needs CTRL to bind it as `script` and to expose a confirmation request; until CTRL is aligned, AC-4 can be tested only against the fake server. Cross-domain issue for `AUTO.CTRL.EXEC` and `CORE.BUS.PRIN`.

## Children

_Leaf: none._
