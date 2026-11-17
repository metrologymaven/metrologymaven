---
id: AUTO.SCRIPTED.GRAPH
title: Recompute of scripted elements in the dependency graph
level: 3
parent: AUTO.SCRIPTED
kind: module
status: open
phase: P1b
size: L
owners: []
claimed_by:
satisfies: [REQ-AUTO.SCRIPTED-002]
covers: [GOM.AUT.03]
depends_on: [AUTO.SCRIPTED.DECL, AUTO.SCRIPTED.FAIL, CORE.DEP, AUTO.PY.HOST, AUTO.PROG.INTERP]
cli: [fid script recompute]
mcp: [script_recompute]
journal: [script.recompute]
updated: 2026-11-16
---

# AUTO.SCRIPTED.GRAPH: Recompute of scripted elements in the dependency graph

## Purpose

Makes scripted elements and checks recompute like built-in ones: when an input changes or the implementation version changes, CORE.DEP schedules them in its deterministic order, the script runs out of process (or the program runs in the interpreter) under the recompute guard, and the outputs join the single journal entry CORE.DEP writes for the whole recompute. `script.recompute@1` forces a recompute of chosen scripted elements.

## Scope

**In:**
- Recompute hook that CORE.DEP calls for scripted element types
- Passing input element values to the script and reading outputs back
- Version-change trigger (implementation upgraded by a human)
- `script.recompute@1`

**Out:**
- Recompute ordering and journaling (`CORE.DEP`)
- Guard, limits and failure states (`AUTO.SCRIPTED.FAIL`)
- Result labelling (`AUTO.SCRIPTED.RESULT`)

## Requirements

- **REQ-AUTO.SCRIPTED.GRAPH-001** When a declared input changes, the scripted element shall be recomputed in the same recompute pass as other dependents and its outputs shall be part of that pass's single journal entry.
- **REQ-AUTO.SCRIPTED.GRAPH-002** When a human changes the implementation version of a scripted element, the element and all its dependents shall be recomputed.
- **REQ-AUTO.SCRIPTED.GRAPH-003** Given the same inputs and implementation digest, a recompute shall pass the script byte-identical input payloads.
- **REQ-AUTO.SCRIPTED.GRAPH-004** `script.recompute@1` shall recompute only the named scripted elements and their dependents.

## Interfaces

- **Offers:** recompute hook for CORE.DEP; `script.recompute@1`.
- **Consumes:** CORE.DEP; `AUTO.PY.HOST`; `AUTO.PROG.INTERP`; `AUTO.SCRIPTED.FAIL`.

## Decomposition hints

HOOK (CORE.DEP integration), IO (input payload and output mapping), CMD (`script.recompute@1`).

## Open questions

- CORE.DEP journals a recompute as one entry; an out-of-process script runs for seconds inside it. Whether a recompute pass may wait on an external process, and how a cancelled recompute rolls back, needs the `CORE.DEP` contract (open).

## Children

_None yet: status `open`._
