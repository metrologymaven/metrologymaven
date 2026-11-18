---
id: AUTO.PROG.EXPR.GRAMMAR
title: Expression grammar and bounded parser
level: 4
parent: AUTO.PROG.EXPR
kind: task
status: leaf
phase: P1b
size: S
owners: []
claimed_by:
built_in: [fid-program]
built_pr: 129
satisfies: [REQ-AUTO.PROG.EXPR-005]
covers: [SA.MP.01]
depends_on: [AUTO.PROG.FMT]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# AUTO.PROG.EXPR.GRAMMAR: Expression grammar and bounded parser

## Purpose

The surface syntax of Command Program expressions and the parser that turns the source strings stored by `AUTO.PROG.FMT` into a typed-ready syntax tree with source spans. The grammar is closed and small: literals (integers, decimals, quantities with units, strings, booleans), variable and parameter references, field and index access on command results, arithmetic, comparison, boolean logic, a conditional expression and calls to a fixed function library. There is no assignment, no lambda, no loop and no way to name a command, so an expression cannot change state (C3).

## Scope

**In:**
- Lexer and grammar (published as EBNF in `docs/program/expr-grammar.md`, project-authored)
- Quantity literal syntax `12.700[mm]`, `30[deg]`
- AST with byte spans; precedence and associativity table
- Parse bounds and typed parse errors

**Out:**
- Types, units and name resolution (`AUTO.PROG.EXPR.TYPES`)
- Evaluation and the function implementations (`AUTO.PROG.EXPR.EVAL`)
- Compilation to the kernel formula IR (`AUTO.PROG.EXPR.FORMULA`)

## Requirements

- **REQ-AUTO.PROG.EXPR.GRAMMAR-001** The parser shall accept exactly the language of the published EBNF: integer, decimal, quantity (`<decimal>[<unit symbol>]`), string and boolean literals; identifiers; `.field` and `[index]` access; unary `-` and `not`; binary `* / %`, `+ -`, `< <= > >= == !=`, `and`, `or` in that precedence order, left-associative; `if <e> then <e> else <e>`; and calls `name(args)`.
- **REQ-AUTO.PROG.EXPR.GRAMMAR-002** If the source exceeds 4 KiB, 1 024 tokens or nesting depth 64, then parsing shall fail with `expr_too_large` and no AST.
- **REQ-AUTO.PROG.EXPR.GRAMMAR-003** If the source does not parse, then the parser shall return `expr_syntax` with the byte offset of the first offending token.
- **REQ-AUTO.PROG.EXPR.GRAMMAR-004** Every AST node shall carry its byte span in the source, so diagnostics can point at the step ID and span.
- **REQ-AUTO.PROG.EXPR.GRAMMAR-005** The grammar shall contain no construct that assigns, declares, loops, performs I/O or names a registry command.
- **REQ-AUTO.PROG.EXPR.GRAMMAR-006** Printing an AST and parsing the result shall give an equal AST.

## Acceptance criteria

- **AC-1** Given the corpus `fixtures/program/expr/valid/*.txt` (one file per production, with expected AST in JSON), when parsed, then each AST equals the expected one, spans included.
- **AC-2** (error path) Given `fixtures/program/expr/invalid/*.txt` (unbalanced brackets, `12.7[`, `a = 1`, `x++`), then each fails with `expr_syntax` at the expected offset.
- **AC-3** (error path) Given a 5 KiB source and one with 65 nested parentheses, then both fail with `expr_too_large`.
- **AC-4** (property) Given 10 000 generated ASTs (seed 7), print-then-parse yields an equal AST each time.
- **AC-5** (fuzz) Given the fuzz target `expr_parse` for 10 minutes, then no crash and no allocation above 64 MiB. [unverified: the 10-minute nightly fuzz run has not been performed; CI only compiles the target, and `tests/expr_alloc.rs` plus a 50 000-case hostile-input test stand in until it is run]

## Design

- **Registry command(s):** none; library `parse_expr(src: &str) -> Result<Expr, ParseError>`, `print_expr(&Expr) -> String`.
- **Data structures:** `Expr { kind: ExprKind, span: Span }`; `ExprKind = Int(i64) | Dec(DecText) | Qty(DecText, UnitSym) | Str(String) | Bool(bool) | Ident(Name) | Field(Box<Expr>, Name) | Index(Box<Expr>, Box<Expr>) | Unary(Op, Box<Expr>) | Binary(Op, Box<Expr>, Box<Expr>) | Cond(Box<Expr>, Box<Expr>, Box<Expr>) | Call(Name, Vec<Expr>)`. Decimal text is kept verbatim for exact handling downstream.
- **Algorithm:** hand-written lexer and Pratt parser with an explicit depth counter (no unbounded recursion).
- **Errors:** `expr_syntax { offset }`, `expr_too_large`.
- **Determinism:** D0.
- **Performance budget:** 4 KiB expression parsed in < 50 µs [unverified until measured].
- **Depth definition (build note, PR #129):** "nesting depth" counts every AST edge and every pair of parentheses (`x` = 0, `(x)` = 1). So 64 nested parentheses are accepted and 65 refused (AC-3), and a flat left-associative chain of 66 or more operands (`a+b+...`) is also refused with `expr_too_large`. This goes beyond a pure bracket-nesting reading; it was chosen so every later recursive pass (TYPES, EVAL, FORMULA) stays within 64 levels.
- **Crate:** `fid-program` (module `expr::syntax`).

## Test fixtures

Project-authored corpora `fixtures/program/expr/valid/` and `fixtures/program/expr/invalid/` (CC-BY-4.0); seeded AST generator for the property test.

## Build prompt

> In crate `fid-program`, add module `expr::syntax`: a lexer and Pratt parser for the Command Program expression language. Literals: integer, decimal (keep text verbatim), quantity `<decimal>[<unit symbol>]`, string, `true`/`false`; identifiers; `.field`, `[index]`; unary `-`, `not`; binary `* / %` > `+ -` > comparisons > `and` > `or`, left-associative; `if c then a else b`; calls `name(args)`. No assignment, declaration, loop, I/O or command naming. Every node carries a byte span. Bounds: 4 KiB source, 1 024 tokens, depth 64 → `expr_too_large`; syntax errors → `expr_syntax` with byte offset. Provide `print_expr` with print-then-parse identity. Publish the EBNF in `docs/program/expr-grammar.md`. Acceptance: AC-1..AC-5 (corpora, bounds, property test with seed 7, fuzz target `expr_parse`). Constraints: no panic, no `unsafe`, deny `unwrap`/`expect`/`panic`/`todo`/`unimplemented`/`unreachable`; explicit depth counter, no unbounded recursion; all input hostile.

## Open questions

- **Precedence of `not` (for the spec owner to confirm):** REQ-001 lists unary `-` and `not` above the binary operators, so `not a == b` parses as `(not a) == b`. Python and SQL bind `not` below comparisons (`not (a == b)`). The build follows REQ-001 as written; the owner should confirm or change this before programs depend on it.
- **Flat chains and depth:** confirm the depth definition in Design (a flat chain of 66+ operands is `expr_too_large`), or relax it to bracket nesting only and bound tree depth elsewhere.

- Unit symbols accepted inside `[...]` are those of the unit catalogue (`GEO.FRAME.UNITS`); the parser accepts any symbol token and `AUTO.PROG.EXPR.TYPES` resolves it.

## Children

_Leaf: none._
