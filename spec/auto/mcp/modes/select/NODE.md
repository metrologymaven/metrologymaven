---
id: AUTO.MCP.MODES.SELECT
title: Auto mode selection, app-closed setting and app launch
level: 4
parent: AUTO.MCP.MODES
kind: task
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-AUTO.MCP.MODES-003]
covers: []
depends_on: [AUTO.MCP.MODES.HEADLESS, AUTO.MCP.MODES.BRIDGE, AUTO.CTRL.CLIENT, CORE.STO.SET]
side_effect: R0
determinism: N
cli: [fid mcp]
mcp: []
journal: []
updated: 2026-11-17
---

# AUTO.MCP.MODES.SELECT: Auto mode selection, app-closed setting and app launch

## Purpose

`fid mcp` with no mode flag: use bridge mode when an authenticated app is reachable, and otherwise act on the user's "app closed" setting (`headless`, `launch` the app and then bridge, or `refuse`). This node declares that setting (admin-flagged, so an agent cannot change its own mode policy), reads it, and launches the app as an OS action, which is not a registry command (CONSTITUTION §3.1, D-031).

## Scope

**In:**
- Declaring setting `mcp.app_closed_action` (`headless` | `launch` | `refuse`, default `refuse`, user scope, admin flag)
- Mode resolution order: explicit flag, reachable app, setting
- Launching the configured desktop app and waiting for its control channel
- Flag validation (`--bridge` and `--headless` together)

**Out:**
- Settings storage and the A0 change command (`CORE.STO.SET`)
- The two modes themselves (`AUTO.MCP.MODES.HEADLESS`, `AUTO.MCP.MODES.BRIDGE`)
- Install locations of the desktop apps (`PLAT`)

## Requirements

- **REQ-AUTO.MCP.MODES.SELECT-001** The node shall declare setting `mcp.app_closed_action` with values `headless`, `launch` and `refuse`, default `refuse`, user scope and the admin flag, so it changes only through the A0 command.
- **REQ-AUTO.MCP.MODES.SELECT-002** If both `--bridge` and `--headless` are given, then `fid mcp` shall exit 2 with `mode_conflict` before any MCP handshake.
- **REQ-AUTO.MCP.MODES.SELECT-003** When no mode flag is given and an authenticated app is reachable within 2 s [unverified], `fid mcp` shall start bridge mode.
- **REQ-AUTO.MCP.MODES.SELECT-004** When no mode flag is given and no app is reachable, `fid mcp` shall start headless mode if the setting is `headless`, and shall fail with `app_unreachable` if it is `refuse`.
- **REQ-AUTO.MCP.MODES.SELECT-005** When no app is reachable and the setting is `launch`, `fid mcp` shall start the configured desktop app as an OS process, wait up to 30 s [unverified] for an authenticated control channel, then start bridge mode, or fail with `app_launch_timeout`.
- **REQ-AUTO.MCP.MODES.SELECT-006** Starting the app shall not be a registry command and shall pass no project path or other argument taken from an agent.
- **REQ-AUTO.MCP.MODES.SELECT-007** The resolved mode and the reason for it shall be written to stderr and reported in the MCP server's initialise metadata.

## Acceptance criteria

- **AC-1** Given the table `fixtures/mcp/modes/select-cases.json` (flags × app reachable × setting value), when `resolve_mode` runs with a fake probe and fake settings, then each row yields the expected mode or error.
- **AC-2** Given the test app running and no flags, then `fid mcp` starts in bridge mode and its initialise metadata says `mode: bridge, reason: app_reachable`.
- **AC-3** (error path) Given no app and setting `refuse`, then `fid mcp` fails with `app_unreachable`; given `--bridge --headless`, exit 2 `mode_conflict`.
- **AC-4** Given no app, setting `launch` and a fake launcher that starts the test app after 1 s, then bridge mode starts and the launcher received no arguments; given a launcher whose app never opens the channel and a 1 s test timeout, then `app_launch_timeout`.
- **AC-5** (error path) Given an MCP agent session calling `settings_set_value` for `mcp.app_closed_action`, then the call is refused because the key is admin-flagged, and no tool for the A0 admin command exists.

## Design

- **Registry command(s):** none; declares one setting (`CORE.STO.SET` declaration API).
- **Data structures:** `enum AppClosedAction { Headless, Launch, Refuse }`; `ModeChoice { mode: Bridge | Headless, reason: Flag | AppReachable | Setting }`; `trait AppLauncher { fn launch(&self) -> Result<(), LaunchError>; }` with the OS implementation reading the install location from PLAT-provided configuration.
- **Algorithm:** flags → conflict check → explicit mode; else probe with `CtrlClient` (2 s) → bridge; else read setting → headless | refuse | launch-and-wait (poll discovery file and authenticate every 250 ms up to 30 s).
- **Errors:** `mode_conflict` (exit 2), `app_unreachable`, `app_launch_timeout`, `app_launch_failed`.
- **Determinism:** N.
- **Performance budget:** probe adds at most 2 s when no app runs [unverified until measured].
- **Crate:** `fid-mcp` (module `modes::select`).

## Test fixtures

`fixtures/mcp/modes/select-cases.json` (project-authored decision table, CC-BY-4.0); fake probe, fake settings and fake launcher; the `AUTO.CTRL` test app.

## Build prompt

> In crate `fid-mcp`, add `modes::select`. Declare setting `mcp.app_closed_action` (`headless` | `launch` | `refuse`, default `refuse`, user scope, admin flag) through the `CORE.STO.SET` declaration API. Implement `resolve_mode(flags, probe, settings, launcher)`: `--bridge` with `--headless` → exit 2 `mode_conflict`; an explicit flag wins; otherwise probe for an authenticated app for 2 s → bridge; otherwise apply the setting: `headless` → headless mode, `refuse` → `app_unreachable`, `launch` → start the desktop app as an OS process with no agent-supplied arguments (not a registry command) and wait up to 30 s for an authenticated control channel, then bridge, else `app_launch_timeout`. Write the chosen mode and reason to stderr and the MCP initialise metadata. Acceptance: AC-1..AC-5 with `fixtures/mcp/modes/select-cases.json`, fakes and the control-channel test app. Constraints: no panic, no `unsafe`, deny `unwrap`/`expect`/`panic`/`todo`/`unimplemented`/`unreachable`; agents can never change the setting (admin flag → A0, never on MCP).

## Open questions

- Where the desktop app's executable is found per OS is not specified by any PLAT node yet; this leaf takes it through the `AppLauncher` trait.
- With several focused apps installed (D-004), `launch` needs to know which one; proposed default: the app whose MCP profile matches `--server`, else Inspect. Needs `ECO.APPS` agreement.

## Children

_Leaf: none._
