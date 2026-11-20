---
id: KRN.SOLV.CERT.KKT
title: Independent KKT optimality check at the recomputed point
level: 4
parent: KRN.SOLV.CERT
kind: task
status: leaf
phase: P0
size: S
owners: []
claimed_by:
built_in: [fid-solv]
built_pr: 324
satisfies: [REQ-KRN.SOLV.CERT-003]
covers: []
depends_on: [KRN.SOLV.CERT.RECOMP, KRN.NUM.LA.QR, KRN.NUM.RED, KRN.NUM.FN, KRN.NUM.ERR]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# KRN.SOLV.CERT.KKT: Independent KKT optimality check at the recomputed point

## Purpose

Checks that the recomputed point is optimal without trusting the solver. At the recomputed parameters it evaluates every constraint (primal feasibility), solves for the multipliers of the active constraints from the stationarity equations (not taken from the solver), and checks their signs (dual feasibility), the stationarity residual and complementarity. The residuals and tolerances go into the certificate so a verifier can repeat the same check.

## Scope

**In:**
- Primal feasibility: max over all rows of the constraint value, which must be at most tol_p
- Multipliers: least-squares solve of objective gradient = sum of lambda_i times active constraint gradients, by QR
- Stationarity residual norm, dual feasibility (min lambda_i >= -tol_d) and complementarity (max |g_i| over active rows <= tol_c)
- Tolerances scaled by recorded data scales; `KktReport { multipliers, primal, dual, stationarity, complementarity, tolerances, pass }`

**Out** (and which node owns it instead):
- The callback and recomputation (`KRN.SOLV.CERT.RECOMP`)
- Raising `certificate_failed` and assembling the certificate (`KRN.SOLV.CERT.CT`)
- Verifier-side rechecking (`TRUST`)

## Requirements

- **REQ-KRN.SOLV.CERT.KKT-001** The check shall compute multipliers by a QR least-squares solve of the stationarity equations over the active constraint gradients at the recomputed point, using no solver multipliers.
- **REQ-KRN.SOLV.CERT.KKT-002** The check shall evaluate every constraint, active or not, at the recomputed point and record the maximum violation as the primal residual.
- **REQ-KRN.SOLV.CERT.KKT-003** The report shall set `pass = true` only when the primal residual is at most tol_p, the minimum multiplier is at least -tol_d, the stationarity residual norm is at most tol_s and the complementarity residual is at most tol_c, each tolerance being recorded with its scale.
- **REQ-KRN.SOLV.CERT.KKT-004** If the active gradient matrix is rank deficient under the recorded tolerance, then the check shall return `pass = false` with reason `degenerate_gradients` rather than an arbitrary multiplier vector.
- **REQ-KRN.SOLV.CERT.KKT-005** The report shall be bit-identical on x86-64, ARM64 and wasm32 for the same inputs.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/solv/cert/kkt-v1` (optimal minimum-zone line, plane and circle sets with exact multipliers, and perturbed non-optimal sets), when the check runs, then optimal sets pass with multipliers within 1e-12 relative of the reference, and non-optimal sets fail with the expected failing condition.
- **AC-2** Given an active set where one point should have been inactive (negative multiplier -0.2), then `pass = false` with dual residual 0.2 recorded.
- **AC-3** Given a recomputed point that violates an inactive row by 1e-6 with tol_p 1e-12, then `pass = false` on primal feasibility.
- **AC-4** Given AC-1's fixture on x86-64, ARM64 and wasm32 under wasmtime, then report digests are identical.

## Design

- **Registry command(s):** none; library API `fid_solv::cert::kkt::{check, KktReport, KktTolerances, KktStatus, KktFail, Verdict, Condition, KktScales}`.
- **Signature:** `check(problem: &P, rec: &Recomputed, tol: &KktTolerances) -> Result<KktReport, CertError>` with `P: ActiveSetProblem + ?Sized`. The check runs on a `Recomputed` (`KRN.SOLV.CERT.RECOMP`), whose box `X = params +- param_bounds` is proven to hold the exact root `p*`; every condition is proven at `p*` over `X` in outward-rounded `Interval` arithmetic, never estimated at the float point.
- **Trait obligations** (`ActiveSetProblem`, all fail closed by default): `data_scale()` (largest magnitude of the constraint data, in the units of `g_i`; default `InvalidInput { field: "data_scale_unset" }`), `constraint_enclosure(X)` (every row), `objective_gradient_enclosure(X)` and `constraint_gradient_enclosure(set, X)` (row-major); a missing enclosure is `BoundNotProven { NoEnclosure }`.
- **Sign convention:** constraints are feasible when `g_i <= 0`; the multipliers solve the Lagrangian stationarity equations `G^T lambda = -grad f` (G rows = active constraint gradients in set order), so `lambda >= 0` at an optimum.
- **Data structures:** `KktTolerances { tol_p, tol_d, tol_s, tol_c, rank_tol }` (relative; defaults 1e-9 [unverified] and `rank_tol = 2^-50`). `KktReport { multipliers, multiplier_bounds, dual_thresholds, primal, dual, stationarity, complementarity, tolerances, scales, status, reason }` with private fields and accessors and `pass() == (status == Certified)`; each condition is `Condition { lo, hi, threshold, verdict }` with `[lo, hi]` a proven enclosure of the exact residual; `Verdict::{Satisfied (hi <= thr), Violated (lo > thr), Undecidable}`; `KktStatus::{Certified, Violated, Undecidable}`: certified only when all four are satisfied, violated when any is (reason = first of primal, dual, stationarity, complementarity), undecidable otherwise; `KktFail::{Primal, Dual, Stationarity, Complementarity, DegenerateGradients, MultipliersNotProven}`; `KktScales { data, gradient_lo, gradient }`.
- **Algorithm:** primal = `max_i g_i(p*)` over every row, threshold `tol_p * data_scale`; complementarity = `max |g_i(p*)|` over the active rows, threshold `tol_c * data_scale`; multipliers by one `qr_pivoted` of `G^T` at `params` and the `lstsq` arithmetic (rank below n: `DegenerateGradients`, dual and stationarity undecidable), then a Krawczyk test on `[G(X)]^T lambda = -[grad f(X)]` proves the exact multipliers lie in `lambda +- multiplier_bounds` (no contraction: `MultipliersNotProven`, undecidable). **Dual feasibility per active row** against `thr_i = tol_d * lo(||grad f(X)||_2) / hi(||grad g_i(X)||_2)`, rounded down (if the lower bound of `||grad f||` is 0, `thr_i = 0`: the row passes only if `lambda*_i >= 0` is proven); row satisfied when `lo(Lambda_i) >= -thr_i`, violated when `hi(Lambda_i) < -thr_i`. Stationarity = `||G(p*)^T lambda + grad f(p*)||_2` at the float multipliers, threshold `tol_s * ||grad f(params)||_2`: it measures the accuracy of the float multipliers (exact stationarity at `lambda*` is proven by the Krawczyk step), so a false negative is possible. Fixed-order loops and total-order min/max.
- **Errors:** input errors (`InvalidInput`, `Shape`, `DuplicateLabel`), `BoundNotProven { NoEnclosure | NotSquare | Overflow }`, callback errors as returned; failing conditions are report content, not errors.
- **Determinism:** D0.
- **Performance budget:** < 50 microseconds for 1000 rows and 7 parameters [unverified until measured].
- **Crate:** `fid-solv` (module `cert::kkt`).

## Test fixtures

`fixtures/solv/cert/kkt-v1`: committed generator with exact references and planted non-optimal sets; CC-BY-4.0.

## Build prompt

> In crate `fid-solv`, module `cert::kkt`: `check(problem, rec: &Recomputed, tol: &KktTolerances) -> Result<KktReport, CertError>`. Prove each KKT condition at the exact optimum in the recomputation's proven box with outward-rounded `Interval` arithmetic from the problem's enclosure methods (`constraint_enclosure`, `objective_gradient_enclosure`, `constraint_gradient_enclosure`; missing gives `BoundNotProven`) and its `data_scale()` (default fails closed). Multipliers by `qr_pivoted`/`lstsq` arithmetic of `G^T lambda = -grad f` at the recomputed point (never solver multipliers), proven by a Krawczyk test; rank deficiency gives reason `DegenerateGradients`. Record each residual as a proven enclosure with its threshold and a three-valued verdict; `Certified` only when all four are certainly satisfied, never rounding a borderline into a pass. Dual thresholds per row `tol_d * lo||grad f|| / hi||grad g_i||`. Tests: `fixtures/solv/cert/kkt-v1` (exact-Fraction Python oracle), AC-2 and AC-3 cases, a materially negative multiplier that must not certify, fail-closed cases, cross-target digest. Constraints (CONSTITUTION §4 KNP-1, §8): only `fid-num` float primitives, fixed-order reductions, total-order min/max, no FMA, no HashMap/HashSet, no `unsafe`, no I/O; deny `unwrap`/`expect`/`panic`/`todo`/`unimplemented`/`unreachable`. Acceptance: AC-1..AC-4 of spec node KRN.SOLV.CERT.KKT.

## Open questions

- For second-order cone constraints the stationarity condition uses the cone's gradient at the active point; at the cone apex it is undefined. Callers with SOC rows must avoid apex contacts or report them as `degenerate_gradients` [unverified whether this occurs in MIC/MCC problems].

## Children

_Leaf: none._
