---
id: AUTO.MCP.MODES.BRIDGE
title: Bridge mode over the control channel
level: 4
parent: AUTO.MCP.MODES
kind: task
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-AUTO.MCP.MODES-001, REQ-AUTO.MCP.MODES-004]
covers: []
depends_on: [AUTO.MCP.SESS, AUTO.MCP.SRV, AUTO.CTRL.CLIENT]
side_effect: R0
determinism: N
cli: [fid mcp --bridge]
mcp: []
journal: []
updated: 2026-11-17
---

# AUTO.MCP.MODES.BRIDGE: Bridge mode over the control channel

## Purpose

`fid mcp --bridge`: the MCP session forwards every tool call to a running app over the authenticated control channel, so the agent works in the same document a person has open, and its W2 commits land on an agent branch the person can see in the Proposal Inbox. The bridge refuses to start if the app's manifest differs from the one `fid mcp` was built with, because the tool list would then lie.

## Scope

**In:**
- Bridge transport: `AUTO.MCP.SESS` calls carried by `AUTO.CTRL.CLIENT`
- The 5 s reachability limit before listing tools
- Manifest-digest check between `fid mcp` and the app
- Mapping control-channel disconnects to `not_run` / `no_reply` tool results

**Out:**
- Control-channel protocol and client (`AUTO.CTRL`)
- Principal binding inside the app (`AUTO.CTRL.EXEC`)
- Mode choice without a flag (`AUTO.MCP.MODES.SELECT`)

## Requirements

- **REQ-AUTO.MCP.MODES.BRIDGE-001** In bridge mode every tool call shall travel over an authenticated `AUTO.CTRL.CLIENT` connection, and no command shall execute in the `fid mcp` process.
- **REQ-AUTO.MCP.MODES.BRIDGE-002** If no authenticated app is reachable within 5 s [unverified] of start, then `fid mcp --bridge` shall fail with `app_unreachable` before answering `tools/list`.
- **REQ-AUTO.MCP.MODES.BRIDGE-003** If the app's manifest digest differs from the digest embedded in `fid mcp`, then bridge mode shall fail with `manifest_mismatch` naming both digests before answering `tools/list`.
- **REQ-AUTO.MCP.MODES.BRIDGE-004** When the control channel drops, the bridge shall return `not_run` or `no_reply` (with job ID) for each in-flight tool call as the client classifies it, and shall end the MCP session.
- **REQ-AUTO.MCP.MODES.BRIDGE-005** The bridge tool list shall be computed from the embedded projection and profile only and shall contain no M1, S1 or A0 tool.

## Acceptance criteria

- **AC-1** Given the `AUTO.CTRL` test app with fixture project `fixtures/projects/min.fid`, when an MCP test client starts `fid mcp --bridge` and calls `demo_set_offset`, then the app's journal holds the entry on an agent branch and the `fid mcp` process executed no command (fake in-process bus untouched).
- **AC-2** (error path) Given no app running, then `fid mcp --bridge` fails with `app_unreachable` within 6 s and never answers `tools/list`.
- **AC-3** (error path) Given a test app built with manifest `fixtures/manifest/min-classes-v2.json`, then bridge fails with `manifest_mismatch` naming both digests.
- **AC-4** Given a fake server that accepts a call and drops the connection, then the tool result is `no_reply` with the job ID; given a drop before acceptance, `not_run`.
- **AC-5** Given `tools/list` in bridge mode, then it equals `fixtures/mcp/modes/bridge-list.json` and contains no M1, S1 or A0 tool.

## Design

- **Registry command(s):** none; host mode of `fid mcp`.
- **Data structures:** `BridgeTransport { client: CtrlClient }` implementing the session's bus transport.
- **Algorithm:** connect with 5 s deadline → `hello` returns the app's manifest digest → compare → start session → forward each call → map disconnect classifications.
- **Errors:** `app_unreachable`, `manifest_mismatch`, `not_run`, `no_reply`.
- **Determinism:** N.
- **Performance budget:** bridge overhead < 2 ms per call on loopback [unverified until measured].
- **Crate:** `fid-mcp` (module `modes::bridge`).

## Test fixtures

`AUTO.CTRL` test app and fake server; `fixtures/projects/min.fid`; `fixtures/manifest/min-classes-v2.json` (a manifest with one extra command); `fixtures/mcp/modes/bridge-list.json`.

## Build prompt

> In crate `fid-mcp`, add `modes::bridge` for `fid mcp --bridge [--server <profile>]`. Connect with `fid_ctrl_client::CtrlClient` within 5 s (`app_unreachable` otherwise); compare the app's manifest digest from the handshake with the digest embedded in `fid mcp` (`manifest_mismatch` naming both otherwise); both checks happen before `tools/list` is answered. Run the `AUTO.MCP.SESS` session with a bridge transport that forwards every call over the channel and never executes locally. On disconnect, return the client's `not_run` / `no_reply` (with job ID) per in-flight call and end the session. Tool list from embedded projection plus profile only. Acceptance: AC-1..AC-5 with the control-channel test app and fake server. Constraints: no panic, no `unsafe`, deny `unwrap`/`expect`/`panic`/`todo`/`unimplemented`/`unreachable`; no M1/S1/A0 tools (CONSTITUTION §3).

## Open questions

- REQ-003 needs the control-channel `hello` reply to carry the app's manifest digest; `AUTO.CTRL.WIRE` lists protocol versions only. If WIRE does not add it, the bridge can call `registry.get_manifest_digest`-style R0 query instead (owner `CORE.REG`).

## Children

_Leaf: none._
