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

- **Registry command(s):** none; library API `fid_solv::cert::kkt::{check, KktReport, KktTolerances}`.
- **Data structures:** `KktTolerances { tol_p, tol_d, tol_s, tol_c }` relative to scales (max |constraint data| and ||objective gradient||); `KktReport { multipliers: Vec<f64>, primal: f64, dual: f64, stationarity: f64, complementarity: f64, tolerances, scales, pass: bool, reason: Option<KktFail> }`.
- **Algorithm:** get gradients from the `ActiveSetProblem`; QR `lstsq` of G^T lambda = grad f (G rows = active gradients); residual norm via `RED::norm2`; feasibility via `constraint_values`; total-order max/min.
- **Errors:** callback errors propagate as `CertError::Callback`; failing conditions are report content, not errors.
- **Determinism:** D0.
- **Performance budget:** < 50 microseconds for 1000 rows and 7 parameters [unverified until measured].
- **Crate:** `fid-solv` (module `cert::kkt`).

## Test fixtures

`fixtures/solv/cert/kkt-v1`: committed generator with exact references and planted non-optimal sets; CC-BY-4.0.

## Build prompt

> In crate `fid-solv`, add module `cert::kkt` with `check(problem: &dyn ActiveSetProblem, set, params, tol: &KktTolerances) -> Result<KktReport, CertError>`. Compute multipliers by `fid_num::la::lstsq` of G^T lambda = grad f over the active constraint gradients (never solver multipliers); rank deficiency gives `pass = false`, reason `degenerate_gradients`. Primal residual = max constraint value over all rows; dual = max(0, -min lambda); stationarity = ||G^T lambda - grad f||; complementarity = max |g_i| over active rows; pass only when all are within their scaled tolerances. Record tolerances and scales. Tests: `fixtures/solv/cert/kkt-v1`, AC-2 and AC-3 cases, cross-target digest. Constraints (CONSTITUTION §4 KNP-1, §8): only `fid-num` float primitives, fixed-order reductions, total-order min/max, no FMA, no HashMap/HashSet, no `unsafe`, no I/O; deny `unwrap`/`expect`/`panic`/`todo`/`unimplemented`/`unreachable`. Acceptance: AC-1..AC-4 of spec node KRN.SOLV.CERT.KKT.

## Open questions

- For second-order cone constraints the stationarity condition uses the cone's gradient at the active point; at the cone apex it is undefined. Callers with SOC rows must avoid apex contacts or report them as `degenerate_gradients` [unverified whether this occurs in MIC/MCC problems].

## Children

_Leaf: none._
