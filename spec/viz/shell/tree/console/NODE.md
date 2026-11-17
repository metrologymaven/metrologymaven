---
id: VIZ.SHELL.TREE.CONSOLE
title: Python console host panel
level: 4
parent: VIZ.SHELL.TREE
kind: task
status: leaf
phase: P1b
size: S
owners: []
claimed_by:
satisfies: [REQ-VIZ.SHELL.TREE-001, REQ-VIZ.SHELL.TREE-005]
covers: []
depends_on: [VIZ.SHELL.DOCK.HOST, AUTO.PY.HOST]
side_effect: R0
determinism: N
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# VIZ.SHELL.TREE.CONSOLE: Python console host panel

## Purpose

A dockable panel that hosts the Python console provided by AUTO.PY: an input line, a scrollback of output, and a run indicator. The console runs in the out-of-process Python host as a `script` principal; every state change it causes is a registry command made by that host, never by the panel.

## Scope

**In:** panel `viz.console`; input editing with history; scrollback rendering of host output as untrusted plain text; cancel button; principal and session label.
**Out:** the Python host process, protocol and SDK (`AUTO.PY.HOST`, `AUTO.PY.PROTO`); command execution (`CORE.BUS`).

## Requirements

- **REQ-VIZ.SHELL.TREE.CONSOLE-001** The panel shall send input to the AUTO.PY host session and shall have no other path to the engine.
- **REQ-VIZ.SHELL.TREE.CONSOLE-002** Output from the host shall be rendered as untrusted plain text with control sequences stripped, limited to the last 10 000 lines.
- **REQ-VIZ.SHELL.TREE.CONSOLE-003** The panel shall show the session's principal kind (`script`, or `human` after an explicit upgrade) at all times.

## Acceptance criteria

- **AC-1** Given the AUTO.PY test host, when `print("\x1b[31mred")` runs, then the panel shows `red` without the escape sequence.
- **AC-2** Given a script calling a W2 command, then the journal entry's principal is `script`, and the panel shows `script`.
- **AC-3** Given 20 000 output lines, then the scrollback holds the last 10 000.

## Design

- **Registry command(s):** none.
- **Data structures:** `ConsoleModel { history, scrollback: RingBuffer<Untrusted<String>>, principal }`.
- **Errors:** host disconnect shown inline.
- **Determinism:** N.
- **Performance budget:** append 1 000 lines/s without frame drops on H1 [unverified].
- **Crate:** `fid-shell` (module `tree::console`).

## Test fixtures

AUTO.PY test host.

## Build prompt

> In crate `fid-shell`, add `tree::console`: a panel (`viz.console`) hosting an AUTO.PY console session: input line with history, scrollback of host output as untrusted plain text with control sequences stripped (last 10 000 lines), cancel, and a permanent principal label. The panel's only engine path is the Python host session. Tests AC-1..AC-3 with the AUTO.PY test host. Rules: CONSTITUTION §3 (script principal; D-012 upgrade only by explicit confirmation), §8; C6.

## Open questions

- Tagged P1b with AUTO.PY.

## Children

_Leaf: none._
