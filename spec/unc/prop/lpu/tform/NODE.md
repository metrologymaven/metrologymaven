---
id: UNC.PROP.LPU.TFORM
title: Tangents of kernel formula IR and formula UEC start
level: 4
parent: UNC.PROP.LPU
kind: task
status: leaf
phase: P1b
size: S
owners: []
claimed_by:
satisfies: [REQ-UNC.PROP.LPU-001, REQ-UNC.PROP.LPU-004]
covers: []
depends_on: [UNC.PROP.LPU.TAN, KRN.CONS.DERIV.FORMULA, KRN.NUM.FN, UNC.UEC.CLASS]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# UNC.PROP.LPU.TFORM: Tangents of kernel formula IR and formula UEC start

## Purpose

Lets LPU propagate uncertainty through user formulas evaluated by `expr.evaluate_formula@1` (D-027). It registers a tangent provider for `expr.evaluate_formula.ir@1.0.0` that differentiates the formula IR node by node in forward mode, and reports kinks for `abs`, `min` and `max`. It also records UNC's answer to D-027: **a formula result starts at UEC-0 with reason `model_missing`**. It gets a propagated uncertainty only when an Uncertainty Model containing the formula operator is bound to it and evaluated. Its class is never inherited from the classes of its input results.

## Scope

**In:**
- Forward-mode derivative rules for every IR operation: add, sub, mul, div, neg, abs, sqrt, integer power, min, max, sum, mean, hypot, sin, cos, tan, asin, acos, atan, atan2
- Kink hints: `abs` at 0, `min` and `max` at ties, with the margin in the argument's SI unit
- Domain-edge errors: sqrt at 0, asin and acos at plus or minus 1, div by 0 return `no_derivative` with the node index
- The D-027 starting-class rule, as an input to `UNC.UEC.CLASS` (basis `none`, so `model_missing`)

**Out:**
- IR validation and evaluation (owned by `KRN.CONS.DERIV.FORMULA`)
- Covariance between formula inputs (declared in the model, `UNC.MODEL`; shared features are handled by `UNC.PROP.MVAR`)
- Combination and flags (owned by `UNC.PROP.LPU.COMB`)

## Requirements

- **REQ-UNC.PROP.LPU.TFORM-001** The provider shall return the Jacobian of the formula result with respect to every input slot, computed by forward-mode rules in IR node order using only `fid-num` operations.
- **REQ-UNC.PROP.LPU.TFORM-002** When an `abs` argument, or the difference between the two leading `min` or `max` arguments, has magnitude below its kink margin input, the provider shall return a kink hint naming the node and the margin.
- **REQ-UNC.PROP.LPU.TFORM-003** If the derivative is undefined at the evaluation point (sqrt of 0, asin or acos of plus or minus 1, division by 0), then the provider shall return `no_derivative` naming the operator and the node index.
- **REQ-UNC.PROP.LPU.TFORM-004** When a formula result has no bound Uncertainty Model, the class input produced for it shall be basis `none`, so that `UNC.UEC.CLASS` assigns UEC-0 with reason `model_missing`, whatever the classes of its input results.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/unc/lpu/tform/ratio.json` (formula `A / B` with A = 0.1 m and B = 0.05 m), when the provider runs, then the Jacobian is [20 m^-1, -40 m^-1] to 1 ulp.
- **AC-2** Given `fixtures/unc/lpu/tform/all_ops.json` (one formula per operation at regular points), when checked with the `UNC.PROP.LPU.TAN` harness, then every provider entry passes.
- **AC-3** Given `max(A, B)` with A - B = 1e-9 m and margin 1e-6 m, then a kink hint is returned for that node.
- **AC-4** (error path) Given `sqrt(A - A)`, then `no_derivative{operator: "expr.evaluate_formula.ir@1.0.0", node: 1}`.
- **AC-5** Given a formula result whose two inputs are UEC-2 and no bound model, when the class input is built and assigned, then the class is UEC-0 with reason `model_missing`.

## Design

- **Registry command(s):** none; provider registered in `TangentTable`.
- **Data structures:** `FormulaTangent` holding the IR schema version; per-node tangent vectors over input slots (dense, slot order).
- **Algorithm:** evaluate values and tangents together node by node (dual numbers over slot count). min, max, sum and mean reduce in argument order; ties in min and max pick the first argument, as the kernel evaluator does.
- **Errors:** `no_derivative{operator, node}`.
- **Determinism:** D0.
- **Performance budget:** 4 096-node IR with 64 slots in under 5 ms [unverified until measured].
- **Crate:** `fid-kunc` (module `prop::lpu::tform`; KNP-1 applies).

## Test fixtures

`fixtures/unc/lpu/tform/`: hand-written IR documents in the `FormulaIr` schema (CC-BY-4.0).

## Build prompt

> In crate `fid-kunc`, module `prop::lpu::tform`, register an `OperatorTangent` provider for `expr.evaluate_formula.ir@1.0.0`. Differentiate the formula IR in forward mode node by node (dual numbers over the input slots) for add, sub, mul, div, neg, abs, sqrt, integer power, min, max, sum, mean, hypot, sin, cos, tan, asin, acos, atan and atan2, using only `fid-num` operations. Reductions go in argument order, and min or max ties pick the first argument. Return kink hints for abs near 0 and min or max near ties (below the margin input), and `no_derivative{operator, node}` where the derivative is undefined. Provide `formula_class_input(has_model: bool)`, which returns basis `none` when no model is bound, so `uec::assign` yields UEC-0 `model_missing` (D-027). Tests AC-1..AC-5 with `fixtures/unc/lpu/tform/`. Constraints: KNP-1, CONSTITUTION 8 (no panic, no unwrap).

## Open questions

- D-027 rule chosen here: no automatic model from input statements, because input correlations would be unknown and inheriting a class would fabricate evidence. An automatic "formula model template" with declared independence is a possible later `UNC.MODEL` feature; reviewers should confirm.
- Phase is P1b because the formula command is P1b.

## Children

_Leaf: none._
