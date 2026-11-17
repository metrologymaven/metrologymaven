---
id: AUTO.PROG.EXPR.EVAL
title: Deterministic control evaluator and function library
level: 4
parent: AUTO.PROG.EXPR
kind: task
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-AUTO.PROG.EXPR-003, REQ-AUTO.PROG.EXPR-004, REQ-AUTO.PROG.EXPR-005]
covers: [PCD.PRG.02]
depends_on: [AUTO.PROG.EXPR.TYPES, KRN.NUM.KDEC, KRN.NUM.FN, GEO.FRAME.UNITS]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# AUTO.PROG.EXPR.EVAL: Deterministic control evaluator and function library

## Purpose

Evaluates type-checked expressions for control use inside the interpreter: conditions, loop bounds, invoke arguments, operator-prompt texts. Evaluation is pure, bit-reproducible (D0) and has no access to files, network, clock or randomness. Every value it produces is tagged `not_of_record`, so it can steer a program but never become a reported result (C1).

## Scope

**In:**
- Value model and evaluation of every AST kind from `AUTO.PROG.EXPR.GRAMMAR`
- Exact decimal handling of authored decimals; single-rounding conversion to SI f64 when a decimal meets an f64 command result or another unit
- Function library: `abs`, `min`, `max`, `round`, `floor`, `ceil`, `sqrt`, `sin`, `cos`, `tan`, `atan2`, `len`, `contains`, `format`, `to_unit`
- Runtime errors (division by zero, index out of range, overflow, NaN)

**Out:**
- Typing (`AUTO.PROG.EXPR.TYPES`); formula compilation for values of record (`AUTO.PROG.EXPR.FORMULA`)
- Variable storage and control flow (`AUTO.PROG.INTERP.ENGINE`)

## Requirements

- **REQ-AUTO.PROG.EXPR.EVAL-001** Every value returned by the evaluator shall carry the `not_of_record` tag, and the tag shall survive storage in variables and use as an invoke argument.
- **REQ-AUTO.PROG.EXPR.EVAL-002** Addition, subtraction, multiplication and comparison of two authored decimals in the same unit shall be exact within 38 significant digits, and if the exact result needs more, then evaluation shall fail with `decimal_overflow`.
- **REQ-AUTO.PROG.EXPR.EVAL-003** When an authored decimal meets an f64 value or a different unit, the evaluator shall convert it once to SI f64 through the unit catalogue (round half to even) and continue in f64.
- **REQ-AUTO.PROG.EXPR.EVAL-004** Comparisons between an f64 value and an authored decimal shall be exact (`kdec::cmp`), so `0.1[mm]` is not equal to the nearest f64 of 0.0001 m.
- **REQ-AUTO.PROG.EXPR.EVAL-005** f64 arithmetic shall follow KNP-1 (no fused multiply-add, transcendental functions only through `fid-num`, fixed evaluation order), so results are bit-identical on x86-64, ARM64 and wasm32.
- **REQ-AUTO.PROG.EXPR.EVAL-006** If evaluation divides by zero, indexes out of range, overflows an integer or produces NaN or infinity, then it shall fail with `eval_error` naming the kind and the span, and return no value.
- **REQ-AUTO.PROG.EXPR.EVAL-007** The evaluator shall have no access to files, network, clock, environment, randomness or the bus; its only inputs shall be the AST, the variable environment and the function library.

## Acceptance criteria

- **AC-1** Given the table `fixtures/program/eval/cases.json` (300 expressions with environments and expected results as canonical JSON, f64 as hex bits), when evaluated on x86-64, ARM64 and wasm32, then every result matches bit for bit and carries `not_of_record`.
- **AC-2** Given `0.1[mm] + 0.2[mm] == 0.3[mm]`, then `true` (exact decimal); given an f64 result 0.0003 m from a command compared with `0.3[mm]`, then the exact comparison result of `kdec::cmp`.
- **AC-3** (error path) Given `1 / 0`, `xs[10]` on a 3-element list and `9223372036854775807 + 1`, then `eval_error` with `div_zero`, `index_range`, `int_overflow` and the right spans.
- **AC-4** Given `format("{} mm", 2.50)`, then `"2.50 mm"` (decimal text preserved).
- **AC-5** (purity) Given the crate's dependency lint, then module `expr::eval` imports no I/O, time, environment or RNG API.

## Design

- **Registry command(s):** none; library `eval_expr(&TypedExpr, &Env) -> Result<Value, EvalError>`.
- **Data structures:** `Value { v: V, not_of_record: true }` with `V = Bool | Int(i64) | Dec { digits: i128, scale: i16, unit: Option<UnitId> } | Real { si: f64, kind: Dim } | Str | Enum | Element | List | Record`.
- **Algorithm:** tree walk with an explicit stack (depth ≤ 64); decimal ops on `i128` significands with scale alignment by `shift10`; conversion via the unit catalogue's single-rounding function; transcendentals via `fid_num::f`; total-order min and max.
- **Errors:** `eval_error { kind: div_zero | index_range | int_overflow | non_finite | domain, span }`, `decimal_overflow`.
- **Determinism:** D0.
- **Performance budget:** 1 M evaluations of a 20-node expression in < 1 s [unverified until measured].
- **Crate:** `fid-program` (module `expr::eval`).

## Test fixtures

`fixtures/program/eval/cases.json` with expected values computed by an offline arbitrary-precision oracle and rounded once (project-authored, CC-BY-4.0).

## Build prompt

> In crate `fid-program`, add module `expr::eval` with `eval_expr(typed_expr, env) -> Result<Value, EvalError>`. Every value carries `not_of_record`. Authored decimals stay exact (`i128` significand + scale, ≤ 38 digits, else `decimal_overflow`) for +, -, * and comparison in the same unit; when a decimal meets an f64 or another unit, convert once to SI f64 with the GEO.FRAME.UNITS single-rounding function and continue in f64; compare f64 with decimal exactly via `kdec::cmp`. f64 math follows KNP-1: no FMA, transcendentals only via `fid_num::f`, fixed order, total-order min/max. Library: abs, min, max, round, floor, ceil, sqrt, sin, cos, tan, atan2, len, contains, format, to_unit. Errors: `eval_error` (div_zero, index_range, int_overflow, non_finite, domain) with span. No I/O, clock, env, RNG or bus access; explicit stack, depth ≤ 64. Acceptance: AC-1..AC-5, including bit-identical results on x86-64, ARM64 and wasm32. Constraints: no panic, no `unsafe`, deny `unwrap`/`expect`/`panic`/`todo`/`unimplemented`/`unreachable`; CONSTITUTION C1 (never a value of record).

## Open questions

- REQ-AUTO.PROG.EXPR-004 asks for "decimal arithmetic for authored decimals"; this leaf keeps decimals exact for +, -, * and comparison and uses one journaled-style single rounding for anything else (division, transcendental, mixed units). If reviewers want exact decimal division, add a bounded-precision rule here.

## Children

_Leaf: none._
