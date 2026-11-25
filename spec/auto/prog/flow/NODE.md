---
id: AUTO.PROG.FLOW
title: Loops, branches, jumps and subroutines
level: 3
parent: AUTO.PROG
kind: task
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
built_in: [fid-program]
built_pr: 626
satisfies: [REQ-AUTO.PROG-006]
covers: [PCD.PRG.03, PCD.PRG.04, PCD.PRG.05]
depends_on: [AUTO.PROG.FMT, AUTO.PROG.EXPR.TYPES]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# AUTO.PROG.FLOW: Loops, branches, jumps and subroutines

## Purpose

The static semantics of control flow in Command Programs: counted and conditional loops with mandatory iteration limits, if / else-if / else, labelled jumps confined to one subroutine, and subroutines with typed parameters and return values. FLOW builds the control-flow graph (CFG) of each subroutine, applies the static rules, reports unreachable steps, and publishes the CFG and the run-time stop rules that the interpreter executes.

## Scope

**In:**
- CFG construction per subroutine from `AUTO.PROG.FMT` step kinds `loop`, `if`, `label`, `jump`, `call`, `return`
- Static rules: `max_iterations` present and in range, jump targets, call binding, return paths, label uniqueness
- Reachability analysis from the program entry (main body plus called subroutines)
- Call graph and recursion detection (reported as information, not an error)
- The specification of run-time stops (`iteration_limit`, `depth_limit`) that `AUTO.PROG.INTERP.ENGINE` enforces

**Out:**
- Step syntax (`AUTO.PROG.FMT`); condition and argument typing (`AUTO.PROG.EXPR.TYPES`)
- Executing control flow and journaling stops (`AUTO.PROG.INTERP.ENGINE`)
- Ordering diagnostics across all modules (`AUTO.PROG.CHECK`)

## Requirements

- **REQ-AUTO.PROG.FLOW-001** If a loop lacks `max_iterations` or declares a value outside 1 to 1 000 000 [unverified], then the analysis shall report `missing_iteration_limit` or `bad_iteration_limit` at the loop's step ID.
- **REQ-AUTO.PROG.FLOW-002** A loop condition and an `if` branch condition shall have type `bool`, and a counted loop's bounds shall be `int`, otherwise the analysis shall report `bad_condition` or `bad_loop_bounds`.
- **REQ-AUTO.PROG.FLOW-003** If a jump targets a label in another subroutine, a label inside a loop or branch body the jump is not itself inside, or an undefined label, then the analysis shall report `illegal_jump` naming the jump and target.
- **REQ-AUTO.PROG.FLOW-004** If two labels in one subroutine share a name, then the analysis shall report `duplicate_label`.
- **REQ-AUTO.PROG.FLOW-005** If a call names an unknown subroutine, omits a parameter without default, passes an extra argument or an argument of the wrong type, then the analysis shall report `bad_call` naming the parameter.
- **REQ-AUTO.PROG.FLOW-006** If a path through a subroutine with a declared return type can reach its end without a `return` of that type, or a void subroutine returns a value, then the analysis shall report `missing_return` or `bad_return` at the subroutine.
- **REQ-AUTO.PROG.FLOW-007** The analysis shall report `unreachable_step` (severity warning) for every step that no path from the program entry reaches, treating loop and branch conditions as unknown.
- **REQ-AUTO.PROG.FLOW-008** The analysis shall report each recursive cycle in the call graph as `recursion` (severity info); recursion is legal and bounded at run time by the call-depth limit of 64 [unverified].
- **REQ-AUTO.PROG.FLOW-009** The published CFG shall give, for each step, its successor steps with edge kinds (`next`, `true`, `false`, `loop_back`, `loop_exit`, `jump`, `call`, `return`), so the interpreter and the editor use one definition of control flow.

## Acceptance criteria

- **AC-1** Given fixture programs `fixtures/program/flow/valid/*.json` (nested loops, else-if chains, early return, recursion to depth 3), when `analyse_flow` runs, then there are no errors, `recursion` is reported once for the recursive fixture, and the CFG equals the expected `*.cfg.json`.
- **AC-2** (error path) Given `fixtures/program/flow/jump-into-loop.json`, `jump-other-sub.json` and `jump-undefined.json`, then each yields one `illegal_jump` naming the jump step and label.
- **AC-3** (error path) Given a loop without `max_iterations`, then `missing_iteration_limit`; given `max_iterations: 0`, then `bad_iteration_limit`.
- **AC-4** (error path) Given a subroutine `area(w: length, h: length) -> area` with a path ending without `return`, then `missing_return`; given a call `area(w = 3[mm])`, then `bad_call` naming `h`.
- **AC-5** Given `fixtures/program/flow/dead-code.json` (steps after an unconditional jump), then `unreachable_step` for exactly those step IDs.
- **AC-6** (determinism) Given the same program, the diagnostics and CFG are byte-identical across runs and platforms.

## Design

- **Registry command(s):** none; library `analyse_flow(&Program, &TypeReport) -> FlowReport`.
- **Data structures:** `Cfg { subroutine: SubId, nodes: Vec<StepId>, edges: Vec<(StepId, StepId, EdgeKind)> }`; `CallGraph { edges: BTreeSet<(SubId, SubId)> }`; `FlowReport { cfgs: BTreeMap<SubId, Cfg>, call_graph, diagnostics: Vec<Diag> }`.
- **Algorithm:** structured lowering of nested steps into a CFG per subroutine; label table per subroutine with the enclosing-body path of each label and jump to check legality; reachability by worklist from entry over CFG and call edges; return-path check by backward dataflow; Tarjan SCC on the call graph for recursion.
- **Errors (diagnostics):** `missing_iteration_limit`, `bad_iteration_limit`, `bad_condition`, `bad_loop_bounds`, `illegal_jump`, `duplicate_label`, `bad_call`, `missing_return`, `bad_return`, `unreachable_step` (warning), `recursion` (info).
- **Determinism:** D0; `BTreeMap`/`BTreeSet` only, iteration in step order.
- **Performance budget:** 10 000-step program analysed in < 50 ms [unverified until measured].
- **Crate:** `fid-program` (module `flow`).

## Test fixtures

Project-authored programs under `fixtures/program/flow/` with expected CFGs and diagnostics (CC-BY-4.0).

## Build prompt

> In crate `fid-program`, add module `flow` with `analyse_flow(program, type_report) -> FlowReport`. Build one control-flow graph per subroutine from the FMT step kinds (loop, if, label, jump, call, return) with edge kinds next, true, false, loop_back, loop_exit, jump, call, return. Rules: every loop has `max_iterations` in 1..=1 000 000; conditions are bool and counted-loop bounds int (types from `expr::types`); jumps only to a label of the same subroutine that is not inside a loop or branch body the jump is outside of; unique labels; calls bind every parameter without default, no extras, right types (`bad_call`); every path of a non-void subroutine returns a value of its type. Report unreachable steps (warning) by worklist reachability from the program entry, and recursive call cycles (info) by Tarjan SCC; recursion is legal and bounded at run time by depth 64. Diagnostics carry step ID; output ordered by step position. Acceptance: AC-1..AC-6 with `fixtures/program/flow/`. Constraints: no panic, no `unsafe`, deny `unwrap`/`expect`/`panic`/`todo`/`unimplemented`/`unreachable`; no unbounded recursion in the analysis itself (explicit stacks); `BTreeMap` only.

## Open questions

- The target's recursion question is answered: recursion is allowed and bounded at run time by the call-depth limit (REQ-AUTO.PROG-007); the analysis only informs.
- Error-handling edges (`on_error: goto`) are not in FMT yet; if FMT adds them (see `AUTO.PROG.INTERP.CALL`), add an `error` edge kind here.
- **Unbounded backward-jump loops BLOCK `AUTO.PROG.INTERP.ENGINE`.** `Analysis::resolve` (crates/fid-program/src/flow.rs:678) accepts a jump to a label at or before the jump, so `label; ...; jump` forms a cycle with no iteration limit (the `duplicate-label` fixture is one). Maintainer decision on the direction: a future REQ-AUTO.PROG.FLOW-010 makes a jump whose label is at or before the jump in document order a `backward_jump` error. Then every cycle inside a unit goes through a `loop_back` edge, bounded by the iteration limits, and recursion is bounded by the call depth of 64. It lands in a spec PR before ENGINE is built.
- **Definite assignment** (follow-up from #418, AUTO.PROG.EXPR.TYPES): whether every use of a variable is preceded by an assignment on every path is a control-flow question for this node, but no REQ or AC here covers it and it is not built. It needs a REQ, here or in `AUTO.PROG.CHECK`.
- **Obligation on `AUTO.PROG.CHECK`:** when `Program::parse` returns `schema_invalid`, call `iteration_limits` on the raw bytes, and report `bad_iteration_limit` only once per loop. Today a limit above 1 000 000 is reported by both `analyse_flow` (flow.rs:591) and `iteration_limits` (flow.rs:983).

## Children

_Leaf: none._
