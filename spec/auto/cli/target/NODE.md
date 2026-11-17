---
id: AUTO.CLI.TARGET
title: Session targeting: headless project or running app
level: 3
parent: AUTO.CLI
kind: module
status: open
phase: P0
size: L
owners: []
claimed_by:
satisfies: [REQ-AUTO.CLI-001, REQ-AUTO.CLI-006]
covers: []
depends_on: [AUTO.SURF.JOBS, AUTO.CTRL.CLIENT]
cli: [fid --project *, fid --app *]
mcp: []
journal: []
updated: 2026-11-16
---

# AUTO.CLI.TARGET: Session targeting: headless project or running app

## Purpose

Decides where a `fid` command runs: in an in-process headless session over a project file, or inside a running app through the control channel. Both targets must give the same command set and the same envelope, so a script behaves the same whether or not the GUI is open.

## Scope

**In:**
- `--project <handle or path>` headless target: open, lock, run, close
- `--app` target: discover and authenticate to a running app via the control-channel client
- Default selection rules and the `FID_PROJECT` environment variable
- Project locking when a running app already holds the project
- Transport adapter so `AUTO.SURF.JOBS` works over both targets

**Out:**
- Control-channel protocol and client (`AUTO.CTRL`)
- Project open, storage and locks (`CORE.PRJ`, `CORE.STO`)
- MCP modes (`AUTO.MCP.MODES`)

## Requirements

- **REQ-AUTO.CLI.TARGET-001** When `--project` is given, the CLI shall run the command in an in-process headless session on that project and release the project on exit, including on interrupt.
- **REQ-AUTO.CLI.TARGET-002** When `--app` is given, the CLI shall run the command in the running app through the authenticated control channel, and if none is reachable within 5 s [unverified], then it shall exit 6 (`not_run`).
- **REQ-AUTO.CLI.TARGET-003** If the project is held open by a running app, then a headless `--project` invocation of a state-changing command shall be refused with `conflict` naming the app, and shall not open a second writer.
- **REQ-AUTO.CLI.TARGET-004** For the same manifest digest, the command set and envelope shape shall be identical for both targets.
- **REQ-AUTO.CLI.TARGET-005** When neither flag is given, the CLI shall use `FID_PROJECT` if set, and otherwise run only commands that need no project.

## Interfaces

- **Offers:** target selection for every `fid` command.
- **Consumes:** `AUTO.CTRL.CLIENT`; CORE project open and lock; `AUTO.SURF.JOBS` transports.

## Decomposition hints

HEADLESS (in-process session and locking), APP (control-channel target), SELECT (defaults and environment). Headless can be built in P0; the app target waits for `AUTO.CTRL` (P1a).

## Open questions

- Whether `--project` with a raw path is allowed depends on D-012 (CLI principal kind).
- Whether a headless session may open a project read-only while the app holds it for writing (for R0 queries) depends on `CORE.STO` locking.

## Children

_None yet: status `open`._
