---
id: KRN.CONS.DERIV.FORMULA
title: Kernel formula evaluation for derived values (D-027)
level: 4
parent: KRN.CONS.DERIV
kind: feature
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-KRN.CONS.DERIV-006]
covers: []
depends_on: [KRN.CONS.DECL, KRN.NUM.FN, KRN.NUM.RED, KRN.NUM.KDEC, GEO.FRAME.UNITS, KRN.MAN.CARD]
side_effect: W2
determinism: D0
cli: [fid expr evaluate-formula]
mcp: [expr_evaluate_formula]
journal: [expr.evaluate_formula@1]
updated: 2026-11-17
---

# KRN.CONS.DERIV.FORMULA: Kernel formula evaluation for derived values (D-027)

## Purpose

The only path by which user arithmetic becomes a value of record (CONSTITUTION C1, D-027). It takes a typed expression in a small intermediate representation (IR) over other results (for example "distance A / distance B" or "the mean of four diameters"), re-validates it, checks dimensions, and evaluates it under KNP-1 with the `fid-num` facade. `AUTO.PROG.EXPR` owns the surface syntax, parsing and type inference and compiles to this IR; the kernel owns evaluation. Because the formula is a construction over its input results, it recomputes when they change.

## Scope

**In:**
- The `FormulaIr` schema (versioned, canonical encoding): input slots (element reference plus expected dimension), literals (exact decimal plus unit), and nodes in topological order
- Operations: add, sub, mul, div, neg, abs, sqrt, integer power (-8 to 8), min, max, sum, mean (variadic, fixed order), hypot, sin, cos, tan, asin, acos, atan, atan2
- Dimension algebra over the seven SI base exponents plus a plane-angle flag
- Literal conversion to SI f64 by `GEO.FRAME.UNITS` (one rounding per literal, recorded in the result)
- Bounds and validation that fail closed on hostile IR
- Algorithm `expr.evaluate_formula.ir@1.0.0`, the command `expr.evaluate_formula@1`, a spec card

**Out:**
- Surface syntax, parser, type inference and the checker messages (`AUTO.PROG.EXPR`)
- Control-flow expressions in programs, which stay `not_of_record` (`AUTO.PROG.EXPR`)
- Uncertainty of formula results and their starting UEC class (`UNC`); ResultRecord assembly (`CORE`)
- Conformance decisions on formula results (`UNC`)

## Requirements

- **REQ-KRN.CONS.DERIV.FORMULA-001** When `expr.evaluate_formula@1` receives an IR, the kernel shall reject it with `IrInvalid{node, reason}` unless every node references only earlier nodes or declared slots, the node count is at most 4 096, every power exponent is an integer in -8 to 8, and the declared result dimension is present.
- **REQ-KRN.CONS.DERIV.FORMULA-002** The kernel shall check dimensions at every node (add, sub, min, max, sum and mean need equal dimensions; sqrt needs even exponents; trigonometric arguments need plane angle; inverse trigonometric results are plane angle) and shall return `UnitMismatch{node, left, right}` on the first violation in node order.
- **REQ-KRN.CONS.DERIV.FORMULA-003** If an input result's dimension differs from its slot's declared dimension, then the kernel shall return `InputKindMismatch{slot}` and evaluate nothing.
- **REQ-KRN.CONS.DERIV.FORMULA-004** The kernel shall evaluate nodes in index order in SI f64 using only `fid-num` operations, with variadic reductions in argument order, and shall return the value with its quantity kind and SI unit and the SI value of every converted literal.
- **REQ-KRN.CONS.DERIV.FORMULA-005** If an operation is outside its domain (division by zero, sqrt of a negative, asin or acos outside [-1, 1]) or any intermediate is non-finite, then the kernel shall return `DomainError{node, op}` or `NonFinite{node}` and no value.
- **REQ-KRN.CONS.DERIV.FORMULA-006** For the same IR and inputs, the value and output digest shall be bit-identical on x86-64, ARM64 and wasm32.
- **REQ-KRN.CONS.DERIV.FORMULA-007** The formula shall implement the `Construction` contract with its input result IDs as declared inputs, so it recomputes when any input result changes.
- **REQ-KRN.CONS.DERIV.FORMULA-008** When the command is committed, the system shall journal one `expr.evaluate_formula@1` event with algorithm ID, input element IDs, parameter digest (the IR's canonical digest) and output digest; a dry run shall return the R1 value and journal nothing.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/expr/formula-cases-v1` (120 IR cases with inputs and expected SI values computed by an independent arbitrary-precision oracle, rounded to f64), when `fid expr evaluate-formula --ir <handle> --json` runs on each, then every value is within 2 ulp of the oracle [unverified bound] and dimensions match.
- **AC-2** Given the same cases on x86-64, ARM64 and wasm32 under wasmtime, then output digests are identical.
- **AC-3** (error path) Given IRs that add a length to an angle, reference a later node, use exponent 9, or divide by a zero-valued input, then the command returns `UnitMismatch`, `IrInvalid`, `IrInvalid` and `DomainError` respectively and journals nothing.
- **AC-4** Given a ratio formula over two distance results, when one input result is replaced, then recompute through the `KRN.CONS.DECL` harness equals a fresh evaluation bit for bit.
- **AC-5** Given a literal `0.1 in`, then the result lists its SI value as the single-rounded f64 of 0.00254 m from `GEO.FRAME.UNITS`.
- **AC-6** Given a committed AC-1 case, when replayed in exact mode, then the outcome is `IDENTICAL`.

## Design

- **Registry command(s):** `expr.evaluate_formula@1`, W2 (dry-run R1). Params: `ir: FormulaIr` (canonical bytes; from `AUTO.PROG.EXPR` or a file through an approved import handle), input result handles (resolved by the bus into `ElementGeom::Scalar{value, kind}`). Result: `ConstructedElement::Value{value, kind, unit}`, `literals_si: Vec<f64>`, IR digest.
- **Data structures:** `FormulaIr { version: u16, slots: Vec<Slot { input: ElementRef, dim: Dim }>, literals: Vec<Literal { value: KDec, unit: UnitId }>, nodes: Vec<Node { op: Op, args: Vec<Arg> }>, result_dim: Dim }`; `Dim { exp: [i8; 7], angle: i8 }`; `Arg = Slot(u16) | Literal(u16) | Node(u32)`.
- **Algorithm:** validate structure and bounds; derive and check dimensions node by node; convert literals once; evaluate in index order with `fid_num::f` transcendentals and `fid_num::red` for sum and mean; check finiteness after each node.
- **Errors:** `IrInvalid`, `UnitMismatch`, `InputKindMismatch`, `DomainError`, `NonFinite`, `IrTooLarge`.
- **Determinism:** D0 (portable transcendentals in `fid-num`, fixed evaluation order, no FMA).
- **Performance budget:** 4 096-node formula in < 200 microseconds [unverified until measured].
- **Crate:** `fid-construct` (module `formula`); IR schema published for `AUTO.PROG.EXPR`.

## Test fixtures

`fixtures/expr/formula-cases-v1`: hand-written and seeded IR cases with oracle values from an arbitrary-precision library run offline in the fixture generator (outside the kernel), CC-BY-4.0.

## Build prompt

> In crate `fid-construct` (pure Rust, f64, no I/O, no `unsafe`; depends on `fid-num` and the unit catalogue crate of GEO.FRAME.UNITS only; never on AUTO, CORE or any command crate), add module `formula`. Define `FormulaIr` (slots with expected dimension, exact-decimal literals with unit, nodes in topological order, declared result dimension; canonical byte encoding) and `Dim { exp: [i8;7], angle: i8 }`. Implement validation (references only to earlier nodes or declared slots, <= 4096 nodes, integer powers in -8..=8), per-node dimension checking, one-rounding literal conversion via the unit catalogue, and evaluation in index order with ops add, sub, mul, div, neg, abs, sqrt, powi, min, max (total order), sum, mean (fixed order via `fid_num::red`), hypot, sin, cos, tan, asin, acos, atan, atan2 (via `fid_num::f`). Errors: `IrInvalid`, `UnitMismatch`, `InputKindMismatch`, `DomainError`, `NonFinite`, `IrTooLarge`. Implement the `Construction` trait with input result IDs as declared inputs, algorithm `expr.evaluate_formula.ir@1.0.0`. Add the registry data row `expr.evaluate_formula@1` (W2, dry-run R1, D0; CLI `fid expr evaluate-formula`; MCP `expr_evaluate_formula`; one journal event with algorithm ID, input element IDs, IR digest as parameter digest, output digest) and a spec card. Tests: `fixtures/expr/formula-cases-v1` within 2 ulp of oracle, cross-target digests on x86-64/ARM64/wasm32, the four error cases, recompute harness, literal conversion, exact replay IDENTICAL. KNP-1: no FMA, transcendentals via `fid_num::f`, no `f64::min`/`max`, no HashMap/HashSet, no SIMD dispatch; deny `unwrap`/`expect`/`panic`. Acceptance: AC-1..AC-6 of spec node KRN.CONS.DERIV.FORMULA.

## Open questions

- `AUTO.PROG.EXPR` must compile to this IR and should depend on this node; its "share grammar and checker" note is answered by sharing the IR and dimension rules, not the parser.
- UNC decides whether formula results start at UEC-0 or get propagated uncertainty (D-027); the kernel result carries no uncertainty field.
- The ResultRecord `evaluation_basis` for formula results (likely `RESULTS_ONLY`) is for `CORE` and `UNC` to confirm.

## Children

_Leaf: none._
