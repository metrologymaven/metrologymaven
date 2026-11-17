---
id: AUTO.REC.LIFT.VALUES
title: Lifting chosen nominal values and element references
level: 4
parent: AUTO.REC.LIFT
kind: feature
status: leaf
phase: P1b
size: S
owners: []
claimed_by:
satisfies: [REQ-AUTO.REC.LIFT-002, REQ-AUTO.REC.LIFT-004]
covers: [GOM.AUT.02]
depends_on: [AUTO.REC.LIFT.PASS]
side_effect: W1
determinism: D0
cli: [fid program lift-value]
mcp: [program_lift_value]
journal: [program.lift_value@1]
updated: 2026-11-17
---

# AUTO.REC.LIFT.VALUES: Lifting chosen nominal values and element references

## Purpose

After the automatic pass has lifted data handles, the user decides which other recorded literals should become parameters: a nominal diameter typed during the session, a tolerance, an element reference that differs per part family member. `program.lift_value@1` turns one literal argument (or every equal literal of the same parameter type the user names) into a declared parameter whose default is the recorded value, keeping the authored decimal text and unit, so the draft still replays identically.

## Scope

**In:**
- `program.lift_value@1` (W1 on drafts): select by step ID and argument name, optional "all equal literals of this type", parameter name and description
- Keeping authored quantity text and unit as the default (no conversion)
- Updating the lift report's unlifted list
- Replay-identity check reused from `AUTO.REC.LIFT.PASS`

**Out:**
- Automatic data-handle lifting and the report format (`AUTO.REC.LIFT.PASS`)
- Selection translation (`AUTO.REC.LIFT.SELECT`)

## Requirements

- **REQ-AUTO.REC.LIFT.VALUES-001** When `program.lift_value@1` names a literal argument of a draft, the system shall declare a parameter of the argument's registry type with the literal as default and rewrite the argument into a reference to it.
- **REQ-AUTO.REC.LIFT.VALUES-002** When `all_equal` is set, the system shall also rewrite every other literal argument with the same registry type and byte-identical canonical value, and list them in the result.
- **REQ-AUTO.REC.LIFT.VALUES-003** A lifted quantity default shall keep its authored decimal text and unit exactly, so evaluating the reference with the default binding yields canonical arguments byte-identical to the recorded literal.
- **REQ-AUTO.REC.LIFT.VALUES-004** If the argument is not a literal, the name is already declared, or the program is not a draft, then the command shall fail with `not_a_literal`, `name_conflict` or `immutable` and change nothing.
- **REQ-AUTO.REC.LIFT.VALUES-005** After the rewrite, the system shall run the replay-identity check of `AUTO.REC.LIFT.PASS` and, if it is not `IDENTICAL`, fail with `lift_not_identical` and change nothing.

## Acceptance criteria

- **AC-1** Given the lifted draft from `AUTO.REC.LIFT.PASS` AC-1, when `fid program lift-value --program P --step S6 --arg nominal --name hole_dia --all-equal --json` runs, then a parameter `hole_dia` with default `"12.700 mm"` exists, S6 and the two other steps with the same literal are rewritten, and the unlifted list shrinks by three.
- **AC-2** (replay) Given the result of AC-1 with its default binding, exact replay is `IDENTICAL`.
- **AC-3** (error path) Given an argument that is already an expression, then `not_a_literal`; given name `input_1` (already declared), then `name_conflict`.
- **AC-4** Given an element-reference argument, then the parameter type is the registry element-reference type and the default is the recorded element ID.

## Design

- **Registry command(s):** `program.lift_value@1`, W1 on drafts. Params: `program`, `expected_digest`, `step: StepId`, `arg: String`, `name: ParamName`, `description: Option<String>`, `all_equal: bool`. Result: `{ param, rewritten: Vec<StepId>, report: LiftReport }`.
- **Data structures:** reuses `LiftReport` and the edit path of `AUTO.REC.LIFT.PASS`.
- **Algorithm:** locate literal; derive type from the manifest parameter; declare parameter; rewrite (and equal literals); replay check; record authorship.
- **Errors:** `not_a_literal`, `name_conflict`, `immutable`, `stale_program`, `lift_not_identical`.
- **Determinism:** D0.
- **Performance budget:** negligible beyond the replay check.
- **Crate:** `fid-program-rec` (module `lift::values`).

## Test fixtures

The lifted draft of `AUTO.REC.LIFT.PASS` AC-1 on `fixtures/projects/align-and-check/`.

## Build prompt

> In crate `fid-program-rec`, add module `lift::values` with registry command `program.lift_value@1` (W1, drafts only; CLI `fid program lift-value`, MCP `program_lift_value`). Given step ID and argument name of a literal, declare a parameter of that argument's registry type with the literal as default (quantities keep authored decimal text and unit, no conversion), rewrite the argument to a reference, and with `all_equal` rewrite every other literal of the same type and byte-identical canonical value. Errors: `not_a_literal`, `name_conflict`, `immutable`, `stale_program`; then run the replay-identity check from `lift::pass` and fail with `lift_not_identical` (nothing changed) if it is not `IDENTICAL`. Record authorship for rewritten steps. Acceptance: AC-1..AC-4. Constraints: no panic, no `unsafe`, deny `unwrap`/`expect`/`panic`/`todo`/`unimplemented`/`unreachable`; authored values keep value and unit (CONSTITUTION §4).

## Open questions

- None.

## Children

_Leaf: none._
