---
id: AUTO.PROG.INTERP.MARK
title: Mark and execute selected steps
level: 4
parent: AUTO.PROG.INTERP
kind: feature
status: leaf
phase: P1b
size: S
owners: []
claimed_by:
satisfies: [REQ-AUTO.PROG.INTERP-005]
covers: [PCD.PRG.09]
depends_on: [AUTO.PROG.INTERP.ENGINE, AUTO.PROG.INTERP.DEBUG, AUTO.PROG.FLOW]
side_effect: W2
determinism: D0
cli: [fid program execute-marked]
mcp: [program_execute_marked]
journal: [program.execute_marked@1]
updated: 2026-11-17
---

# AUTO.PROG.INTERP.MARK: Mark and execute selected steps

## Purpose

Lets a user mark a subset of a program's steps (for example, re-measure three features) and execute only those, either fresh or inside a debug session whose already-executed steps supply variables. Before anything runs, a read-before-define analysis over the marked subset refuses the request if a marked step reads a variable that no marked or already-executed step defines.

## Scope

**In:**
- `program.execute_marked@1` (W2): validation, dependency analysis, execution of the subset in program order
- Read/write variable sets per step (from typed expressions and invoke bindings)
- Optional `session` parameter linking to a debug session's executed steps and variables
- Rules for marked steps inside loops, branches and subroutines

**Out:**
- Engine and hooks (`AUTO.PROG.INTERP.ENGINE`); debug sessions (`AUTO.PROG.INTERP.DEBUG`)
- Selecting steps in the editor (`AUTO.PROG.EDIT.VIEWS`, `VIZ`)

## Requirements

- **REQ-AUTO.PROG.INTERP.MARK-001** When `program.execute_marked@1` is invoked, the system shall execute only the marked steps, in program order, through the engine under the caller's principal.
- **REQ-AUTO.PROG.INTERP.MARK-002** If a marked step reads a variable that no earlier marked step defines and that is not defined in the linked session's context, then the command shall fail with `unbound_variable` naming the step and variable, before any step runs.
- **REQ-AUTO.PROG.INTERP.MARK-003** A marked loop, branch or call step shall run with its whole body; marking a step inside a loop body without marking the loop shall be refused with `partial_control_block`.
- **REQ-AUTO.PROG.INTERP.MARK-004** If a marked step ID is not in the program, then the command shall fail with `unknown_step` and run nothing.
- **REQ-AUTO.PROG.INTERP.MARK-005** The command shall journal one `program.execute_marked@1` entry with the program digest, the marked step IDs, the session ID if any and the run end, in addition to the entries of the invoked commands.

## Acceptance criteria

- **AC-1** Given fixture program `fixtures/program/interp/marked.json` (10 steps) and marks on steps 2, 5 and 7 whose reads are defined by 2 and 5, when `fid program execute-marked --program root:in/marked.json --steps S2,S5,S7 --json` runs on the fake bus, then exactly those three invokes are sent, in order, and one `program.execute_marked@1` entry is journaled.
- **AC-2** (error path) Given marks on 5 and 7 only (7 reads a variable defined by 2), then `unbound_variable` naming step 7 and the variable, and the fake bus receives nothing.
- **AC-3** Given the same marks on 5 and 7 with a debug session in which step 2 already executed, then the command succeeds.
- **AC-4** (error path) Given a mark on a step inside a loop without the loop, then `partial_control_block`; given an unknown step ID, `unknown_step`.

## Design

- **Registry command(s):** `program.execute_marked@1`, W2. Params: `program: ProgramRef`, `steps: Vec<StepId>` (1..10 000), `binding: Option<Binding>`, `session: Option<DebugSessionId>`. Result: `{ run_end, executed: Vec<StepKey> }`.
- **Data structures:** `StepIo { reads: BTreeSet<VarName>, writes: BTreeSet<VarName> }` computed from the type report.
- **Algorithm:** validate IDs and control-block completeness; forward pass over marked steps in program order accumulating defined variables (seeded by the session context); then run the engine with a filter hook that returns `Allow` only for marked steps and their bodies.
- **Errors:** `unbound_variable`, `partial_control_block`, `unknown_step`, `program_check_failed`, `session_not_found`.
- **Determinism:** D0 for the invocation sequence.
- **Performance budget:** analysis of 10 000 steps < 20 ms [unverified until measured].
- **Crate:** `fid-program` (module `interp::mark`).

## Test fixtures

`fixtures/program/interp/marked.json` (project-authored, CC-BY-4.0); fake bus; a debug session fixture from `AUTO.PROG.INTERP.DEBUG` tests.

## Build prompt

> In crate `fid-program`, add module `interp::mark` and register `program.execute_marked@1` (W2; CLI `fid program execute-marked`, MCP `program_execute_marked`). Validate step IDs (`unknown_step`) and that marked steps inside loop/branch/subroutine bodies have their enclosing block marked (`partial_control_block`). Compute read/write variable sets per step from the type report; walk the marked steps in program order, seeding defined variables from the linked debug session (if any), and fail with `unbound_variable` (step, variable) before running anything. Then run the engine under the caller's principal with a filter hook allowing only marked steps and their bodies. Journal one entry with program digest, marked IDs, session ID and run end. Acceptance: AC-1..AC-4. Constraints: no panic, no `unsafe`, deny `unwrap`/`expect`/`panic`/`todo`/`unimplemented`/`unreachable`; `BTreeSet` only.

## Open questions

- `program.execute_marked` is not yet in `AUTO.PROG`'s `cli`/`mcp`/`journal` lists.

## Children

_Leaf: none._
