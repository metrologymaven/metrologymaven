---
id: KRN.SOLV.CONIC.WRAP
title: Kernel conic API, fixed limits and error mapping
level: 4
parent: KRN.SOLV.CONIC
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
satisfies: [REQ-KRN.SOLV.CONIC-003, REQ-KRN.SOLV.CONIC-004]
covers: []
depends_on: [KRN.SOLV.CONIC.PATCH, KRN.NUM.ERR]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# KRN.SOLV.CONIC.WRAP: Kernel conic API, fixed limits and error mapping

## Purpose

The only door from the kernel into the vendored solver. It defines one canonical problem form for LP, convex QP and SOCP, validates input, runs the patched solver with iteration limits and tolerances fixed in code, exports those limits as data for the kernel manifest, and maps every non-solved status to a typed `SolverError` that carries the last residuals and no solution. Fits and certificates never see the vendored types.

## Scope

**In:**
- Canonical form: minimise 1/2 x'Px + q'x subject to Ax + s = b, s in K, with K a product of zero, nonnegative and second-order cones in a fixed listed order
- `ConicProblem` with a checked builder: sparse upper-triangular P (optional), q, sparse A (compressed column), b, cone list
- Entry points `solve_lp`, `solve_qp`, `solve_socp` that check the problem class and call one internal solve
- `ConicLimits`: max iterations, feasibility, gap and infeasibility tolerances, regularisation and refinement settings, fixed constants with a version, plus `limits_descriptor()` for `KRN.MAN.MANI`
- `ConicSolution { x, y, s, objective, iterations, residuals }` and `SolverError` with status and last residuals

**Out** (and which node owns it instead):
- The patched solver internals (`KRN.SOLV.CONIC.PATCH`)
- Active sets, recomputation and KKT checks (`KRN.SOLV.CERT`)
- Writing limits into the manifest (`KRN.MAN.MANI`)
- Problem formulations per primitive (`KRN.MZ`, `KRN.ENV`, `GDT`)

## Requirements

- **REQ-KRN.SOLV.CONIC.WRAP-001** The builder shall reject, with a typed `InvalidProblem` error naming the field, non-finite entries, dimension mismatches, cone dimensions that do not sum to the row count of A, second-order cones of dimension below 2, and P entries below the diagonal.
- **REQ-KRN.SOLV.CONIC.WRAP-002** `solve_lp` shall reject problems with P present or any second-order cone, `solve_qp` problems with second-order cones, and `solve_socp` problems with P present, each with `WrongProblemClass`.
- **REQ-KRN.SOLV.CONIC.WRAP-003** The wrapper shall run the solver single-threaded with the `ConicLimits` constants compiled into the crate, and shall expose no parameter by which a caller can change them.
- **REQ-KRN.SOLV.CONIC.WRAP-004** `limits_descriptor()` shall return the limits' name, version and every constant as an IEEE-754 hex string, so the manifest records them exactly.
- **REQ-KRN.SOLV.CONIC.WRAP-005** If the solver ends in primal infeasibility, dual infeasibility, the iteration limit, insufficient progress or a numerical failure, then the wrapper shall return `SolverError { cause, iterations, primal_residual, dual_residual, gap }` and no solution.
- **REQ-KRN.SOLV.CONIC.WRAP-006** A returned `ConicSolution` shall carry primal x, dual y, slack s, objective, iteration count and final residuals, and every value in it shall be finite.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/solv/wrap/small-v1` (an LP for the minimum-zone straightness of 10 points, a QP projection onto a box, and an SOCP for the minimum circumscribed circle of 8 points, with optima computed offline), when solved, then each objective matches within 1e-9 relative and the status is solved.
- **AC-2** (error path) Given an LP with a NaN in b, then `InvalidProblem { field: "b" }`; given an SOC of dimension 1, `InvalidProblem { field: "cones" }`; given an SOCP passed to `solve_lp`, `WrongProblemClass`.
- **AC-3** (infeasible) Given x >= 1 and x <= 0, then `SolverError { cause: PrimalInfeasible, .. }` with finite residuals and no solution.
- **AC-4** (limit) Given a test-only build with max iterations 2 on AC-1's SOCP, then `SolverError { cause: IterationLimit, iterations: 2, .. }`.
- **AC-5** Given `limits_descriptor()`, then it lists every constant as hex, and two builds give identical bytes.

## Design

- **Registry command(s):** none; library API `fid_solv::conic::{ConicProblem, ConicProblemBuilder, Cone, solve_lp, solve_qp, solve_socp, ConicSolution, ConicLimits, limits_descriptor, SolverError, ConicCause}`.
- **Data structures:** `Csc { nrows, ncols, colptr: Vec<usize>, rowval: Vec<usize>, nzval: Vec<f64> }` with sorted row indices per column; `Cone::{Zero(usize), Nonneg(usize), Soc(usize)}`; `ConicLimits { version: u16, max_iter: u32, tol_feas: f64, tol_gap_abs: f64, tol_gap_rel: f64, tol_infeas: f64, static_reg: f64, refine_steps: u8 }` with initial values max_iter 200, tolerances 1e-10 [unverified; tune on XTEST].
- **Algorithm:** validate, convert to the patched solver's types, call solve once, map status, copy outputs, check finiteness.
- **Errors:** `InvalidProblem { field }`, `WrongProblemClass`, `SolverError { cause: PrimalInfeasible | DualInfeasible | IterationLimit | InsufficientProgress | Numerical, .. }`, all convertible to the `KRN.SOLV` `SolverError`.
- **Determinism:** D0.
- **Performance budget:** SOCP with 1000 points and 3 unknowns < 20 ms [unverified until measured].
- **Crate:** `fid-solv` (module `conic`).

## Test fixtures

`fixtures/solv/wrap/small-v1`: synthetic problems from a committed generator with seeds; optima computed offline with an independent solver at high precision; CC-BY-4.0.

## Build prompt

> In crate `fid-solv`, add module `conic` wrapping the vendored `fid-clarabel` crate. Define the canonical form (min 1/2 x'Px + q'x, Ax + s = b, s in K; K = ordered list of `Cone::{Zero, Nonneg, Soc}`), a checked `ConicProblemBuilder` (reject non-finite entries, dimension mismatches, cone sizes not summing to rows of A, SOC dimension < 2, P entries below the diagonal: `InvalidProblem { field }`), and `solve_lp`, `solve_qp`, `solve_socp` (class checks: `WrongProblemClass`). Run single-threaded with compiled-in `ConicLimits` (no caller override) and export `limits_descriptor()` with every constant as IEEE-754 hex. Map non-solved statuses to `SolverError { cause, iterations, primal_residual, dual_residual, gap }` with no solution; return `ConicSolution { x, y, s, objective, iterations, residuals }` with all-finite values. Tests: `fixtures/solv/wrap/small-v1` (objectives within 1e-9 relative), AC-2..AC-5 cases. Constraints (CONSTITUTION §4 KNP-1, §8): only `fid-num` for float primitives, no FMA, no HashMap/HashSet, no `unsafe`, no I/O; deny `unwrap`/`expect`/`panic`/`todo`/`unimplemented`/`unreachable`. Acceptance: AC-1..AC-5 of spec node KRN.SOLV.CONIC.WRAP.

## Open questions

- Limit values (200 iterations, 1e-10 tolerances) are placeholders until `KRN.SOLV.CONIC.XTEST` and the minimum-zone problems in `KRN.MZ` are run [unverified].

## Children

_Leaf: none._
