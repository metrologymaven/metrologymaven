---
id: ECO.SDK.LIFE
title: Plugin lifecycle commands
level: 3
parent: ECO.SDK
kind: module
status: open
phase: P1b
size: L
owners: []
claimed_by:
satisfies: [REQ-ECO.SDK-006, REQ-ECO.SDK-009]
covers: []
depends_on: [ECO.SDK.PKG, ECO.SDK.SIGN, ECO.SDK.API, CORE.BUS]
cli: [fid plugin *]
mcp: [plugin_list, plugin_inspect]
journal: [plugin.*]
updated: 2026-11-16
---

# ECO.SDK.LIFE: Plugin lifecycle commands

## Purpose

The registry commands that list, inspect, install, enable, disable, update and uninstall plugins. Reads are R0 and visible to agents; every command that changes which plugin code runs is A0 (CONSTITUTION §3.1, D-007), never on MCP. The commands compose the package reader, the trust decision and the compatibility check into one journaled, offline workflow.

## Scope

**In:**
- `plugin.list@1` (R0), `plugin.inspect@1` (R0; manifest, signature state, capabilities, compatibility)
- `plugin.install@1`, `plugin.enable@1`, `plugin.disable@1`, `plugin.update@1`, `plugin.uninstall@1` (all A0)
- Install store layout, per-user vs machine-wide scope, atomic install and rollback on failure
- Behaviour of projects that reference a plugin that is missing or disabled (generic part; packs add their own rules)

**Out** (and which node owns it instead):
- Package reading (owned by `ECO.SDK.PKG`), trust decision (owned by `ECO.SDK.SIGN`), compatibility rule (owned by `ECO.SDK.API`)
- Capability grants and run-time isolation (owned by `ECO.SDK.ISO`)
- Pack-specific list and show displays (owned by `IND.CONTRACT`)

## Requirements

- **REQ-ECO.SDK.LIFE-001** Install, enable, disable, update and uninstall shall be registry commands of class A0, and no MCP tool shall be generated for them.
- **REQ-ECO.SDK.LIFE-002** List and inspect shall be R0 commands exposed on MCP as `plugin_list` and `plugin_inspect`.
- **REQ-ECO.SDK.LIFE-003** When enable is requested and the plugin's SDK range does not admit the host's SDK version, the command shall refuse and report the required range and the available version.
- **REQ-ECO.SDK.LIFE-004** If any step of install or update fails, then the previous installed state shall be restored and the failure journaled.
- **REQ-ECO.SDK.LIFE-005** Install and update shall work fully offline from a local package file or local mirror, with no account.
- **REQ-ECO.SDK.LIFE-006** Every lifecycle command that changes state shall be journaled with the plugin ID, version, package digest and the acting principal.

## Interfaces

- **Offers:** `plugin.*` commands; install store; plugin state query for other domains.
- **Consumes:** ECO.SDK.PKG reader; ECO.SDK.SIGN trust decision; ECO.SDK.API compatibility; CORE.BUS principals and side-effect enforcement; CORE.JRN.

## Decomposition hints

READ (list and inspect), INSTALL (install, update, rollback), STATE (enable, disable, uninstall), MISSING (projects referencing missing plugins).

## Open questions

- Machine-wide installs need administrator rights on most OSes; interaction with PLAT.DEPLOY's enterprise install modes is to be agreed (PLAT depends on ECO, so this node consumes PLAT's rules as data).

## Children

_None yet: status `open`._
