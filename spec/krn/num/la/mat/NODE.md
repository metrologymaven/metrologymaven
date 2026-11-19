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
built_in: [fid-num]
satisfies: [REQ-KRN.NUM.LA-003, REQ-KRN.NUM.LA-004, REQ-KRN.NUM.LA-005]
covers: []
depends_on: [KRN.NUM.ERR, KRN.NUM.FN, KRN.NUM.RED]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-18
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

- **Registry command(s):** none; library API `fid_num::la::{Mat, mat_vec, mat_t_vec, mat_mul, solve_upper, solve_lower, rank_by_rule, cond_triangular, Factorization, LaError, MAX_DIM, MAX_MAT_BYTES, COND_ITERATIONS}`.
- **Data structures:** `Mat { rows: usize, cols: usize, data: Vec<f64> }` with private fields, so every value has `1..=4096` rows and columns, `rows * cols` entries and only finite entries; accessors `rows`, `cols`, `data`, `into_data`, `get`, `row` (slice), `col` (gathered copy), `transpose` (exact copy), `heap_bytes`. `Factorization { rank: usize, cond: f64, rel_tol: f64 }` (plain public fields). `LaError::{BadShape, TooLarge, BadTolerance, BadSpectrum, Singular { index }}` decoded from a `NumError` by `LaError::of`.
- **Algorithm:** products by row-wise `dot` over contiguous slices (for `mat_t_vec`, gather the column into a scratch vector first so `dot` sees ascending order); substitution in fixed order with `dot` for the partial sums; condition estimate = (power-method estimate of ||R||_2) x (power-method estimate of ||R^-1||_2), each with 8 iterations from the all-ones start vector, norms via `RED::norm2`. Reference for estimator quality: Higham, Accuracy and Stability of Numerical Algorithms, 2nd ed., chapter 15. Pinned details (values of record depend on every one; any implementation must match bit for bit; the Python model in `fixtures/num/la/make_la_fixtures.py` reproduces exactly these):
  - **Arithmetic:** every inner product is one call of `KRN.NUM.RED::dot` over two slices in ascending index order (products `a[i] * b[i]`, no FMA); every other operation is one IEEE-754 `-`, `*` or `/`. An empty `dot` is `+0.0`.
  - **Products:** `mat_vec`: `y[i] = dot(A[i, 0..k], x)`. `mat_t_vec`: `y[j] = dot(A[0..m, j], x)` with the column gathered first. `mat_mul`: `C[i][j] = dot(A[i, 0..k], B[0..k, j])`; B is transposed once into scratch, and elements are computed in row-major order.
  - **Triangular solves:** `solve_upper`: for `i = n-1` down to `0`, `x[i] = (b[i] - dot(R[i, i+1..n], x[i+1..n])) / R[i][i]`. `solve_lower`: for `i = 0` up to `n-1`, `x[i] = (b[i] - dot(L[i, 0..i], x[0..i])) / L[i][i]`. Only the named triangle (diagonal included) is read; the other strict triangle is ignored, so a factor stored in a full matrix (as QR leaves it) needs no clean-up.
  - **Singularity test (REQ-003):** `m` = total-order maximum of `|r_jj|`; `thr = rel_tol * m` (one rounded product); the diagonal is scanned in ascending index order before any arithmetic and the first `i` with `|r_ii| <= thr` is `Singular { index: i }`. So a zero diagonal is always singular (also with `rel_tol = 0`), and an all-zero diagonal is `Singular { index: 0 }`.
  - **Rank rule (REQ-004):** `rank_by_rule(d, rel_tol)` = number of `d[i] > rel_tol * d[0]` (strict; one rounded product). `d` must be non-negative and non-increasing (`-0.0` counts as zero); a value equal to the threshold does not count.
  - **Condition estimator (REQ-005):** (1) checks as below; (2) scaling: `e` = binary exponent of the total-order maximum `m` of `|r_ij|` over the upper triangle (`2^e <= m < 2^(e+1)`), `S = R * 2^-e` entry by entry as `(r * d1) * d2` with the same two power-of-two factors as `RED::norm2`; `S^T` is formed explicitly. The 2-norm condition number is invariant under this scaling, and it keeps the iterations in range. (3) For an operator `P` with adjoint `Q` (first `P = S`, `Q = S^T`; then `P = S^-1`, `Q = S^-T`): `x = [1.0; n]`; 8 times: `u = x / norm2(x)`, `y = P u`, `v = y / norm2(y)`, `x = Q v`; then `u = x / norm2(x)` and the estimate is `norm2(P u)`. Each `/` is one division per element. `S u` is `dot(S[i, i..n], u[i..n])`, `S^T v` is `dot(S^T[j, 0..=j], v[0..=j])`, `S^-1` is the back substitution above on `S`, `S^-T` the forward substitution on `S^T`. (4) The result is `est(S) * est(S^-1)`, one product. Each norm estimate is a lower bound of the true norm in exact arithmetic.
  - **Overflow:** a `dot` that overflows (product or accumulation) or a quotient that is not finite gives `Infinite { index: Some(k) }` with `k` the output element: the row-major index for products, the unknown for solves, first in computation order (descending `i` for `solve_upper`). In the estimator, an overflow, a zero norm or a non-finite or zero result means the condition number is beyond the `f64` range and gives `Infinite { index: None }`; the estimator never returns infinity.
  - **Allocation:** one `Mat` holds at most `4096^2` entries, `MAX_MAT_BYTES = 4096^2 * size_of::<f64>()` = 128 MiB; results and scratch are allocated with exact capacity (no growth). Peak per call: `mat_mul` its result plus a transposed copy of B; `cond_triangular` two `n x n` scratch matrices; everything else `O(n)` vectors.
  - **Accuracy (asserted by the oracle script, not by Rust):** each product element within `2 k u (|A||x|)_i + k 2^-1074` of the exact value (`u = 2^-53`, `k` the inner dimension; the second term covers gradual underflow); each solve on the fixture's diagonally dominant systems within `2 n u ||x||_inf + n 2^-1074` of the exact solution in max norm. Ill-conditioned solves (Kahan, Hilbert) are pinned bit for bit only.
- **Errors:** check order and kinds (`op` is `la.mat`, `la.mat_vec`, `la.mat_t_vec`, `la.mat_mul`, `la.solve_upper`, `la.solve_lower`, `la.rank_by_rule`, `la.cond_triangular`, or `la.get` / `la.row` / `la.col`):
  - `Mat::new(rows, cols, data)`: a zero dimension (`BadShape`), then more than 4096 rows or columns (`TooLarge`), then `data.len() != rows * cols` (`BadShape`), then the first NaN (`NotANumber`) or infinity (`Infinite`) by row-major index. `Mat::from_rows`: no rows or an empty first row (`BadShape`), size (`TooLarge`), a ragged row (`BadShape`), then the entries.
  - Products: dimension mismatch (`BadShape`), then the first NaN or infinity of the vector, then overflow.
  - Solves: non-square or `b.len() != n` (`BadShape`), the first NaN or infinity of `b`, `rel_tol` (NaN: `NotANumber { index: None }`, infinity: `Infinite { index: None }`, negative: `BadTolerance`; `-0.0` acts as zero), the singularity test, then overflow. `cond_triangular`: non-square, `rel_tol`, singularity, overflow.
  - `rank_by_rule`: empty (`EmptyInput`), the first NaN or infinity, `rel_tol`, then a negative value or an increase (`BadSpectrum`).
  - Encoding: `BadShape`, `TooLarge`, `BadTolerance`, `BadSpectrum` are `NumError::Domain { op, detail }` with detail `bad_shape`, `too_large`, `bad_tolerance`, `bad_spectrum` (as `KRN.NUM.FMT.LEN` wraps `FmtError`). `Singular { index }` is `NumError::Singular { op, rank: index }`: `KRN.NUM.ERR` fixes the variant set and `Domain` carries only a static string, so the existing `Singular` variant carries the index in its `rank` field (every diagonal entry before `index` passed the test, so it is the size of the leading block that passed). `LaError::of` decodes both forms.
- **Determinism:** D0; fixed loop order, all accumulation through `RED`; the fixtures pin result bits and digests, checked on x86-64 (Linux, Windows), ARM64 (macOS) and wasm32-wasip1 under wasmtime.
- **Performance budget:** 200x200 `mat_mul` < 20 ms on the minimum reference machine [unverified until measured].
- **Crate:** `fid-num` (module `la`, files `mat`, `tri`, `cond`). The power-of-two scale factors of `RED::norm2` are shared through a crate-private helper, unchanged in behaviour.

## Test fixtures

All generated by `fixtures/num/la/make_la_fixtures.py` (stdlib Python; CC-BY-4.0; CI runs it with `--check`), with two independent parts: an exact rational oracle (exact dot products as integer sums of dyadic rationals, checked against `fractions.Fraction`; exact `Fraction` substitution; condition numbers bracketed rigorously by exact inertia counts of `R^T R - mu I` with fraction-free elimination) and a bit-exact model of the pinned rules above.

- `fixtures/num/la/mat-ops-v1.txt` (AC-1): 500 seeded cases from 1x1 to 200x200; inputs are rebuilt from a splitmix64 seed (stream documented in the header), results pinned by per-operation hash prefixes and a run digest.
- `fixtures/num/la/mat-edge-v1.txt`: signed zeros, subnormals, cancellation, overflow positions, error order, ignored triangles, Kahan and Hilbert systems, rank-rule edges and estimator error paths, with result bits or the exact error.
- `fixtures/num/la/cond-v1.txt` (AC-2): 83 upper triangular matrices (graded, built from a prescribed SVD, Kahan, scaled by 2^900 and 2^-1000, identity, Hilbert) with condition 1e0 to 1e14 (Kahan cases to 1e19), the rigorous exact interval and the estimate's bits.

The leaf first named `mpmath` at 50 digits; exact rational arithmetic in the standard library replaces it (no third-party dependency, and exact rather than 50-digit).

## Build prompt

> In crate `fid-num`, create module `la` with `Mat { rows, cols, data: Vec<f64> }` (row-major; constructors reject non-finite entries, zero dimensions, and more than 4096 rows or columns), `transpose`, `mat_vec`, `mat_t_vec`, `mat_mul`, `solve_upper`, `solve_lower`, `rank_by_rule(diag_desc: &[f64], rel_tol) -> usize`, `cond_triangular(r: &Mat, rel_tol) -> Result<f64, NumError>` and `Factorization { rank, cond, rel_tol }`. Every inner product goes through `fid_num::red::dot` in ascending index order (gather columns into scratch first); no FMA; no `mul_add`. Triangular solves return `Singular { index }` when |r_ii| <= rel_tol * max|r_jj|. The condition estimator multiplies 8-iteration power-method estimates of ||R||_2 and ||R^-1||_2 from the all-ones start vector. Tests: `fixtures/num/la/mat-ops-v1`, `fixtures/num/la/cond-v1`, the error cases of AC-3 and AC-4, the rank example of AC-5, and an identical output digest on x86-64, ARM64 and wasm32 under wasmtime. Constraints (CONSTITUTION §4 KNP-1, §8): transcendentals only through `fid_num::fn`, total-order min/max only, no HashMap/HashSet, no SIMD dispatch, no `unsafe`, no I/O; deny `unwrap`/`expect`/`panic`/`todo`/`unimplemented`/`unreachable`. Acceptance: AC-1..AC-5 of spec node KRN.NUM.LA.MAT.

## Open questions

- The 4096 size bound and 8 power iterations are starting values; set them from the largest kernel problem (`KRN.LSQ` Jacobians are tall but narrow) [unverified].

## Children

_Leaf: none._
