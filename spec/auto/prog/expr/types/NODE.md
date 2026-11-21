---
id: AUTO.PROG.EXPR.TYPES
title: Type system, unit algebra and variable scoping
level: 4
parent: AUTO.PROG.EXPR
kind: task
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
built_in: [fid-program]
built_pr: 418
satisfies: [REQ-AUTO.PROG.EXPR-001, REQ-AUTO.PROG.EXPR-002, REQ-AUTO.PROG.EXPR-003]
covers: [PCD.PRG.02]
depends_on: [AUTO.PROG.EXPR.GRAMMAR, AUTO.PROG.FAMILY, CORE.REG.DEF, GEO.FRAME.UNITS]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# AUTO.PROG.EXPR.TYPES: Type system, unit algebra and variable scoping

## Purpose

Gives every expression a static type and every variable a declared type and scope. Quantities carry a quantity kind (length, angle, temperature and the other kinds of the registry quantity catalogue) and a unit; combining incompatible kinds is a check-time error. The checker also marks every expression-derived value `not_of_record` and refuses to pass one to a command parameter that the registry declares as a value of record (C1), which leaves `expr.evaluate_formula@1` as the only path from user arithmetic to a value of record.

## Scope

**In:**
- Types: `bool`, `int`, `decimal`, `quantity<kind>`, `string`, `enum<E>`, `element<T>`, `list<T>`, `record<Schema>` (command result shapes from the manifest), `formula<kind>` (a compiled formula handle)
- Name resolution: program parameters (from `AUTO.PROG.FAMILY` declarations), declared variables, invoke result bindings, subroutine parameters; block scoping rules
- Unit algebra: dimension of `*` and `/`, equal kinds for `+ - < == ...`, unit symbol lookup in the unit catalogue
- Function signatures of the library (types only)
- The `not_of_record` rule and its check against parameter declarations

**Out:**
- Parsing (`AUTO.PROG.EXPR.GRAMMAR`), evaluation (`AUTO.PROG.EXPR.EVAL`), formula compilation (`AUTO.PROG.EXPR.FORMULA`)
- Running all checks over a program and ordering diagnostics (`AUTO.PROG.CHECK`)

## Requirements

- **REQ-AUTO.PROG.EXPR.TYPES-001** The checker shall derive a static type for every expression from declarations, invoke result schemas in the manifest and the function signatures, and if it cannot, then it shall report `type_unknown` with step ID and span.
- **REQ-AUTO.PROG.EXPR.TYPES-002** If `+`, `-`, a comparison, `min` or `max` combines quantities of different quantity kinds, or a quantity with a plain number other than in `*` and `/`, then the checker shall report `unit_mismatch` naming both kinds.
- **REQ-AUTO.PROG.EXPR.TYPES-003** The kind of a product or quotient of quantities shall be derived by adding or subtracting dimension exponents, and a result whose dimensions match no catalogue kind shall be typed as an anonymous derived kind usable only in further `*` and `/` or comparison with the same derived kind.
- **REQ-AUTO.PROG.EXPR.TYPES-004** If a quantity literal's unit symbol is not in the unit catalogue, then the checker shall report `unknown_unit` with the span.
- **REQ-AUTO.PROG.EXPR.TYPES-005** A variable shall be visible from its declaration to the end of the enclosing block, and a reference outside that region or a second declaration of the same name in one block shall be reported as `undeclared` or `redeclared`.
- **REQ-AUTO.PROG.EXPR.TYPES-006** Every expression type shall carry the flag `not_of_record`, and if an expression is passed to a command parameter that the manifest declares `value_of_record`, then the checker shall report `expression_value_of_record` at that argument.
- **REQ-AUTO.PROG.EXPR.TYPES-007** Assigning a value to a variable of another type shall be reported as `type_mismatch`, with no implicit conversion except `int` to `decimal`.

## Acceptance criteria

- **AC-1** Given fixture programs `fixtures/program/types/valid/*.json` with manifest `fixtures/manifest/prog-demo.json`, when `check_types` runs, then each produces no diagnostic and the inferred types equal the expected types file.
- **AC-2** (error path) Given `fixtures/program/types/unit-mismatch.json` (`d1 + 5[deg]` where `d1` is a length), then one `unit_mismatch` naming `length` and `angle` at the expression's step ID and span.
- **AC-3** Given `area = w * h` with `w` and `h` lengths, then `area` has type `quantity<area>`; given `w * h + w`, then `unit_mismatch`.
- **AC-4** (error path) Given an invoke argument `{"expr": "r.diameter * 2"}` for parameter `nominal` declared `value_of_record` in the manifest, then `expression_value_of_record` at that argument.
- **AC-5** (error path) Given `12[furlong]`, then `unknown_unit`; given a variable used after its block, then `undeclared`.
- **AC-6** (determinism) Given the same program and manifest, the diagnostic list is byte-identical across runs and platforms.

## Design

- **Registry command(s):** none; library `check_types(&Program, &Manifest) -> TypeReport`.
- **Data structures:** `Type` enum as in Scope; `Dim { exp: [i8; 7], angle: i8 }` matching the kernel `FormulaIr` dimension so the two agree; `TypeEnv` with scoped `BTreeMap<Name, VarInfo>`; `TypeReport { types: BTreeMap<(StepId, Span), Type>, diagnostics: Vec<Diag> }`.
- **Algorithm:** one pass over the program in step order, pushing scopes for subroutine bodies, loop bodies and branches; bottom-up typing of each AST; kind lookup from the registry quantity catalogue (`CORE.REG.DEF`); unit symbol lookup in `GEO.FRAME.UNITS`.
- **Errors (diagnostics):** `type_unknown`, `unit_mismatch`, `unknown_unit`, `undeclared`, `redeclared`, `type_mismatch`, `expression_value_of_record`.
- **Determinism:** D0.
- **Performance budget:** type-check 10 000 steps in < 100 ms [unverified until measured].
- **Crate:** `fid-program` (module `expr::types`).

## Test fixtures

`fixtures/program/types/` programs with expected types and diagnostics; `fixtures/manifest/prog-demo.json` (synthetic manifest with result schemas, quantity kinds and one `value_of_record` parameter). Project-authored, CC-BY-4.0.

## Build prompt

> In crate `fid-program`, add module `expr::types` with `check_types(program, manifest) -> TypeReport`. Types: bool, int, decimal, quantity<kind>, string, enum, element<T>, list<T>, record<schema from manifest result>, formula<kind>. Resolve names from parameter declarations, variable declarations, invoke result bindings and subroutine parameters with block scoping (`undeclared`, `redeclared`). Unit algebra over `Dim { exp: [i8;7], angle: i8 }` (same layout as the kernel `FormulaIr`): `*`/`/` add/subtract exponents; `+ - comparisons min max` need equal kinds (`unit_mismatch` naming both); unknown unit symbols → `unknown_unit` (catalogue from the GEO.FRAME.UNITS crate); kinds from the CORE.REG quantity catalogue. Every expression type is `not_of_record`; passing one to a parameter declared `value_of_record` → `expression_value_of_record`. No implicit conversion except int→decimal (`type_mismatch`). Diagnostics carry step ID and byte span; output ordered by step position. Acceptance: AC-1..AC-6. Constraints: no panic, no `unsafe`, deny `unwrap`/`expect`/`panic`/`todo`/`unimplemented`/`unreachable`; `BTreeMap` only; CONSTITUTION C1 (expressions never yield values of record).

## Open questions

- REQ-006 needs the manifest to flag parameters that hold values of record (`value_of_record: true`); `CORE.REG.DEF` does not name this keyword yet. Until it does, the checker treats every parameter typed as a ResultRecord value as `value_of_record`.

## Children

_Leaf: none._
