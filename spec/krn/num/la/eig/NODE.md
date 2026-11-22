---
id: KRN.NUM.LA.EIG
title: Cyclic Jacobi symmetric eigendecomposition
level: 4
parent: KRN.NUM.LA
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
built_in: [fid-num]
satisfies: [REQ-KRN.NUM.LA-002, REQ-KRN.NUM.LA-003, REQ-KRN.NUM.LA-004, REQ-KRN.NUM.LA-005]
covers: []
depends_on: [KRN.NUM.LA.MAT, KRN.NUM.RED, KRN.NUM.FN, KRN.NUM.ERR]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-21
---

# KRN.NUM.LA.EIG: Cyclic Jacobi symmetric eigendecomposition

## Purpose

The symmetric eigendecomposition used for inertia and scatter matrices (initial plane, line and cylinder axis estimates), principal directions and covariance analysis. Cyclic Jacobi with a fixed pair order gives the same eigenvalues and vectors on every target, and the same method serves 3 x 3 and larger matrices, so there is one code path to validate.

## Scope

**In:**
- `sym_eig(A, rel_tol)` for symmetric n x n matrices: eigenvalues and orthonormal eigenvectors
- A symmetry check with a stated tolerance
- Cyclic sweep order over (p, q), p < q, row-major; fixed convergence test and sweep limit
- Eigenvalues sorted ascending by total order with matching vectors; eigenvector sign normalisation
- `Factorization` diagnostics: rank from |lambda| and cond = max|lambda| / min|lambda|

**Out** (and which node owns it instead):
- Matrix types and the rank rule (`KRN.NUM.LA.MAT`)
- Non-symmetric eigenproblems (not needed by the kernel; would be a new node)
- Interpreting eigenvectors as feature directions (`KRN.LSQ`)

## Requirements

- **REQ-KRN.NUM.LA.EIG-001** If |a_ij - a_ji| exceeds n u max|a| for any pair, then `sym_eig` shall return a typed `NotSymmetric { i, j }` error; otherwise it shall use the upper triangle only.
- **REQ-KRN.NUM.LA.EIG-002** The decomposition shall apply Jacobi rotations to pairs (p, q), p < q, in the fixed order (0,1), (0,2), ..., (n-2, n-1) per sweep, skipping a rotation only by a fixed threshold test, with rotation parameters from `fid-num` transcendentals.
- **REQ-KRN.NUM.LA.EIG-003** The decomposition shall stop after the first sweep in which the off-diagonal Frobenius norm is at most n u times the Frobenius norm of A, and shall return a typed `NoConvergence { sweeps }` error after 50 sweeps [unverified].
- **REQ-KRN.NUM.LA.EIG-004** Eigenvalues shall be returned ascending in total order (ties in original diagonal order), and each eigenvector shall be signed so that its largest-magnitude entry (lowest index on ties) is positive.
- **REQ-KRN.NUM.LA.EIG-005** The result shall carry `Factorization { rank, cond, rel_tol }`, rank by the `KRN.NUM.LA.MAT` rule on |lambda| sorted descending, cond = max|lambda| / min|lambda|.
- **REQ-KRN.NUM.LA.EIG-006** On the fixture matrices up to 200 x 200 [unverified], each eigenvalue shall match a 50-digit oracle within 10 n u ||A||_2, ||A V - V Lambda||_F shall be at most 10 n u ||A||_F, and ||V^T V - I||_F at most 10 n u.
- **REQ-KRN.NUM.LA.EIG-007** The same Jacobi path shall be used for every size including 3 x 3, and outputs shall be bit-identical on x86-64, ARM64 and wasm32.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/num/la/eig-v1` (300 seeded symmetric matrices 2 x 2 to 200 x 200, including repeated eigenvalues, clustered eigenvalues and 3 x 3 scatter matrices from near-planar point sets, with 50-digit oracle values), when `sym_eig` runs, then REQ-006 bounds hold.
- **AC-2** Given the 3 x 3 scatter matrix of the points of a perfect plane z = 0 sampled on a grid, then the smallest eigenvalue is at most 1e-15 times the largest and its eigenvector is (0, 0, 1).
- **AC-3** (error path) Given a matrix with a_01 = 1 and a_10 = 1.1, then `sym_eig` returns `NotSymmetric { i: 0, j: 1 }`; given a NaN entry, a typed non-finite error.
- **AC-4** (determinism) Given the identity matrix (all eigenvalues tied), then eigenvalues are [1, 1, 1] and vectors are the unit basis in order on every run.
- **AC-5** Given AC-1's fixture on x86-64, ARM64 and wasm32 under wasmtime, then the digest of all outputs is identical.

## Design

- **Registry command(s):** none; library API `fid_num::la::{sym_eig, SymEig}`.
- **Data structures:** `SymEig { values: Vec<f64>, vectors: Mat, diag: Factorization, sweeps: u16 }`, eigenvectors as columns.
- **Algorithm:** classical cyclic Jacobi with threshold: for each (p, q) with |a_pq| above the sweep threshold, theta = (a_qq - a_pp) / (2 a_pq), t = sign(theta) / (|theta| + sqrt(theta^2 + 1)) (overflow-safe form for large theta) via `FN::sqrt`, c = 1 / sqrt(t^2 + 1), s = t c; update rows and columns p and q in ascending index order and accumulate V. Off-diagonal norm by `RED::norm2` over the strict upper triangle in row-major order. Reference: Golub and Van Loan, Matrix Computations, 4th ed., section 8.5.
- **Errors:** `LaError::{BadShape, NotSymmetric{i, j}, NoConvergence{sweeps}}`, non-finite errors.
- **Determinism:** D0.
- **Performance budget:** 3 x 3 < 2 microseconds; 200 x 200 < 1 s [unverified until measured].
- **Crate:** `fid-num` (module `la::eig`).

## Test fixtures

`fixtures/num/la/eig-v1`: committed offline generator with seeds and `mpmath` 50-digit references, CC-BY-4.0.

## Build prompt

> In crate `fid-num`, module `la::eig`, implement `sym_eig(A: &Mat, rel_tol: f64) -> Result<SymEig, NumError>` by cyclic Jacobi: check symmetry (|a_ij - a_ji| <= n u max|a| else `NotSymmetric`), use the upper triangle, rotate pairs (p, q), p < q, in fixed row-major order with the overflow-safe tangent formula via `fid_num::fn::sqrt`, accumulate eigenvectors, stop after a sweep where off(A) <= n u ||A||_F, max 50 sweeps then `NoConvergence`. Sort eigenvalues ascending by total order (stable), permute vectors, sign each vector so its largest-magnitude entry (lowest index on ties) is positive. Diagnostics via `la::rank_by_rule` and cond = max|lambda| / min|lambda|. No closed-form 3 x 3 special case. Tests: `fixtures/num/la/eig-v1` (eigenvalues within 10 n u ||A||_2 of 50-digit oracle; residual and orthogonality bounds), the plane-scatter, asymmetric, NaN and identity cases, identical output digest on x86-64, ARM64 and wasm32 under wasmtime. Constraints (CONSTITUTION §4 KNP-1, §8): no FMA, all sums through `fid_num::red`, no HashMap/HashSet, no SIMD dispatch, no `unsafe`, no I/O; deny `unwrap`/`expect`/`panic`/`todo`/`unimplemented`/`unreachable`. Acceptance: AC-1..AC-5 of spec node KRN.NUM.LA.EIG.

## Open questions

- The parent asked whether 3 x 3 and 4 x 4 solvers should be closed forms. This leaf uses the general Jacobi path for every size: closed forms based on the cubic need acos and cbrt and lose accuracy for nearly repeated eigenvalues [unverified], and one path is cheaper to validate. Revisit only if profiling shows 3 x 3 eigen as a bottleneck.
- Eigenvectors of repeated eigenvalues are deterministic but not unique; callers must not treat them as meaningful directions.

## Children

_Leaf: none._
