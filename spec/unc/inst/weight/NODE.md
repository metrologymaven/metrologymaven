---
id: UNC.INST.WEIGHT
title: Covariance and weight export for networks and transforms
level: 3
parent: UNC.INST
kind: task
status: leaf
phase: P1b
size: S
owners: []
claimed_by:
satisfies: [REQ-UNC.INST-006]
covers: []
depends_on: [KRN]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# UNC.INST.WEIGHT: Covariance and weight export for networks and transforms

## Purpose

Converts per-observation covariance matrices into the weight forms that adjustment and alignment solvers consume: the inverse covariance (weight matrix) and a whitening factor (inverse Cholesky factor), with a stated rank and conditioning, in one kernel function. `LVM` adjustment and `UNC.XFM` both call it, so weighting is consistent everywhere.

## Scope

**In:**
- `weights(cov: Cov3 or CovN, policy) -> Weight { w: SymMatrix, whitening: LowerTri, rank, cond }`
- Rank-deficient handling: pseudo-inverse with declared threshold, or error, per policy
- Batch form over observation arrays

**Out:**
- Producing the covariance (owned by `UNC.INST.PORT`)
- Using the weights (owned by `LVM` and `UNC.XFM`)

## Requirements

- **REQ-UNC.INST.WEIGHT-001** Given a symmetric positive-definite covariance Σ, the function shall return W = Σ⁻¹ and a lower-triangular whitening factor L⁻¹ where Σ = L·Lᵀ, with ‖W·Σ − I‖_max ≤ 1e-12·cond(Σ).
- **REQ-UNC.INST.WEIGHT-002** If Σ is not symmetric or has a negative eigenvalue below −1e-12·‖Σ‖, then the function shall fail with `invalid_covariance`.
- **REQ-UNC.INST.WEIGHT-003** If Σ is singular or its condition estimate exceeds 1e12, then under policy `strict` the function shall fail with `ill_conditioned`, and under policy `pseudo` it shall return the pseudo-inverse with the numerical rank and flag `rank_deficient`.
- **REQ-UNC.INST.WEIGHT-004** The batch form shall return results in input order and produce outputs bit-identical to per-observation calls.

## Acceptance criteria

- **AC-1** Given `fixtures/unc/weight/spd_cases.json` (50 random SPD 3x3 matrices, seed `0x57454947`, condition numbers 1 to 1e8), when weights are computed, then each satisfies the REQ-001 residual bound and W matches the 50-digit reference within 1e-12 relative.
- **AC-2** (error path) Given `fixtures/unc/weight/indefinite.json`, then the call fails with `invalid_covariance`.
- **AC-3** (singular) Given a rank-2 3x3 covariance (a point constrained to a plane), under `strict` the call fails with `ill_conditioned`; under `pseudo` it returns rank 2 and `rank_deficient`.
- **AC-4** (batch) Given AC-1's 50 matrices as one batch, outputs equal the single calls bit for bit.
- **AC-5** (portability) Outputs are bit-identical on wasm32, x86-64 and ARM64.

## Design

- **Registry command(s):** none; kernel algorithm `unc.inst.weights@1.0.0`.
- **Data structures:** `WeightPolicy { Strict | Pseudo { rank_tol: f64 } }`; `Weight { w, whitening, rank, cond, flags }`.
- **Algorithm:** symmetric eigen decomposition from `fid-num` (which reports rank and condition); W = V·diag(1/λ)·Vᵀ over retained eigenvalues; whitening via Cholesky when full rank (from `fid-num`), else from the eigenbasis. Fixed-order reductions.
- **Errors:** `invalid_covariance`, `ill_conditioned`, `dimension_mismatch`.
- **Determinism:** D0.
- **Performance budget:** 10^6 3x3 weights in under 0.5 s single-threaded [unverified until measured].
- **Crate:** `fid-kunc`.

## Test fixtures

`fixtures/unc/weight/`: seeded random SPD matrices with 50-digit references from an mpmath script; an indefinite case; a rank-2 case (CC-BY-4.0).

## Build prompt

> In crate `fid-kunc` (kernel; KNP-1; linear algebra only through `fid-num`; no panics, no `unwrap`), add `inst::weights(cov, policy) -> Result<Weight, WeightError>` and a batch form, algorithm ID `unc.inst.weights@1.0.0`.
> Return W = Σ⁻¹ and a lower-triangular whitening factor L⁻¹ (Σ = L·Lᵀ), plus numerical rank and condition estimate. Fail `invalid_covariance` if Σ is not exactly symmetric or has an eigenvalue below −1e-12·‖Σ‖. If singular or cond > 1e12: policy `Strict` fails `ill_conditioned`; policy `Pseudo{rank_tol}` returns the eigen pseudo-inverse with rank and flag `rank_deficient`. Batch output order equals input order and is bit-identical to single calls.
> Tests: `fixtures/unc/weight/spd_cases.json` residual ‖WΣ − I‖ ≤ 1e-12·cond and 1e-12 relative agreement with references; indefinite case; rank-2 case under both policies; batch equality; bit-identical on wasm32, x86-64, ARM64.

## Open questions

- None.

## Children

_Leaf: none._
