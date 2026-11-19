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
built_in: [fid-solv]
built_pr: 218
satisfies: [REQ-KRN.SOLV-001, REQ-KRN.SOLV-002, REQ-KRN.SOLV-009]
covers: []
depends_on: [KRN.NUM.LA, KRN.NUM.RED, KRN.NUM.ERR]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-19
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

- **Registry command(s):** none; library API `fid_solv::nlls::{solve, NllsProblem, NllsConfig, NllsResult, SolverError, Diagnostics, Method, StopReason, Stage}`.
- **Data structures:** `trait NllsProblem { fn n_params(&self) -> usize; fn n_resid(&self) -> usize; fn eval(&self, p: &[f64], r: &mut [f64], j: &mut [f64]) -> Result<(), NumError>; }` with `j` row-major m x n (a caller buffer, so the callback fills it in place; `fid_num::la::Mat` has no mutable access and caps rows at 4096); `NllsConfig { method, max_iter, tol_grad, tol_step, tol_cost, cond_limit, rank_tol, lm_tau }` with explicit `Default` values echoed in the result.
- **Defaults (pinned):** method LM, max_iter 200, tol_grad 1e-10, tol_step 1e-12, tol_cost 1e-14, cond_limit 1e12, rank_tol 1e-12, lm_tau 1e-3. Bounds: max_iter <= 1 000 000; tolerances finite and >= 0; cond_limit finite and >= 1; rank_tol in [0, 1); lm_tau finite and > 0; 1 <= n <= 4096; m >= n; (m + n) n <= 2^24. Out of range is `InvalidConfig { field }`; a non-finite p0 is `NonFinite { stage: Start }`.
- **Algorithm:** Gauss-Newton step from QR least-squares solve of J dp = -r. LM: solve the augmented system [J; sqrt(lambda) D] dp = [-r; 0] by QR, with D the column norms of J (Moré 1978, "The Levenberg-Marquardt algorithm: implementation and theory"); gain ratio rho decides acceptance; lambda update by the fixed factors (x1/3 on rho > 0.75, x2 on rho < 0.25, documented).
- **Pinned rules (normative; `fixtures/solv/make_nlls_fixtures.py` models them bit for bit).** cost = `red::sum_sq(r)` (no factor 1/2). Each iterate is assessed: (1) d_j = `red::norm2` of Jacobian column j (1.0 if zero), Js = J / d column by column; (2) column-pivoted Householder QR of Js with right-hand side -r, following the KRN.NUM.LA.QR rules (norms of the trailing columns recomputed with `red::norm2` at every step, strict total-order maximum so ties go to the lowest index, whole-column swaps, beta = -xnorm if a_kk > 0 else +xnorm, v = [1, x[1..] / (alpha - beta)], tau = (beta - alpha) / beta, reflection c -= (tau * `red::dot`(v, c)) * v applied to later columns in ascending order and then to the right-hand side); rank = first k with |r_kk| <= rank_tol * max |r_jj| (else n); cond = `la::cond_triangular(R, 0)`, absent if not estimable; rank < n is `RankDeficient`, cond absent or > cond_limit is `IllConditioned`, checked at every iterate including the returned one; (3) stop tests in order: a pending stop returns; cost == 0 is `ZeroResidual`; max_j |`red::dot`(Js col j, r)| <= tol_grad * `red::norm2`(r) is `Gradient`; max_iter accepted steps is `IterationLimit`. Step: GN solves Js z = -r from the same factor by `la::solve_upper` (rel_tol 0) and un-permutes; LM starts at lambda = lm_tau (the scaled cross-product has unit diagonal) and each trial factors [Js; `f::sqrt`(lambda) I] with right-hand side [-r; 0] afresh, at most 64 trials per accepted step. Trial: p_t = p + z / d; pred = `red::sum_sq`(Js z) + (2 lambda) `red::sum_sq`(z) (lambda = 0 for GN); act = cost - cost_t. GN accepts every trial; LM, if pred > 0, sets rho = act / pred, accepts iff rho > 1e-4, divides lambda by 3 if rho > 0.75, multiplies it by 2 if rho < 0.25, clamped to [1e-200, 1e200]; pred == 0 rejects and keeps lambda. After an accepted trial, `red::norm2`(z) <= tol_step (`red::norm2`(d * p_new) + tol_step) sets pending `Step`; then, after any trial with no pending stop, pred <= tol_cost cost and |act| <= tol_cost cost (pre-trial cost) sets pending `Cost`. 64 rejected trials without a stop is `IterationLimit`. `iterations` counts accepted steps, `evaluations` callback calls.
- **Errors:** `SolverError::{IterationLimit, RankDeficient { rank, cond }, IllConditioned { cond }, NonFinite { stage, index }, InvalidConfig { field }, Numeric { error }}`, each with last `Diagnostics` (iterations, evaluations, cost, grad_norm, rank, cond, lambda; unreached fields absent) and no p. `Numeric` covers a callback `Err`, an allocation failure (all buffers reserved with `try_reserve_exact`) and an internal overflow.
- **Determinism:** D0; fixed-path QR, fixed-order reductions, no threads.
- **Performance budget:** 10 000-point cylinder-sized problem (5 parameters) converging in 10 iterations < 20 ms on reference hardware [unverified until measured].
- **Crate:** `fid-solv` (module `nlls`). The QR step stays private to the module (`nlls::qr`). `KRN.NUM.LA.QR` is built in `fid-num`, and the unit test `nlls::qr::tests::matches_public_qr_pivoted` proves the private factor bit-identical to `fid_num::la::qr_pivoted` plus `Qr::apply_qt` (R, permutation and the first n entries of Q^T b). Follow-up: move the driver onto the public QR once `fid-num` offers a workspace-based entry point (caller-owned buffers reused across iterations, no per-call allocation) without the 4096-row `Mat` cap; until then the private step is kept, and any change to it is re-checked against that test and the fixtures.

## Test fixtures

`fixtures/solv/nist-strd-nls-v1`: NIST Statistical Reference Datasets, nonlinear regression (US government work, public domain [unverified]); converted by the committed `convert.py` for local runs only, because D-038 keeps third-party reference data out of the repository until its terms are settled. CI uses `fixtures/solv/strd-models-synth-v1.txt` instead: seeded data on eight StRD model forms (Misra1a, Chwirut, DanWood, Lanczos3 of lower difficulty; Kirby2, Misra1d, Roszman1, MGH17 of average difficulty) with two starts each and 50-digit reference solutions, CC-BY-4.0. `fixtures/solv/circle-synth-v1`: seeded synthetic generator, CC-BY-4.0.

## Build prompt

> Create crate `fid-solv` (pure Rust, f64, no I/O, no `unsafe`; depends only on `fid-num`) with module `nlls`. Define `trait NllsProblem` (residuals and Jacobian into caller buffers, returning `Result<(), NumError>`), `NllsConfig` (method GN|LM, max_iter, tol_grad, tol_step, tol_cost, cond_limit, lm_tau; explicit defaults echoed in the result), `NllsResult` (p, residuals, cost, grad_norm, rank, cond, iterations, stop_reason) and `SolverError` (IterationLimit, RankDeficient, IllConditioned, NonFinite, InvalidConfig, each with last diagnostics). Steps come from `fid_num::la` QR least-squares solves; LM uses the augmented system [J; sqrt(lambda) D] with column-norm scaling and fixed gain-ratio rules; never form J^T J. Single-threaded; all sums via `fid_num::red`; transcendentals via `fid_num::f`. Tests: NIST StRD nonlinear datasets `fixtures/solv/nist-strd-nls-v1` (certified values, both starts), synthetic circle `fixtures/solv/circle-synth-v1`, cross-target digest equality on x86-64/ARM64/wasm32, rank-deficient and iteration-limit error paths. KNP-1: no FMA/`mul_add`, no `powi`, no `f64::min`/`max`, no HashMap/HashSet, no SIMD dispatch; deny `unwrap`/`expect`/`panic`. Acceptance: AC-1..AC-5 of spec node KRN.SOLV.NLLS.

## Open questions

- Whether bounds or equality constraints on parameters (fixed radius, fixed axis) are handled by reparameterisation in the caller (current assumption, `KRN.LSQ`) or by the driver.

## Children

_Leaf: none._
