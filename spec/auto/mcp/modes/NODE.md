---
id: AUTO.MCP.MODES
title: Bridge, headless and auto modes
level: 3
parent: AUTO.MCP
kind: module
status: split
phase: P0
size: L
owners: []
claimed_by:
satisfies: [REQ-AUTO.MCP-002, REQ-AUTO.MCP-006, REQ-AUTO.MCP-007, REQ-AUTO.MCP-008]
covers: []
depends_on: [AUTO.MCP.SESS, AUTO.CTRL.CLIENT]
cli: [fid mcp, fid mcp --bridge, fid mcp --headless]
mcp: []
journal: []
updated: 2026-11-17
---

# AUTO.MCP.MODES: Bridge, headless and auto modes

## Purpose

Where an MCP session's commands execute. Bridge mode forwards every tool call to a running app over the authenticated control channel; headless mode runs an in-process session with no window; auto mode picks bridge when an app is running and otherwise follows the user's "app closed" setting (headless, launch the app, or refuse). All three modes must present the same tools for the same manifest digest.

## Scope

**In:**
- Mode selection and the "app closed" setting (read-only here)
- Bridge transport over `AUTO.CTRL.CLIENT`
- Headless in-process session over a project
- Launching the app in auto mode and waiting for its control channel
- Equality of tool lists across modes

**Out:**
- The setting's storage and its A0 change command (`CORE.STO`)
- App launch mechanics per OS (`PLAT`)
- Control-channel protocol (`AUTO.CTRL`)

## Requirements

- **REQ-AUTO.MCP.MODES-001** In bridge mode, every tool call shall travel over the authenticated control channel, and if no authenticated app is reachable within 5 s [unverified], then the server shall fail with `app_unreachable` before listing tools.
- **REQ-AUTO.MCP.MODES-002** In headless mode, the server shall run an in-process session with no window and the same tool list as bridge mode for the same manifest digest and profile.
- **REQ-AUTO.MCP.MODES-003** With no mode flag, the server shall use bridge mode when an authenticated app is reachable, and otherwise act on the "app closed" setting: headless, launch the app and then bridge, or refuse with `app_unreachable`.
- **REQ-AUTO.MCP.MODES-004** In every mode, the tool list shall contain no M1, S1 or A0 command.

## Interfaces

- **Offers:** `fid mcp [--bridge | --headless] [--server <profile>]`.
- **Consumes:** `AUTO.MCP.SESS`; `AUTO.CTRL.CLIENT`; CORE settings (read); `PLAT` app launch.

## Decomposition hints

SELECT (mode selection and setting), BRIDGE (control-channel transport), HEADLESS (in-process session), LAUNCH (launch and wait). Headless can be a P0 leaf; bridge needs `AUTO.CTRL` (P1a).

## Open questions

- If a headless MCP session and a running app both want the same project, the locking rules of `AUTO.CLI.TARGET` should apply; one shared rule may belong in `CORE.STO`. `AUTO.MCP.MODES.HEADLESS` applies the same rule (write lock if free, read-only otherwise, W-class calls refused with `conflict`).
- In P0, before `AUTO.MCP.MODES.SELECT` (P1a) exists, `fid mcp` without a flag runs headless.
- The "app closed" setting is declared by `AUTO.MCP.MODES.SELECT` as `mcp.app_closed_action` with the admin flag, so it changes only through the A0 settings command.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| AUTO.MCP.MODES.HEADLESS | Headless in-process MCP session (leaf, P0) | 002 004 | none |
| AUTO.MCP.MODES.BRIDGE | Bridge mode over the control channel (leaf, P1a) | 001 004 | none |
| AUTO.MCP.MODES.SELECT | Auto mode selection, app-closed setting and app launch (leaf, P1a) | 003 | none |
