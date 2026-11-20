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
built_in: [fid-solv]
built_pr: 312
satisfies: [REQ-KRN.SOLV.CONIC-003, REQ-KRN.SOLV.CONIC-004]
covers: []
depends_on: [KRN.SOLV.CONIC.PATCH, KRN.NUM.ERR]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-20
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
- **As built (crates/fid-solv, module `conic`):** the error type is `ConicError { InvalidProblem { field }, WrongProblemClass { entry, has_p, has_soc }, TooLarge { kkt_dim, kkt_nnz }, FactorisationSetup, SetupRejected { reason }, Solver(SolverError) }`; `SolverError` is the struct of REQ-005 with `cause: ConicCause`. Its residuals and gap are `Option<f64>` (`None` when the solver's last value is not finite), so no NaN leaves the API. `ConicCause` adds to the five listed causes `AlmostSolved`, `AlmostPrimalInfeasible`, `AlmostDualInfeasible` (the solver's reduced-accuracy statuses, never accepted as solved), `NotRun` and `NonFiniteOutput` (status solved but a non-finite value). Field names are `"q"`, `"A"`, `"b"`, `"cones"`, `"P"`, checked in that order; a cone of dimension 0 is also rejected. The cause is read from the solver's final status (`solution.status`, after the PATCH demotion of a result or certificate without finite values of record to `NumericalError`), through an exhaustive `cause_of` table. The `Almost*` causes can also end a run that hit the iteration limit, stalled or failed numerically, because the solver then checks the reduced tolerances; every `Almost*` cause is an error.
- **As built, size budget:** before setup the wrapper checks the KKT bounds that `fid-clarabel` refuses as `BadInputData` (patch 0006, `MAX_ORDERING_DIM`/`MAX_ORDERING_NNZ`) and returns `TooLarge`; if the logical LDL factorisation could not be set up (the fill budget `MAX_ORDERING_FILL`), it returns `FactorisationSetup` without running the solver. Any other setup refusal is `SetupRejected`.
- **As built, limits:** `ConicLimits::FIXED` (version 1) holds the placeholder values above (200 iterations, tolerances 1e-10, static regularisation 1e-8, 10 refinement steps). The wrapper writes every other solver setting out explicitly (presolve off, since the builder rejects non-finite `b`), and `limits_descriptor()` lists all 39 constants (floats as IEEE-754 hex, integers and flags as JSON values, the three ordering budgets included) as RFC 8785 canonical JSON. AC-4's test-only limits are a crate-internal unit test; no public entry point takes limits.
- **As built, output pins:** the crate's own output bits on `small-v1` are pinned (SHA-256) in `tests/conic.rs`, one table for all targets, checked by CI on x86-64, ARM64 and wasm32; this is a regression pin, not part of the oracle. The cross-target test set itself stays with `KRN.SOLV.CONIC.XTEST`.
- **As built, convexity of P:** `P` must be positive semidefinite; this is a documented precondition of `ConicProblemBuilder::p`. The builder checks only a necessary condition: no negative diagonal entry (field `"P"`). An indefinite `P` with a nonnegative diagonal is accepted, and the solver's result on it is undefined.
- **Open: full PSD check of P.** Before any caller may pass a `P` it did not construct to be PSD, the wrapper needs a full check, for example an exact LDLᵀ inertia test, or a certified lower bound on the smallest eigenvalue. Until then, only problem formulations that build `P` PSD by construction (`J'J`, a nonnegative diagonal) may call `solve_qp` with `P`.
- **As built, `FactorisationSetup`:** the branch is defensive. The only known way to reach it is an elimination graph past `MAX_ORDERING_FILL` within the dimension and entry budgets, which costs too much to build in a debug or wasm32 CI test.
- **Follow-up:** there is no `KRN.SOLV`-wide `SolverError` type yet (each module has its own), so the conversion the Design mentions is not written; add it when `KRN.SOLV.CERT` defines the shared type. The performance budget (1000 points, 3 unknowns, < 20 ms) is not measured [unverified].

## Children

_Leaf: none._
