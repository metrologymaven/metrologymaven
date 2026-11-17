---
id: KRN.NUM.LA.MAT
title: Dense matrix types, triangular solves, rank and condition estimation
level: 4
parent: KRN.NUM.LA
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
satisfies: [REQ-KRN.NUM.LA-003, REQ-KRN.NUM.LA-004, REQ-KRN.NUM.LA-005]
covers: []
depends_on: [KRN.NUM.ERR, KRN.NUM.FN, KRN.NUM.RED]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# KRN.NUM.LA.MAT: Dense matrix types, triangular solves, rank and condition estimation

## Purpose

The base that every factorization in `fid_num::la` builds on: row-major f64 matrix and vector types with checked construction, matrix-vector and matrix-matrix products routed through fixed-order reductions, triangular solves, the shared `Factorization` diagnostics record, the numerical-rank rule and a deterministic 2-norm condition estimator for triangular factors. Putting these in one place means QR, SVD and eigen report rank and condition the same way.

## Scope

**In:**
- `Mat { rows, cols, data: Vec<f64> }` row-major and `Vec`-backed vectors; finiteness-checked constructors; transpose; column and row views
- Products `mat_vec`, `mat_t_vec`, `mat_mul` with every inner product through `KRN.NUM.RED::dot`
- Forward and back substitution for upper and lower triangular systems
- `Factorization { rank, cond, rel_tol }` diagnostics and the rank rule
- A 2-norm condition estimator for triangular matrices
- Size limits and typed errors

**Out** (and which node owns it instead):
- QR, least squares (`KRN.NUM.LA.QR`), SVD (`KRN.NUM.LA.SVD`), symmetric eigen (`KRN.NUM.LA.EIG`)
- Summation and dot kernels themselves (`KRN.NUM.RED`)
- Sparse matrices and conic solvers (`KRN.SOLV`)

## Requirements

- **REQ-KRN.NUM.LA.MAT-001** `Mat::from_rows` shall reject any non-finite entry, a zero dimension, or more than 4096 rows or columns [unverified bound] with a typed `NumError`.
- **REQ-KRN.NUM.LA.MAT-002** Every inner product in `mat_vec`, `mat_t_vec`, `mat_mul` and the triangular solves shall be computed by `KRN.NUM.RED::dot` over operands in ascending index order, with no FMA.
- **REQ-KRN.NUM.LA.MAT-003** If a triangular solve meets a diagonal entry whose magnitude is at most `rel_tol` times the largest diagonal magnitude, then it shall return a typed `Singular { index }` error and no solution.
- **REQ-KRN.NUM.LA.MAT-004** The rank rule shall count the diagonal values (R diagonal magnitudes or singular values, sorted descending) that exceed `rel_tol` times the largest, with `rel_tol` an explicit caller parameter and no default hidden in the factorization.
- **REQ-KRN.NUM.LA.MAT-005** The triangular condition estimator shall return an estimate of the 2-norm condition number from a fixed number of power iterations on R and on R^-1 from a fixed start vector, and shall return a typed `Singular` error, never infinity, when R is rank-deficient under `rel_tol`.
- **REQ-KRN.NUM.LA.MAT-006** On the fixture matrices, the triangular condition estimate shall lie within a factor of 10 of the exact 2-norm condition number [unverified].
- **REQ-KRN.NUM.LA.MAT-007** All operations shall give bit-identical results on x86-64, ARM64 and wasm32.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/num/la/mat-ops-v1` (500 seeded random matrices 1x1 to 200x200 with products and triangular solves computed offline at 50 digits and rounded), when the products and solves run, then each result is within 2 n u of the offline value in max norm, and the output digest is identical on x86-64, ARM64 and wasm32 under wasmtime.
- **AC-2** Given the triangular matrices in `fixtures/num/la/cond-v1` (condition numbers 1e0 to 1e14 known offline), when the estimator runs, then each estimate is within a factor of 10 of the exact value.
- **AC-3** (error path) Given a matrix with a NaN, an infinite entry, 0 rows, or 5000 columns, when constructed, then each returns a typed error.
- **AC-4** (error path) Given an upper triangular matrix with r_33 = 1e-20 and r_11 = 1, rel_tol = 1e-12, when back substitution runs, then it returns `Singular { index: 2 }`.
- **AC-5** Given singular values [3, 1, 1e-13, 0] and rel_tol 1e-12, when the rank rule runs, then the rank is 2.

## Design

- **Registry command(s):** none; library API `fid_num::la::{Mat, mat_vec, mat_t_vec, mat_mul, solve_upper, solve_lower, rank_by_rule, cond_triangular, Factorization}`.
- **Data structures:** `Mat { rows: usize, cols: usize, data: Vec<f64> }`; `Factorization { rank: usize, cond: f64, rel_tol: f64 }`.
- **Algorithm:** products by row-wise `dot` over contiguous slices (for `mat_t_vec`, gather the column into a scratch vector first so `dot` sees ascending order); substitution in fixed order with `dot` for the partial sums; condition estimate = (power-method estimate of ||R||_2) x (power-method estimate of ||R^-1||_2), each with 8 iterations [unverified count] from the all-ones start vector, norms via `RED::norm2`. Reference for estimator quality: Higham, Accuracy and Stability of Numerical Algorithms, 2nd ed., chapter 15.
- **Errors:** `NumError::{NotANumber, Infinite, Domain(LaError::{BadShape, TooLarge, Singular{index}})}`.
- **Determinism:** D0; fixed loop order, all accumulation through `RED`.
- **Performance budget:** 200x200 `mat_mul` < 20 ms on the minimum reference machine [unverified until measured].
- **Crate:** `fid-num` (module `la`, files `mat`, `tri`, `cond`).

## Test fixtures

`fixtures/num/la/mat-ops-v1` and `fixtures/num/la/cond-v1`: generated by a committed offline script (seeded matrices, `mpmath` at 50 digits for references), CC-BY-4.0.

## Build prompt

> In crate `fid-num`, create module `la` with `Mat { rows, cols, data: Vec<f64> }` (row-major; constructors reject non-finite entries, zero dimensions, and more than 4096 rows or columns), `transpose`, `mat_vec`, `mat_t_vec`, `mat_mul`, `solve_upper`, `solve_lower`, `rank_by_rule(diag_desc: &[f64], rel_tol) -> usize`, `cond_triangular(r: &Mat, rel_tol) -> Result<f64, NumError>` and `Factorization { rank, cond, rel_tol }`. Every inner product goes through `fid_num::red::dot` in ascending index order (gather columns into scratch first); no FMA; no `mul_add`. Triangular solves return `Singular { index }` when |r_ii| <= rel_tol * max|r_jj|. The condition estimator multiplies 8-iteration power-method estimates of ||R||_2 and ||R^-1||_2 from the all-ones start vector. Tests: `fixtures/num/la/mat-ops-v1`, `fixtures/num/la/cond-v1`, the error cases of AC-3 and AC-4, the rank example of AC-5, and an identical output digest on x86-64, ARM64 and wasm32 under wasmtime. Constraints (CONSTITUTION §4 KNP-1, §8): transcendentals only through `fid_num::fn`, total-order min/max only, no HashMap/HashSet, no SIMD dispatch, no `unsafe`, no I/O; deny `unwrap`/`expect`/`panic`/`todo`/`unimplemented`/`unreachable`. Acceptance: AC-1..AC-5 of spec node KRN.NUM.LA.MAT.

## Open questions

- The 4096 size bound and 8 power iterations are starting values; set them from the largest kernel problem (`KRN.LSQ` Jacobians are tall but narrow) [unverified].

## Children

_Leaf: none._
