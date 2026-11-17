---
id: AUTO.PROG.FLOW
title: Loops, branches, jumps and subroutines
level: 3
parent: AUTO.PROG
kind: module
status: open
phase: P1b
size: L
owners: []
claimed_by:
satisfies: [REQ-AUTO.PROG-006]
covers: [PCD.PRG.03, PCD.PRG.04, PCD.PRG.05]
depends_on: [AUTO.PROG.FMT, AUTO.PROG.EXPR]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# AUTO.PROG.FLOW: Loops, branches, jumps and subroutines

## Purpose

The semantics of control flow in Command Programs: counted and conditional loops with mandatory iteration limits, if / else-if / else, labelled jumps confined to one subroutine, and subroutines with typed parameters and return values. FLOW defines what each construct means and what is statically legal; the interpreter executes it.

## Scope

**In:**
- Semantics of `loop`, `if`, `label`, `jump`, `call`, `return`
- Static rules: jump targets in the same subroutine, no jump into a loop body, every non-void subroutine path returns, recursion depth limit declaration
- Reachability analysis (unreachable steps reported to the checker)
- Iteration-limit and call-depth semantics (stop conditions the interpreter enforces)

**Out:**
- Step syntax (`AUTO.PROG.FMT`); runtime enforcement and journaling of stops (`AUTO.PROG.INTERP`)

## Requirements

- **REQ-AUTO.PROG.FLOW-001** Every loop shall declare `max_iterations`, and a loop without it shall be rejected by the checker.
- **REQ-AUTO.PROG.FLOW-002** A jump shall target a label in the same subroutine and outside any loop body it is not already inside, and any other jump shall be rejected with `illegal_jump`.
- **REQ-AUTO.PROG.FLOW-003** A subroutine call shall bind arguments to typed parameters, and a call with a missing or ill-typed argument shall be rejected with `bad_call`.
- **REQ-AUTO.PROG.FLOW-004** Every path through a subroutine that declares a return type shall end in `return` with a value of that type.
- **REQ-AUTO.PROG.FLOW-005** The analysis shall report every step that no path from the program entry can reach.

## Interfaces

- **Offers:** control-flow graph and static rules to `AUTO.PROG.CHECK`; semantics to `AUTO.PROG.INTERP`.
- **Consumes:** `AUTO.PROG.FMT`; `AUTO.PROG.EXPR` (condition types).

## Decomposition hints

CFG (control-flow graph and reachability), LOOPS, CALLS (subroutines and returns), JUMPS.

## Open questions

- Whether recursion is allowed at all. Proposed: allowed up to the call-depth limit (64 [unverified]) enforced by the interpreter.

## Children

_None yet: status `open`._
