---
id: KRN.NUM.LA.SVD
title: One-sided Jacobi singular value decomposition
level: 4
parent: KRN.NUM.LA
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
built_in: [fid-num]
built_pr: 409
satisfies: [REQ-KRN.NUM.LA-002, REQ-KRN.NUM.LA-003, REQ-KRN.NUM.LA-004, REQ-KRN.NUM.LA-005, REQ-KRN.NUM.LA-006]
covers: []
depends_on: [KRN.NUM.LA.MAT, KRN.NUM.RED, KRN.NUM.FN, KRN.NUM.ERR]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# KRN.NUM.LA.SVD: One-sided Jacobi singular value decomposition

## Purpose

A singular value decomposition whose sweep order is fixed, so the same matrix yields the same singular values and vectors on every target. Fits use it for plane and line normals, rank tests and pseudo-inverses; uncertainty code uses it for conditioning. One-sided (Hestenes) Jacobi is chosen because its operations are rotations of column pairs in a fixed cyclic order, which is easy to make bit-reproducible and is accurate for small singular values.

## Scope

**In:**
- `svd(A, rel_tol)` for m x n (m >= n directly; m < n through the transpose) returning U (thin), sigma and V
- Cyclic sweep order over column pairs (p, q), p < q, row-major
- A fixed convergence test and a fixed sweep limit
- Sorting singular values descending by total order (with the matching permutation of U and V columns) and the sign normalisation of V columns
- `Factorization` diagnostics: rank from sigma, cond = sigma_max / sigma_min (exact from the computed values)

**Out** (and which node owns it instead):
- Matrix types and the rank rule (`KRN.NUM.LA.MAT`)
- QR (`KRN.NUM.LA.QR`), symmetric eigen (`KRN.NUM.LA.EIG`)
- Fitting logic that interprets singular vectors (`KRN.LSQ`)

## Requirements

- **REQ-KRN.NUM.LA.SVD-001** The SVD shall apply one-sided Jacobi rotations to column pairs (p, q) with p < q in the fixed order (0,1), (0,2), ..., (n-2, n-1) per sweep, with rotation parameters computed through `fid-num` transcendentals only.
- **REQ-KRN.NUM.LA.SVD-002** A sweep shall be the last when every pair satisfies |a_p . a_q| <= n u ||a_p|| ||a_q||; if 60 sweeps [unverified] pass without that, the SVD shall return a typed `NoConvergence { sweeps }` error and no factors.
- **REQ-KRN.NUM.LA.SVD-003** Singular values shall be returned in descending total order, ties kept in original column order, and each V column shall be signed so that its largest-magnitude entry (lowest index on ties) is positive, with the matching U column sign.
- **REQ-KRN.NUM.LA.SVD-004** The SVD shall return `Factorization { rank, cond, rel_tol }` with rank by the `KRN.NUM.LA.MAT` rule and cond = sigma_1 / sigma_n, or a typed `Singular` diagnostic flag when sigma_n is zero.
- **REQ-KRN.NUM.LA.SVD-005** On the fixture matrices up to 200 x 200 [unverified], each singular value shall match a 50-digit oracle within 10 n u sigma_max, and ||A - U diag(sigma) V^T||_F shall be at most 10 n u ||A||_F.
- **REQ-KRN.NUM.LA.SVD-006** If the input contains a non-finite entry, then the SVD shall return a typed `NumError` and no factors, and its outputs shall otherwise be bit-identical on x86-64, ARM64 and wasm32.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/num/la/svd-v1` (300 seeded matrices up to 200 x 200, including graded singular values 1 to 1e-15, repeated singular values and rank-deficient cases, with 50-digit oracle values), when `svd` runs, then REQ-005 bounds hold.
- **AC-2** Given a 3 x 3 diagonal matrix diag(1, 3, 2), then sigma = [3, 2, 1] and each V column is a signed unit vector with a positive largest entry.
- **AC-3** (sign) Given A and -A, then both give identical sigma and V, and U columns differ only in sign.
- **AC-4** (error path) Given a matrix with an infinity, then `svd` returns a typed error; given a test build with the sweep limit set to 1 on a 50 x 50 random matrix, it returns `NoConvergence`.
- **AC-5** Given AC-1's fixture on x86-64, ARM64 and wasm32 under wasmtime, then the digest of all outputs is identical.

## Design

- **Registry command(s):** none; library API `fid_num::la::{svd, Svd}`.
- **Data structures:** `Svd { u: Mat, sigma: Vec<f64>, v: Mat, diag: Factorization, sweeps: u16 }`.
- **Algorithm:** work on a column-major copy of A; for each pair compute alpha = ||a_p||^2, beta = ||a_q||^2, gamma = a_p . a_q with `RED::dot`; skip when the convergence test holds; otherwise zeta = (beta - alpha) / (2 gamma), t = sign(zeta) / (|zeta| + sqrt(1 + zeta^2)) via `FN::sqrt` (with the overflow-safe form for large zeta), c = 1 / sqrt(1 + t^2), s = c t; rotate columns of A and V. After convergence sigma_k = ||a_k||, u_k = a_k / sigma_k (zero sigma keeps a zero U column). Sort, then normalise signs. Reference: Demmel and Veselic, Jacobi's method is more accurate than QR, SIAM J. Matrix Anal. Appl. 13 (1992).
- **Errors:** `LaError::{BadShape, NoConvergence{sweeps}}`, non-finite errors.
- **Determinism:** D0.
- **Performance budget:** 1000 x 3 SVD < 0.3 ms; 200 x 200 < 1 s [unverified until measured].
- **Crate:** `fid-num` (module `la::svd`).

## Test fixtures

`fixtures/num/la/svd-v1`: committed offline generator with seeds and `mpmath` 50-digit references, CC-BY-4.0.

## Build prompt

> In crate `fid-num`, module `la::svd`, implement `svd(A: &Mat, rel_tol: f64) -> Result<Svd, NumError>` by one-sided Jacobi: column pairs (p, q), p < q, in fixed cyclic order; dots via `fid_num::red::dot`; rotation from zeta = (beta - alpha) / (2 gamma) with the overflow-safe tangent formula using `fid_num::fn::sqrt`; stop after a sweep in which every pair satisfies |gamma| <= n u sqrt(alpha beta); max 60 sweeps then `NoConvergence`. m < n: factor the transpose and swap U and V. Output: thin U, sigma descending by total order (stable for ties), V; V columns signed so the largest-magnitude entry (lowest index on ties) is positive, U columns to match. Diagnostics: rank by `la::rank_by_rule`, cond = sigma_1 / sigma_n. Tests: `fixtures/num/la/svd-v1` (singular values within 10 n u sigma_max of 50-digit oracle; reconstruction within 10 n u ||A||_F), AC-2..AC-4 examples, identical output digest on x86-64, ARM64 and wasm32 under wasmtime. Constraints (CONSTITUTION §4 KNP-1, §8): no FMA, no HashMap/HashSet, no SIMD dispatch, no `unsafe`, no I/O; deny `unwrap`/`expect`/`panic`/`todo`/`unimplemented`/`unreachable`. Acceptance: AC-1..AC-5 of spec node KRN.NUM.LA.SVD.

## Open questions

- A QR preconditioning step (factor A = QR first, then Jacobi on R) speeds up tall matrices; it would change outputs, so add it only before the first release or as a new algorithm version.

## Children

_Leaf: none._
