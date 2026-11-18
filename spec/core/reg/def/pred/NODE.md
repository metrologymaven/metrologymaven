---
id: CORE.REG.DEF.PRED
title: Precondition language and bounded evaluator
level: 4
parent: CORE.REG.DEF
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
built_in: [fid-registry]
built_pr: 56
satisfies: [REQ-CORE.REG.DEF-005]
covers: []
depends_on: []
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# CORE.REG.DEF.PRED: Precondition language and bounded evaluator

## Purpose

Settles the precondition language question left open by `CORE.REG` (CEL or restricted Starlark): preconditions are written in **FPL**, a small, non-Turing-complete subset of CEL-style expression syntax, parsed and checked when the registry is built and evaluated by an in-house interpreter with a hard step budget. Choosing an in-house subset avoids a new dependency (no D-001 allowlist question) and makes the step bound and determinism easy to guarantee.

## Scope

**In:** FPL grammar, types, built-in functions, the parser and type checker, the AST, the evaluator with step counting, and the `PredContext` trait through which an evaluator reads state.
**Out:** supplying document state at run time (`CORE.BUS.PIPE` implements `PredContext`); the formula expression language for values of record (`AUTO.PROG.EXPR` syntax, `KRN` evaluation, D-027); deciding which commands have which preconditions (each domain).

## Requirements

- **REQ-CORE.REG.DEF.PRED-001** FPL shall offer only literals, field access on context values, comparison, boolean `&&` `||` `!`, the conditional operator, `in`, `size`, `has`, and the bounded list functions `all` and `exists`, and shall offer no recursion, loops, assignment, user-defined functions, I/O, clock or randomness.
- **REQ-CORE.REG.DEF.PRED-002** When the registry is built, every precondition shall be parsed and type-checked against the command's parameter schema and the declared context types, and a parse or type error shall fail the build naming the command and the source position.
- **REQ-CORE.REG.DEF.PRED-003** Each precondition shall declare a step bound of at most 10,000 steps, and if evaluation reaches its bound, then the evaluator shall stop and return `precondition_budget` [unverified that 10,000 suffices].
- **REQ-CORE.REG.DEF.PRED-004** The evaluator shall count one step per AST node visited and per list element visited by `all` or `exists`, so the count for a given expression and input is the same on every platform.
- **REQ-CORE.REG.DEF.PRED-005** Evaluation shall return `true`, `false` with the precondition's declared message key, or a typed error, and shall never panic.
- **REQ-CORE.REG.DEF.PRED-006** The evaluator shall read state only through the `PredContext` trait and shall not depend on any CORE.BUS or document type.
- **REQ-CORE.REG.DEF.PRED-007** Numbers in FPL shall compare as exact integers or as f64 under total order, with `-0.0` normalised to `+0.0` when a value enters evaluation, and a NaN operand shall be a typed `nan_operand` error.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/core/reg/def/pred/cases.json` (at least 60 cases: expression, context, parameters, expected result or error, expected step count), when each is evaluated, then result and step count match.
- **AC-2** (error path) Given `fixtures/core/reg/def/pred/bad_syntax.json` (a loop keyword, a user function, an unknown field, a type mismatch), when the registry build parses them, then each fails with its position.
- **AC-3** (error path) Given `all(x in params.ids, exists(y in params.ids, y == x))` with 1,000 distinct IDs and a bound of 10,000, when evaluated with short-circuiting `all` and `exists`, then the result is `precondition_budget` after exactly 10,000 steps.
- **AC-4** Given AC-1 run on x86-64, ARM64 and wasm32, then every result and step count is identical.

## Design

- **Registry command(s):** none; library used by the registry build and by `CORE.BUS.PIPE`.
- **Data structures:** `Expr` AST (`&&` and `||` chains are flat n-ary nodes); `Type { Bool, Int, Float, String, EntityRef, List(Box<Type>), Map }`; `Predicate { src, ast, bound: u32, message_key }`, compiled from the structured `CommandDef.preconditions` entry `Precondition { expr, bound, message_key }` (JSON `{"expr", "bound", "message_key"}`, defined in `CORE.REG.DEF.FIELD`; a declared bound above 10,000 fails the build with `fpl_bound`); `trait PredContext { fn get(&self, path: &FieldPath) -> Result<Value, PredError>; }`; `PredError { budget, nan_operand, missing_field, type_error }`.
- **Algorithm:** hand-written recursive-descent parser with a depth limit of 64; static type checker; tree-walking evaluator with an explicit work stack and a step counter checked before each node. Short-circuit order of `&&` and `||` is left to right and fixed; `all` stops at the first false element and `exists` at the first true one. Forbidden words (loop and definition keywords) are refused only where an identifier starts an expression or names a variable; after `.` they are field names.
- **Errors:** as in REQ-003, -005, -007; parse errors `fpl_syntax`, `fpl_type`, `fpl_bound`.
- **Determinism:** D0; no hash maps, fixed evaluation order.
- **Performance budget:** a 50-node predicate evaluates in < 10 µs [unverified until measured].
- **Crate:** `fid-registry` (module `pred`).

## Test fixtures

`fixtures/core/reg/def/pred/` hand-written JSON with a fake `PredContext` (fixed maps); CC-BY-4.0.

## Build prompt

> In crate `fid-registry`, add module `pred` implementing FPL, a non-Turing-complete precondition language with CEL-like syntax: literals, field access, comparisons, `&&` `||` `!`, `?:`, `in`, `size`, `has`, and bounded `all(x in list, expr)` / `exists(x in list, expr)`; no loops, recursion, assignment, user functions, I/O, clock or randomness. Provide `parse(src) -> Result<Expr, ParseError>` (depth limit 64), `typecheck(&Expr, &ParamSchema, &ContextTypes)`, and `eval(&Predicate, &dyn PredContext) -> Result<bool, PredError>` with one step per AST node and per list element, stopping at the declared bound (max 10,000) with `precondition_budget`. NaN operand is `nan_operand`. Do not depend on any bus or document crate; the bus implements `PredContext`. Tests AC-1..AC-4 with fixtures in `fixtures/core/reg/def/pred/`. Constraints: CONSTITUTION §4 (D0 portable), §8 (no panic, no `unsafe`, deny unwrap/expect), no new third-party dependency.

## Open questions

- FPL is a CEL-like subset written in-house, not a conforming CEL implementation; any claim of CEL compatibility is [unverified] and should not be made in docs.
- Whether D-027's formula language (`expr.evaluate_formula@1`) could later share FPL's parser is left to `AUTO.PROG.EXPR`; the two languages have different jobs (gating vs values of record).

## Children

_Leaf: none._
