---
id: KRN.SOLV.SEQ
title: Sequential linearisation driver
level: 3
parent: KRN.SOLV
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
built_in: [fid-solv]
satisfies: [REQ-KRN.SOLV-001, REQ-KRN.SOLV-008, REQ-KRN.SOLV-009]
covers: []
depends_on: [KRN.SOLV.CONIC.WRAP, KRN.SOLV.CERT.AS, KRN.SOLV.CERT.CT, KRN.SOLV.MSTART, KRN.NUM.RED, KRN.NUM.FN, KRN.NUM.ERR]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-20
---

# KRN.SOLV.SEQ: Sequential linearisation driver

## Purpose

Minimum-zone, maximum-inscribed and tangential problems for curved features are non-convex in the axis or centre. This driver linearises such a problem about the current estimate, solves the resulting LP or SOCP with a step bound, updates, and repeats until the active set is stable. It is generic over a problem callback, so `KRN.MZ`, `KRN.ENV` and `GDT` share one loop, and it hands the final active set to the certificate module so that only recomputed values leave it. A multi-start entry point wraps it in `KRN.SOLV.MSTART` and sets `local_optimum_only` when starts disagree.

## Scope

**In:**
- The `LinearisableProblem` callback: build the local LP or SOCP about a point (rows labelled as for `ActiveSetProblem`), and evaluate the true objective and constraints
- The iteration loop with an infinity-norm step bound (trust region) on scaled parameters, fixed acceptance and radius-update rules
- Stop rules: active set unchanged for a configured number of consecutive iterations and step below tolerance, or iteration limit
- Hand-off of the final subproblem solution to `KRN.SOLV.CERT.CT` and return of the certified result only
- Optional outer trust region supplied by the caller (for example the MIC centre region from `KRN.ENV`) and the `on_trust_boundary` flag
- `solve_multi`: running the driver through `KRN.SOLV.MSTART` and propagating `local_optimum_only` into the certificate

**Out** (and which node owns it instead):
- The conic solves themselves (`KRN.SOLV.CONIC.WRAP`)
- Candidate extraction, recomputation, KKT checks and certificates (`KRN.SOLV.CERT`)
- Start-set generation and agreement test (`KRN.SOLV.MSTART`)
- Problem formulations per primitive (`KRN.MZ`, `KRN.ENV`, `GDT.ZONE.POSE.LIN`)

## Requirements

- **REQ-KRN.SOLV.SEQ-001** The driver shall iterate linearise-solve-update from a caller-supplied start, with a step bound whose update rule is a fixed documented function of the ratio of actual to predicted improvement.
- **REQ-KRN.SOLV.SEQ-002** The driver shall stop with success only when the active set is unchanged for 3 [unverified] consecutive iterations and the step norm is below the configured tolerance.
- **REQ-KRN.SOLV.SEQ-003** If the iteration limit is reached, a subproblem is infeasible, or the step bound shrinks below its minimum, then the driver shall return a typed `SolverError` with the last active set and diagnostics, and no value.
- **REQ-KRN.SOLV.SEQ-004** If the caller supplies an outer trust region and the converged point lies on its boundary, then the driver shall report `on_trust_boundary` so the caller can raise its own typed error.
- **REQ-KRN.SOLV.SEQ-005** On success, the driver shall pass the final active set to `KRN.SOLV.CERT` and return only the certified result, never its own last iterate.
- **REQ-KRN.SOLV.SEQ-006** The step bound shall be the box |dp_j| <= radius * scale_j appended as the last rows of each subproblem, so an LP subproblem stays an LP, and those rows shall be removed from the solution passed to certification.
- **REQ-KRN.SOLV.SEQ-007** The radius update shall be: if rho < 0.25 then radius = radius / 4; else if rho > 0.75 and the bound is active then radius = min(2 radius, radius_max); otherwise unchanged; and a step shall be accepted only when rho > 0.1 [unverified constants], where rho is the ratio of actual to predicted objective decrease.
- **REQ-KRN.SOLV.SEQ-008** The stability count and step tolerance shall be fields of `SeqConfig` supplied by the caller, with documented defaults (3 iterations, 1e-12 relative), so a caller can set them per quantity kind.
- **REQ-KRN.SOLV.SEQ-009** When `solve_multi` runs, the driver shall solve from every start of `KRN.SOLV.MSTART` in its fixed order, return the selected certified result, and set `local_optimum_only` on its certificate when MSTART reports disagreement.
- **REQ-KRN.SOLV.SEQ-010** The driver shall be single-threaded, use only `fid-num` for floating-point primitives, and give bit-identical results on x86-64, ARM64 and wasm32.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/solv/seq/ref-v1` (minimum-zone circle, maximum-inscribed circle and minimum-zone cylinder axis problems, 20 each, seeded synthetic points with planted form error and 50-digit reference optima), when `solve` runs from the least-squares start, then each certified value matches the reference within 1e-12 relative [unverified] and the active set equals the reference contact set.
- **AC-2** (path independence) Given each AC-1 problem started from two different seeded starts that converge to the same active set, then the certified value and certificate digest are bit-identical.
- **AC-3** (error path) Given a test-only problem whose subproblem is infeasible at the start, then `SolverError { cause: SubproblemInfeasible, .. }` with the last active set; given iteration limit 2, then `IterationLimit`; given radius_min larger than radius_0, then `StepBoundCollapsed`.
- **AC-4** (outer region) Given a MIC circle problem whose true centre lies outside a caller box, then the outcome reports `on_trust_boundary = true`.
- **AC-5** (multi-start) Given a cylinder problem with two local optima reachable from different starts, when `solve_multi` runs with 8 seeded starts, then the result carries `local_optimum_only = true`; given a problem where all starts agree, it is `false`.
- **AC-6** Given AC-1's fixture on x86-64, ARM64 and wasm32 under wasmtime, then the digest of all certified outputs is identical.

## Design

- **Registry command(s):** none; library API `fid_solv::seq::{solve, solve_multi, LinearisableProblem, SeqConfig, SeqOutcome}`.
- **Data structures:** `trait LinearisableProblem: ActiveSetProblem { fn scales(&self) -> Vec<f64>; fn local_problem(&self, p: &[f64]) -> Result<(ConicProblem, f64), SolverError>; fn objective(&self, p: &[f64]) -> Result<f64, SolverError>; fn outer_region(&self) -> Option<Vec<(f64, f64)>>; }` (the f64 is the model objective at zero step); `SeqConfig { max_iter: 100, stable_iters: 3, step_tol_rel: 1e-12, radius_0, radius_min, radius_max, thresholds, cert: CertConfig }`; `SeqOutcome { certified: Certified, iterations, on_trust_boundary: bool, history: Vec<IterSummary> }`, where history holds diagnostics only and never enters a digest.
- **Algorithm:**
  1. Start at p0.
  2. Build the local problem and append the step-bound rows, intersected with the outer region.
  3. Solve it with `conic::solve_lp` or `conic::solve_socp`.
  4. Compute rho from the true and model objectives; accept or reject the step and update the radius by REQ-007.
  5. Classify the strongly active rows with `cert::active` and compare the set with the previous iteration.
  6. Repeat until the stop rule holds.
  7. Strip the step-bound rows and call `cert::certify` on the final subproblem solution.
  `solve_multi` passes a closure over `solve` to `mstart::run`. Reference: Conn, Gould and Toint, Trust-Region Methods, SIAM 2000 [unverified chapter].
- **Errors:** `SolverError::{IterationLimit, SubproblemInfeasible, StepBoundCollapsed, Conic(..), Cert(..)}`, each carrying the last active set, radius and iteration count.
- **Determinism:** D0.
- **Performance budget:** minimum-zone cylinder on 10 000 points < 200 ms [unverified until measured].
- **Crate:** `fid-solv` (module `seq`).

## Test fixtures

`fixtures/solv/seq/ref-v1`: committed seeded generator with planted geometry and 50-digit reference optima computed offline (for example by exhaustive contact-set search in `mpmath` on small point counts); CC-BY-4.0. Test-only `LinearisableProblem` implementations for circle, MIC circle and cylinder axis live in the test module, not in `KRN.MZ` or `KRN.ENV`.

## Build prompt

> In crate `fid-solv`, add module `seq` with trait `LinearisableProblem: cert::recomp::ActiveSetProblem` (scales, local_problem(p) returning a labelled `ConicProblem` and the model objective at zero step, objective(p), optional outer_region box), `SeqConfig` (max_iter 100, stable_iters 3, step_tol_rel 1e-12, radius_0, radius_min, radius_max, active-set thresholds, `CertConfig`), and `solve(problem, start, cfg) -> Result<SeqOutcome, SolverError>`. Loop: build the local problem; append the step bound |dp_j| <= radius*scale_j (intersected with the outer region) as the last rows; solve via `fid_solv::conic`; rho = actual/predicted decrease; accept when rho > 0.1; radius /4 when rho < 0.25, x2 (capped) when rho > 0.75 and the bound is active; classify strongly active rows with `cert::active`; stop with success when the set is unchanged for `stable_iters` iterations and the step norm is below tolerance; errors `IterationLimit`, `SubproblemInfeasible`, `StepBoundCollapsed` with last active set and diagnostics. On success, strip the step-bound rows from the solution and return `cert::certify`'s result only (never the iterate), with `on_trust_boundary` when the point lies on the outer region boundary. Add `solve_multi(problem, StartSpec, cfg)` that runs `mstart::run` over `solve` and calls `set_local_optimum_only()` on the certificate when starts disagree. Tests: `fixtures/solv/seq/ref-v1` (MZ circle, MIC circle, MZ cylinder axis), two-start bit identity, error cases, outer-region flag, multi-start flag, cross-target digest on x86-64, ARM64 and wasm32 under wasmtime. Constraints (CONSTITUTION §4 KNP-1, §8): single-threaded, only `fid-num` float primitives, fixed-order reductions, total-order min/max, no FMA, no HashMap/HashSet, no `unsafe`, no I/O; deny `unwrap`/`expect`/`panic`/`todo`/`unimplemented`/`unreachable`. Acceptance: AC-1..AC-6 of spec node KRN.SOLV.SEQ.

## Open questions

- The active-set stability count and step tolerance may need to be per quantity kind (REP-1 table in the concept doc) [unverified]. **Applied as a leaf (2026-11-17):** both are `SeqConfig` fields set by the caller (REQ-008), with defaults until REP-1 is adopted in the constitution.
- `GDT.ZONE.POSE.LIN` was written against the open `LinearisableProblem` interface; it must provide `scales` and label its rows as `ActiveSetProblem` labels.
- The `history` diagnostics are useful for debugging but are not values of record; callers must not journal them as results.
- **Applied at build (KRN.SOLV.SEQ):**
  - *Null step.* A step whose bound is inactive and whose predicted decrease `f(p) - m(dp)` is at most `decrease_tol_rel * data_scale` (new `SeqConfig` field, default `1e-9` [unverified]) is not taken: the point and radius stay and the step norm is 0. The conic solver's tolerances are `1e-10`, so the IPM step at a converged point is noise of that size and could never meet the `1e-12` step tolerance of REQ-008 on its own; without this rule the radius would oscillate at the noise floor. REQ-002 is unchanged: success still needs the active set stable for `stable_iters` iterations and a step norm below `step_tol_rel * (1 + max_j |p_j| / scale_j)`.
  - *Predicted decrease.* `rho = (f(p) - f(p+)) / (f(p) - (m0 + q'dp))`, with `f` = `objective(p)` and `m0` the model at zero step. For zone problems `objective` is the width with the radius parameters eliminated (defined at slightly infeasible radii), which keeps the predicted decrease nonnegative. Undefined `rho` (predicted `<= 0`) shrinks the radius. The step bound is active when some `|dp_j| >= 0.999 * radius * scale_j`.
  - *`on_trust_boundary`.* Set when the final point lies within `boundary_tol_rel * scale_j` (new field, default `1e-6` [unverified]) of a finite end of the outer region. Certification is still run on the unconstrained problem (fail closed). If the converged point is a KKT point of it, the result is returned with the flag. Otherwise the run is a `Cert` error that carries `on_trust_boundary = true` and no value. AC-4 tests both: a box edge through the true MIC centre (certified, flag set) and a box that excludes it (error, flag set).
  - *Signatures.* `LinearisableProblem` callbacks return `Result<_, CertError>`, like `ActiveSetProblem`. `outer_region` defaults to `None`. `SeqConfig` also holds `kind: CertKind` for `certify`. `SolverError` is a struct `{ cause: SeqCause, active_set, radius, iterations, on_trust_boundary }`. `SeqCause` has the five causes of the Design plus `InvalidInput`, `Callback` and `Numeric`, and a stable `code()`. `solve_multi(problem, &StartSpec, cfg)` returns mstart's `MultiStartOutcome<SeqOutcome, SolverError>`. The starts are compared by their **certified** value and parameters, so `local_optimum_only` never depends on an iterate.
  - *Classification.* The per-iteration active set is the `strongly_active` list of `cert::active::extract` on the stripped subproblem, with `SeqConfig::thresholds`.
- **For routing to other leaves (not changed here):**
  - `KRN.SOLV.CERT.AS`: `extract` accepts only nonnegative-orthant rows, so an SOCP subproblem can be solved but not classified or certified. SEQ fails it closed with `Cert(InvalidInput { field: "cones" })`. SOCP extraction needs a rule there before `KRN.ENV` uses SOCP subproblems.
  - `KRN.SOLV.CERT.RECOMP`: the Gauss-Newton convergence test (last step `<= 4 u ||p||`) sits at the rounding-noise floor of a plain `f64` residual. An uncentred cylinder with condition near 10 failed it in 2 of 20 cases. The SEQ test problems evaluate their residuals in double-double, which removes the failures. `KRN.MZ` cylinder and cone problems will hit the same limit, so the test should allow for `cond * u`, or the leaf should state the residual accuracy it needs.
  - The cylinder references in `fixtures/solv/seq/ref-v1` are proven strict local minima (6 independent active rows, multipliers `>= 1e-3`) with no rival strict local minimum within one contact exchange. They are not proven global. The circle references are global (exact exhaustive search).

## Children

_Leaf: none._
