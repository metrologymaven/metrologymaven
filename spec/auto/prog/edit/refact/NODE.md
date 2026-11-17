---
id: AUTO.PROG.EDIT.REFACT
title: Structural edits: wrap, extract subroutine, rename variable
level: 4
parent: AUTO.PROG.EDIT
kind: feature
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-AUTO.PROG.EDIT-001, REQ-AUTO.PROG.EDIT-002]
covers: [PW.AUT.02]
depends_on: [AUTO.PROG.EDIT.OPS, AUTO.PROG.EXPR.TYPES, AUTO.PROG.FLOW]
side_effect: [W1, W1, W1]
determinism: D0
cli: [fid program edit-wrap-steps, fid program edit-extract-subroutine, fid program edit-rename-variable]
mcp: [program_edit_wrap_steps, program_edit_extract_subroutine, program_edit_rename_variable]
journal: [program.edit_wrap_steps@1, program.edit_extract_subroutine@1, program.edit_rename_variable@1]
updated: 2026-11-17
---

# AUTO.PROG.EDIT.REFACT: Structural edits: wrap, extract subroutine, rename variable

## Purpose

The three structural edits that turn a recorded macro into a maintainable program: wrap a contiguous run of steps in a loop or a branch, extract a contiguous run into a subroutine with inferred parameters and return value, and rename a variable everywhere it is visible. Each is a W1 `program.edit_*` command built on the edit-apply core of `AUTO.PROG.EDIT.OPS`, so the draft-only rule, step-ID preservation, provenance and journaling are identical.

## Scope

**In:**
- `program.edit_wrap_steps@1` (loop with `max_iterations`, or `if` with a condition)
- `program.edit_extract_subroutine@1` (parameters from variables read but not defined inside; return value from at most one variable defined inside and read after)
- `program.edit_rename_variable@1` (scope-aware rename in expressions and bindings)
- Preconditions using `AUTO.PROG.EXPR.TYPES` scopes and `AUTO.PROG.FLOW` jump rules

**Out:**
- The edit-apply core (`AUTO.PROG.EDIT.OPS`)
- Parameter lifting of recorded values (`AUTO.REC.LIFT`)

## Requirements

- **REQ-AUTO.PROG.EDIT.REFACT-001** `program.edit_wrap_steps@1` shall wrap a contiguous run of sibling steps in a new `loop` (with a required `max_iterations`) or `if` step whose body holds the same step IDs in the same order.
- **REQ-AUTO.PROG.EDIT.REFACT-002** If the steps to wrap or extract are not contiguous siblings, or a jump crosses the boundary of the run in either direction, then the command shall fail with `not_contiguous` or `jump_crosses_boundary` and change nothing.
- **REQ-AUTO.PROG.EDIT.REFACT-003** `program.edit_extract_subroutine@1` shall move the run into a new subroutine whose parameters are the variables the run reads but does not define, typed from the type report, and replace the run by one `call` step; the moved steps shall keep their step IDs.
- **REQ-AUTO.PROG.EDIT.REFACT-004** If more than one variable defined inside the extracted run is read after it, then extraction shall fail with `multiple_outputs` naming them and change nothing.
- **REQ-AUTO.PROG.EDIT.REFACT-005** `program.edit_rename_variable@1` shall rename every reference within the variable's scope and no reference outside it, and if the new name is already visible at any of those references, then it shall fail with `name_conflict`.
- **REQ-AUTO.PROG.EDIT.REFACT-006** For the same program and the same arguments, executing the program before and after an extract or rename with the same binding and command results shall issue the same invocation sequence apart from step keys of moved steps.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/program/edit/macro.json` (recorded 9-step macro), when `fid program edit-wrap-steps --program P --expected-digest <d> --steps S3..S6 --loop counted:1..4 --max-iterations 4` runs, then S3 to S6 sit in a new loop in the same order with unchanged IDs and the check passes.
- **AC-2** Given `edit-extract-subroutine` on S3..S6 that read `frame` and define `d` read later, then a new subroutine with parameter `frame` and return `d` exists, a `call` replaces the run, and an interpreter run with `fixtures/program/edit/fake-results.json` issues the same invocations as before.
- **AC-3** (error path) Given a run S3, S5 (not contiguous), then `not_contiguous`; given a run containing the target label of a jump outside it, then `jump_crosses_boundary`.
- **AC-4** (error path) Given an extraction where `d` and `e` are both read after the run, then `multiple_outputs` naming both.
- **AC-5** Given renaming `d` to `dist` where `dist` is already declared in an enclosing scope, then `name_conflict`; renaming to a free name changes exactly the references found by the type report.
- **AC-6** (replay) Given the journal of AC-1 to AC-5, exact replay is `IDENTICAL`.

## Design

- **Registry command(s):** all W1 on drafts. `program.edit_wrap_steps@1` (`program`, `expected_digest`, `steps`, `wrapper: Loop{kind, max_iterations} | If{condition}`), `program.edit_extract_subroutine@1` (`program`, `expected_digest`, `steps`, `name`), `program.edit_rename_variable@1` (`program`, `expected_digest`, `declaration_step`, `old`, `new`). Results as for `AUTO.PROG.EDIT.OPS`.
- **Data structures:** `RunSpan { block, start, end }`; uses `StepIo` read/write sets and scopes from the type report.
- **Algorithm:** validate span and jump boundaries with FLOW's label table; compute free variables (reads minus defines) and outputs (defines read after); rewrite expressions through the GRAMMAR AST printer; apply through `EditCore`.
- **Errors:** `not_contiguous`, `jump_crosses_boundary`, `multiple_outputs`, `name_conflict`, plus the core's errors.
- **Determinism:** D0.
- **Performance budget:** extract on a 10 000-step draft < 100 ms [unverified until measured].
- **Crate:** `fid-program` (module `edit::refactor`).

## Test fixtures

`fixtures/program/edit/macro.json` and `fake-results.json` (project-authored, CC-BY-4.0); fake bus.

## Build prompt

> In crate `fid-program`, add module `edit::refactor` with three W1 draft-only commands built on `edit::ops::EditCore`: `program.edit_wrap_steps@1` (wrap contiguous sibling steps in a loop with required `max_iterations` or an `if` with a condition), `program.edit_extract_subroutine@1` (parameters = variables read but not defined in the run, typed from the type report; at most one output, else `multiple_outputs`; replace the run with a `call`; moved steps keep IDs), `program.edit_rename_variable@1` (scope-aware; `name_conflict` if the new name is visible at any reference). Refuse non-contiguous runs (`not_contiguous`) and jumps crossing the run boundary (`jump_crosses_boundary`). Rewrite expressions via the expression AST printer. CLI `fid program edit-*`, MCP `program_edit_*`. Acceptance: AC-1..AC-6, including same invocation sequence before and after extract. Constraints: no panic, no `unsafe`, deny `unwrap`/`expect`/`panic`/`todo`/`unimplemented`/`unreachable`; step IDs of surviving steps never change.

## Open questions

- None.

## Children

_Leaf: none._
