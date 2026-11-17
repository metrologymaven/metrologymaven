---
id: KRN.SOLV.NLLS
title: Nonlinear least-squares driver (Gauss-Newton and Levenberg-Marquardt)
level: 3
parent: KRN.SOLV
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
satisfies: [REQ-KRN.SOLV-001, REQ-KRN.SOLV-002, REQ-KRN.SOLV-009]
covers: []
depends_on: [KRN.NUM.LA, KRN.NUM.RED, KRN.NUM.ERR]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# KRN.SOLV.NLLS: Nonlinear least-squares driver

## Purpose

The deterministic nonlinear least-squares engine that every orthogonal-distance fit and every least-squares alignment calls. It minimises a sum of squared residuals given a residual-and-Jacobian callback, solving each step by QR (never normal equations), and returns either a converged solution with diagnostics or a typed error.

## Scope

**In:** the `NllsProblem` callback trait; Gauss-Newton and Levenberg-Marquardt (Moré-style trust region with fixed update rules); column scaling; convergence tests; diagnostics (gradient norm, Jacobian rank and condition, iteration count, final cost); typed errors.
**Out:** problem formulations and parameterisations (`KRN.LSQ`, `ALN`); initial estimates and multi-start (`KRN.LSQ.INIT`, `KRN.SOLV.MSTART`); conic problems (`KRN.SOLV.CONIC`).

## Requirements

- **REQ-KRN.SOLV.NLLS-001** The driver shall take a problem object that returns a residual vector r(p) and Jacobian J(p) for parameters p, a start p0 and a `NllsConfig` (max iterations, gradient, step and cost tolerances, condition limit, method GN or LM), with no default hidden from the caller's record.
- **REQ-KRN.SOLV.NLLS-002** Each step shall be computed from a QR factorization of the (damped, scaled) Jacobian from `fid-num`, and the driver shall never form J^T J.
- **REQ-KRN.SOLV.NLLS-003** The LM damping update, step acceptance and scaling rules shall be fixed, documented functions of the current iterate only, so the iterate sequence is a deterministic function of (problem, p0, config).
- **REQ-KRN.SOLV.NLLS-004** On convergence, the driver shall return p, the final residuals, the cost, the gradient norm ||J^T r||, the Jacobian rank and condition estimate, the iteration count and the stop reason.
- **REQ-KRN.SOLV.NLLS-005** If the iteration limit is reached, the Jacobian condition estimate exceeds the configured limit, the Jacobian is rank-deficient, or the callback returns a non-finite value, then the driver shall return a typed `SolverError` naming the cause with the last diagnostics, and no solution.
- **REQ-KRN.SOLV.NLLS-006** The driver shall be single-threaded and use only `fid-num` for floating-point primitives.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/solv/nist-strd-nls-v1` (the NIST StRD nonlinear regression datasets of lower and average difficulty, both published start points), when LM runs with the default config, then the certified parameter values are reproduced to at least 6 significant digits [unverified target] for every dataset and start.
- **AC-2** Given a synthetic 2D circle problem `fixtures/solv/circle-synth-v1` (seeded, 50 points, noise 1e-6 m), when GN and LM run, then both converge to the same parameters within 1e-12 m and report rank 3.
- **AC-3** Given AC-1's inputs on x86-64, ARM64 and wasm32 under wasmtime, then the SHA-256 over all returned bit patterns (p, cost, iteration counts) is identical.
- **AC-4** (error path) Given a problem whose Jacobian has two identical columns, when the driver runs, then it returns `SolverError::RankDeficient{rank, cond}` and no p; given max_iter = 1 on a non-trivial problem, it returns `SolverError::IterationLimit` with the last cost.
- **AC-5** Given the crate source, when the KNP-1 lint runs, then it reports no finding, and a source search finds no product of J transposed with J.

## Design

- **Registry command(s):** none; library API `fid_solv::nlls::{solve, NllsProblem, NllsConfig, NllsResult, SolverError}`.
- **Data structures:** `trait NllsProblem { fn n_params(&self) -> usize; fn n_resid(&self) -> usize; fn eval(&self, p: &[f64], r: &mut [f64], j: &mut Mat) -> Result<(), NumError>; }`; `NllsConfig { method, max_iter, tol_grad, tol_step, tol_cost, cond_limit, lm_tau }` with explicit `Default` values recorded in the result.
- **Algorithm:** Gauss-Newton step from QR least-squares solve of J dp = -r. LM: solve the augmented system [J; sqrt(lambda) D] dp = [-r; 0] by QR, with D the column norms of J (Moré 1978, "The Levenberg-Marquardt algorithm: implementation and theory"); gain ratio rho decides acceptance; lambda update by the fixed factors (x1/3 on rho > 0.75, x2 on rho < 0.25, documented).
- **Errors:** `SolverError::{IterationLimit, RankDeficient, IllConditioned, NonFinite, InvalidConfig}`, each with last diagnostics.
- **Determinism:** D0; fixed-path QR, fixed-order reductions, no threads.
- **Performance budget:** 10 000-point cylinder-sized problem (5 parameters) converging in 10 iterations < 20 ms on reference hardware [unverified until measured].
- **Crate:** `fid-solv` (module `nlls`).

## Test fixtures

`fixtures/solv/nist-strd-nls-v1`: NIST Statistical Reference Datasets, nonlinear regression (US government work, public domain [unverified]); converted to JSON by a committed script. `fixtures/solv/circle-synth-v1`: seeded synthetic generator, CC-BY-4.0.

## Build prompt

> Create crate `fid-solv` (pure Rust, f64, no I/O, no `unsafe`; depends only on `fid-num`) with module `nlls`. Define `trait NllsProblem` (residuals and Jacobian into caller buffers, returning `Result<(), NumError>`), `NllsConfig` (method GN|LM, max_iter, tol_grad, tol_step, tol_cost, cond_limit, lm_tau; explicit defaults echoed in the result), `NllsResult` (p, residuals, cost, grad_norm, rank, cond, iterations, stop_reason) and `SolverError` (IterationLimit, RankDeficient, IllConditioned, NonFinite, InvalidConfig, each with last diagnostics). Steps come from `fid_num::la` QR least-squares solves; LM uses the augmented system [J; sqrt(lambda) D] with column-norm scaling and fixed gain-ratio rules; never form J^T J. Single-threaded; all sums via `fid_num::red`; transcendentals via `fid_num::f`. Tests: NIST StRD nonlinear datasets `fixtures/solv/nist-strd-nls-v1` (certified values, both starts), synthetic circle `fixtures/solv/circle-synth-v1`, cross-target digest equality on x86-64/ARM64/wasm32, rank-deficient and iteration-limit error paths. KNP-1: no FMA/`mul_add`, no `powi`, no `f64::min`/`max`, no HashMap/HashSet, no SIMD dispatch; deny `unwrap`/`expect`/`panic`. Acceptance: AC-1..AC-5 of spec node KRN.SOLV.NLLS.

## Open questions

- Whether bounds or equality constraints on parameters (fixed radius, fixed axis) are handled by reparameterisation in the caller (current assumption, `KRN.LSQ`) or by the driver.

## Children

_Leaf: none._
