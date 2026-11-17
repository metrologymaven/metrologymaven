---
id: AUTO.PROG.EXPR.FORMULA
title: Compilation of formulas to the kernel formula IR
level: 4
parent: AUTO.PROG.EXPR
kind: task
status: leaf
phase: P1b
size: S
owners: []
claimed_by:
satisfies: [REQ-AUTO.PROG.EXPR-003]
covers: [PCD.PRG.02]
depends_on: [AUTO.PROG.EXPR.TYPES, KRN.CONS.DERIV.FORMULA]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# AUTO.PROG.EXPR.FORMULA: Compilation of formulas to the kernel formula IR

## Purpose

The only bridge from user arithmetic to a value of record (C1, D-027). A program or scripted check writes a formula in the same expression syntax (for example `d1 / d2` or `mean(a, b, c, d)` over result references); this module type-checks it with the shared checker and compiles it to the kernel's `FormulaIr`, which the program then passes to `expr.evaluate_formula@1`. AUTO owns syntax and typing; the kernel re-validates and evaluates.

## Scope

**In:**
- The formula subset: quantity and number literals, element-result references as input slots, `+ - * /`, unary `-`, integer powers, and the functions the IR supports (`abs`, `sqrt`, `min`, `max`, `sum`, `mean`, `hypot`, `sin`, `cos`, `tan`, `asin`, `acos`, `atan`, `atan2`)
- Mapping the shared `Dim` and literal units to the IR; canonical IR bytes and digest
- Refusing constructs outside the subset (strings, booleans, conditionals, field access other than result value, variables holding `not_of_record` values)
- The `formula<kind>` type used by `AUTO.PROG.EXPR.TYPES` for an invoke argument of `expr.evaluate_formula@1`

**Out:**
- Evaluation, dimension re-check and bounds (`KRN.CONS.DERIV.FORMULA`)
- The starting uncertainty class of formula results (`UNC`)

## Requirements

- **REQ-AUTO.PROG.EXPR.FORMULA-001** When a type-checked formula uses only the formula subset, the compiler shall emit a `FormulaIr` whose slots are the referenced element results in first-use order, whose literals keep their exact decimal text and unit, and whose nodes are in topological order.
- **REQ-AUTO.PROG.EXPR.FORMULA-002** If a formula uses a construct outside the subset or reads a program variable, then compilation shall fail with `not_a_formula` naming the construct and span.
- **REQ-AUTO.PROG.EXPR.FORMULA-003** The declared result dimension of the emitted IR shall equal the dimension the shared type checker inferred.
- **REQ-AUTO.PROG.EXPR.FORMULA-004** Compiling the same formula source against the same declarations shall give byte-identical canonical IR and the same IR digest.
- **REQ-AUTO.PROG.EXPR.FORMULA-005** If the emitted IR would exceed the kernel's bounds (4 096 nodes, power exponent outside -8 to 8), then compilation shall fail with `formula_too_large` or `bad_exponent` before any command is invoked.

## Acceptance criteria

- **AC-1** Given the 40 formula sources in `fixtures/program/formula/cases.json` with their expected IR, when compiled, then each IR's canonical bytes equal the expected file and pass `FormulaIr` validation from the kernel crate.
- **AC-2** Given `d1 / d2` over two length results, then the IR has two slots, one `div` node and result dimension dimensionless; evaluating it with `expr.evaluate_formula@1` on the fake kernel bus returns the ratio.
- **AC-3** (error path) Given `if d1 > d2 then d1 else d2`, `format("{}", d1)` and `d1 + count` (count a program variable), then each fails with `not_a_formula` and the span.
- **AC-4** (error path) Given `d1 ^ 9`, then `bad_exponent`.
- **AC-5** (determinism) Given the same source on Windows and Linux, the IR digest is identical.

## Design

- **Registry command(s):** none; library `compile_formula(&TypedExpr, &Decls) -> Result<FormulaIr, FormulaError>`. The IR is passed as the `ir` argument of `expr.evaluate_formula@1` (owned by `KRN.CONS.DERIV.FORMULA`).
- **Data structures:** uses `FormulaIr`, `Slot`, `Literal`, `Node`, `Dim` from the kernel crate.
- **Algorithm:** post-order walk; deduplicate slots by element reference; literals as exact decimals with unit IDs; emit nodes once per AST node.
- **Errors:** `not_a_formula`, `formula_too_large`, `bad_exponent`.
- **Determinism:** D0.
- **Performance budget:** negligible.
- **Crate:** `fid-program` (module `expr::formula`); depends on the kernel crate `fid-construct` for the IR types only.

## Test fixtures

`fixtures/program/formula/cases.json` (project-authored sources and expected IR, CC-BY-4.0); the kernel's IR validator; fake bus with the kernel formula command.

## Build prompt

> In crate `fid-program`, add module `expr::formula` with `compile_formula(typed_expr, decls) -> Result<FormulaIr, FormulaError>` using the `FormulaIr` types from the kernel crate (`fid-construct::formula`). Subset: number and quantity literals (exact decimal text and unit), element-result references as slots in first-use order, + - * /, unary -, integer powers in -8..=8, and abs, sqrt, min, max, sum, mean, hypot, sin, cos, tan, asin, acos, atan, atan2. Anything else, or any program variable, → `not_a_formula` with span; > 4 096 nodes → `formula_too_large`; bad power → `bad_exponent`. Result dimension = the type checker's inferred `Dim`. Output canonical IR bytes and digest, identical across platforms. Acceptance: AC-1..AC-5. Constraints: no panic, no `unsafe`, deny `unwrap`/`expect`/`panic`/`todo`/`unimplemented`/`unreachable`; the kernel re-validates everything (CONSTITUTION C1, D-027): this compiler never evaluates a formula.

## Open questions

- None; the kernel IR is fixed by `KRN.CONS.DERIV.FORMULA`.

## Children

_Leaf: none._
