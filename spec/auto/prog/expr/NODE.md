---
id: AUTO.PROG.EXPR
title: Typed variables and unit-aware expressions
level: 3
parent: AUTO.PROG
kind: module
status: open
phase: P1b
size: L
owners: []
claimed_by:
satisfies: [REQ-AUTO.PROG-004, REQ-AUTO.PROG-005]
covers: [PCD.PRG.02, SA.MP.01]
depends_on: [AUTO.PROG.FMT, CORE.REG]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# AUTO.PROG.EXPR: Typed variables and unit-aware expressions

## Purpose

The expression language of Command Programs: literals (including authored quantities with units), variables, arithmetic, comparison, boolean logic, string formatting, field access on command results, and a small library of pure functions. Every expression has a static type, quantities carry quantity kinds, and combining incompatible kinds is a check-time error. Expressions steer programs (conditions, loop bounds, arguments); they never produce values of record (C1).

## Scope

**In:**
- Grammar and parser for expression source text stored by `AUTO.PROG.FMT`
- Type system: bool, integer, decimal, quantity (kind + unit), string, enum, element reference, list, record (command result shapes from the manifest)
- Unit algebra for quantity kinds; conversion only through the CORE single-rounding rule
- Evaluator for control use; marking every expression-derived value as "not of record"
- Variable declarations and scoping rules

**Out:**
- Values of record from formulas (decision D-027, kernel side in `KRN`)
- Static checker orchestration (`AUTO.PROG.CHECK`)

## Requirements

- **REQ-AUTO.PROG.EXPR-001** Every expression shall have a static type derivable from declarations and the manifest, and if it does not, then the checker shall report `type_unknown` at the expression's step ID.
- **REQ-AUTO.PROG.EXPR-002** If an expression adds, subtracts or compares quantities of different quantity kinds, then the checker shall report `unit_mismatch`.
- **REQ-AUTO.PROG.EXPR-003** Every value computed by an expression shall be tagged `not_of_record`, and if such a value is passed to a command parameter declared as a value of record, then the checker shall report `expression_value_of_record`.
- **REQ-AUTO.PROG.EXPR-004** Expression evaluation shall be deterministic (D0) for the same inputs, using decimal arithmetic for authored decimals and the `fid-num` facade for any transcendental function.
- **REQ-AUTO.PROG.EXPR-005** Expression evaluation shall have no side effects and no access to files, network, clock or randomness.

## Interfaces

- **Offers:** `parse_expr`, `type_of`, `eval_expr` for `AUTO.PROG.CHECK` and `AUTO.PROG.INTERP`.
- **Consumes:** `AUTO.PROG.FMT`; CORE.REG quantity kinds, units and result schemas; `fid-num`.

## Decomposition hints

GRAMMAR (syntax and parser), TYPES (type system and unit algebra), EVAL (evaluator), LIB (function library).

## Open questions

- D-027 decides whether the kernel offers a formula command for values of record; if yes, this node should share its grammar and type checker with that command.

## Children

_None yet: status `open`._
