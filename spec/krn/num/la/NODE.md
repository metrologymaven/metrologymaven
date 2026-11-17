---
id: KRN.NUM.LA
title: Deterministic dense linear algebra
level: 3
parent: KRN.NUM
kind: module
status: split
phase: P0
size: L
owners: []
claimed_by:
satisfies: [REQ-KRN.NUM-001, REQ-KRN.NUM-005, REQ-KRN.NUM-007]
covers: []
depends_on: [KRN.NUM.ERR, KRN.NUM.FN, KRN.NUM.RED]
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# KRN.NUM.LA: Deterministic dense linear algebra

## Purpose

Small and medium dense matrix factorizations for the kernel: Householder QR with a fixed pivoting path, SVD and symmetric eigendecomposition, each reporting numerical rank and a condition estimate. Fits and solvers use these instead of any third-party LA crate, because crates with runtime dispatch or unspecified accumulation order change results by CPU.

## Scope

**In:**
- Dense row-major matrix and vector types for f64 (no generic scalar)
- Householder QR, with and without column pivoting, with a fixed pivot tie-break (lowest column index)
- Least-squares solve through QR, and triangular solves
- SVD by a fixed-order method (for example one-sided Jacobi with cyclic sweep order)
- Symmetric eigendecomposition by a fixed-order method (for example cyclic Jacobi), with eigenvalues sorted by total order and eigenvector sign normalised
- Numerical rank with an explicit relative tolerance parameter, and a condition-number estimate with every factorization

**Out** (and which node owns it instead):
- Sparse and iterative methods, conic solvers (`KRN.SOLV`)
- Reductions used inside the factorizations (`KRN.NUM.RED`)
- Autodiff and Jacobian assembly for fits (`KRN.LSQ`, `UNC`)

## Requirements

- **REQ-KRN.NUM.LA-001** The module shall provide Householder QR with optional column pivoting whose pivot choice breaks ties by the lowest column index, so the factorization path is fixed for given input.
- **REQ-KRN.NUM.LA-002** The module shall provide an SVD and a symmetric eigendecomposition whose sweep order is fixed, whose singular values and eigenvalues are returned sorted by total order, and whose vectors have a documented sign normalisation.
- **REQ-KRN.NUM.LA-003** Every factorization shall return the numerical rank under an explicit relative tolerance and a 2-norm condition-number estimate.
- **REQ-KRN.NUM.LA-004** All inner products and accumulations shall go through `KRN.NUM.RED` or a documented fixed-order loop, with no FMA, and results shall be bit-identical on x86-64, ARM64 and wasm32.
- **REQ-KRN.NUM.LA-005** If a factorization receives a non-finite entry or fails to reach its sweep-limit convergence criterion, then it shall return a typed `NumError` and no factors.
- **REQ-KRN.NUM.LA-006** On matrices up to 200 x 200 [unverified], QR reconstruction error shall be below 10 n u ||A|| and SVD singular values shall match a high-precision oracle within 10 n u sigma_max [unverified], where u is the unit roundoff.

## Interfaces

- **Offers:** `fid_num::la::{Mat, qr, qr_pivoted, lstsq, svd, sym_eig, Factorization{rank, cond}}`.
- **Consumes:** `KRN.NUM.RED`, `KRN.NUM.FN`, `KRN.NUM.ERR`.

## Decomposition hints

QR (Householder QR, pivoting, triangular and least-squares solves; P0 first), SVD (one-sided Jacobi), EIG (symmetric Jacobi, sorting and sign rules), COND (rank and condition estimation shared by all three). The concept baseline's P0 cut line keeps QR only in P0; SVD and eigen may start once fits need them.

## Open questions

- Whether small fixed-size (3 x 3, 4 x 4) eigen solvers for line and plane fits should be specialised closed forms or the general Jacobi path [unverified accuracy trade-off]. **Resolved at split (2026-11-17):** `KRN.NUM.LA.EIG` uses the general Jacobi path for every size; revisit only on profiling evidence.

## Children

The split follows the decomposition hints: one shared base (types, triangular solves, rank rule and condition estimator, so all factorizations report diagnostics the same way) and one leaf per factorization. COND was merged into MAT because QR is its only estimator user, while SVD and EIG compute condition exactly from their spectra.

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| KRN.NUM.LA.MAT | Dense matrix types, triangular solves, rank and condition estimation | 003 004 005 | none |
| KRN.NUM.LA.QR | Householder QR, column pivoting and least-squares solve | 001 003 004 005 006 | none |
| KRN.NUM.LA.SVD | One-sided Jacobi singular value decomposition | 002 003 004 005 006 | none |
| KRN.NUM.LA.EIG | Cyclic Jacobi symmetric eigendecomposition | 002 003 004 005 | none |
