---
id: AUTO.PY.RUN
title: Running scripts from every surface and principal assignment
level: 3
parent: AUTO.PY
kind: module
status: decision
decision: D-012
phase: P1b
size: L
owners: []
claimed_by:
satisfies: [REQ-AUTO.PY-005, REQ-AUTO.PY-006]
covers: [GOM.AUT.01]
depends_on: [AUTO.PY.HOST]
cli: [fid script run]
mcp: [script_run]
journal: [script.run]
updated: 2026-11-16
---

# AUTO.PY.RUN: Running scripts from every surface and principal assignment

## Purpose

The `script.run@1` command: start a registered or released script (or, for a human, a draft) from the GUI, CLI or MCP, decide the principal it runs under, and keep M1 and S1 out of every script whatever its principal.

## Scope

**In:**
- `script.run@1` (and its job), parameters and result
- Principal assignment by starter: agent or automation → `agent` limits; human → per D-012
- Refusal or redirection of M1 and S1 steps
- Lifecycle checks against `AUTO.REC` (which states may run, by whom)

**Out:**
- Process management (`AUTO.PY.HOST`)
- Lifecycle states (`AUTO.REC.LIFE`)

## Requirements

- **REQ-AUTO.PY.RUN-001** When an agent or automation starts a script, the run shall execute under an automation principal with agent side-effect limits.
- **REQ-AUTO.PY.RUN-002** When a human starts a script, the run shall execute under the principal kind that D-012 assigns to a human-started script.
- **REQ-AUTO.PY.RUN-003** The principal of a run shall be fixed at start and shall not change while the run lasts.
- **REQ-AUTO.PY.RUN-004** If a script invokes an M1 or S1 command, then the run shall receive `NotAllowedFromScripts` and nothing shall execute, whatever the run's principal.

## Interfaces

- **Offers:** `script.run@1` (class W2, MCP named tool `script_run`).
- **Consumes:** `AUTO.PY.HOST`; `AUTO.REC` lifecycle state; CORE bus principals.

## Decomposition hints

CMD (the `script.run@1` command and job), PRIN (principal assignment table). Likely one M leaf once D-012 is decided.

## Open questions

- **Blocked on D-012.** REQ-AUTO.PY-005 says a human-started script runs under the human's principal; REQ-CORE.BUS-002 says every CLI and Python caller is `script`. Until D-012 decides, REQ-AUTO.PY.RUN-002 cannot be made concrete.

## Children

_None yet: status `decision`._
