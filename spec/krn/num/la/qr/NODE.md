---
id: KRN.NUM.LA.QR
title: Householder QR, column pivoting and least-squares solve
level: 4
parent: KRN.NUM.LA
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
satisfies: [REQ-KRN.NUM.LA-001, REQ-KRN.NUM.LA-003, REQ-KRN.NUM.LA-004, REQ-KRN.NUM.LA-005, REQ-KRN.NUM.LA-006]
covers: []
depends_on: [KRN.NUM.LA.MAT, KRN.NUM.RED, KRN.NUM.FN, KRN.NUM.ERR]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# KRN.NUM.LA.QR: Householder QR, column pivoting and least-squares solve

## Purpose

The factorization that least-squares fits and the nonlinear least-squares driver solve every step with. Householder QR with an optional column-pivoting path whose ties break by the lowest column index, so the same input always takes the same path on every target, plus the least-squares solve built on it. It is the first LA piece needed in P0.

## Scope

**In:**
- Householder QR of an m x n matrix (m >= n) without pivoting
- QR with column pivoting by largest remaining column norm, ties to the lowest column index, with norms recomputed (not downdated) after each step
- Applying Q and Q^T to vectors without forming Q; forming the thin Q on request
- `lstsq(A, b, rel_tol)`: least-squares solution, residual norm and the `Factorization` diagnostics; minimum-norm handling is out (rank-deficient input is an error)

**Out** (and which node owns it instead):
- Matrix types, triangular solves, rank rule and condition estimator (`KRN.NUM.LA.MAT`)
- SVD (`KRN.NUM.LA.SVD`) and symmetric eigen (`KRN.NUM.LA.EIG`)
- Trust-region and Levenberg-Marquardt logic (`KRN.SOLV.NLLS`)

## Requirements

- **REQ-KRN.NUM.LA.QR-001** `qr` and `qr_pivoted` shall compute A P = Q R by Householder reflections in column order, with the reflector sign chosen so that r_kk has the opposite sign of a_kk (positive when a_kk is zero), documented and fixed.
- **REQ-KRN.NUM.LA.QR-002** `qr_pivoted` shall choose at step k the remaining column with the largest 2-norm under total order, breaking ties by the lowest column index, and shall recompute the remaining column norms from scratch at each step.
- **REQ-KRN.NUM.LA.QR-003** Every factorization shall return `Factorization { rank, cond, rel_tol }` with rank from the `KRN.NUM.LA.MAT` rule on |r_kk| and cond from the triangular estimator.
- **REQ-KRN.NUM.LA.QR-004** When `lstsq` is called and the rank under `rel_tol` is less than n, then it shall return a typed `RankDeficient { rank }` error and no solution.
- **REQ-KRN.NUM.LA.QR-005** If the input has fewer rows than columns or contains a non-finite entry, then the factorization shall return a typed `NumError` and no factors.
- **REQ-KRN.NUM.LA.QR-006** On the fixture matrices up to 200 x 200 [unverified], ||A P - Q R||_F shall be at most 10 n u ||A||_F and ||Q^T Q - I||_F at most 10 n u.
- **REQ-KRN.NUM.LA.QR-007** QR, pivoting and `lstsq` outputs shall be bit-identical on x86-64, ARM64 and wasm32, with all inner products through `KRN.NUM.RED`.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/num/la/qr-v1` (300 seeded matrices up to 200 x 200, including graded, rank-deficient and tied-column-norm cases), when `qr` and `qr_pivoted` run, then the reconstruction and orthogonality bounds of REQ-006 hold for every full-rank matrix.
- **AC-2** (tie-break) Given a 4 x 3 matrix whose three columns have equal norms, when `qr_pivoted` runs, then the permutation starts with column 0, and repeated runs give the same permutation.
- **AC-3** Given `fixtures/num/la/lstsq-v1` (NIST StRD linear regression problems Longley and Filip if their licence permits [unverified], else seeded synthetic problems with 50-digit reference solutions), when `lstsq` runs, then the solution matches the certified values to at least 7 significant digits [unverified for Filip].
- **AC-4** (error path) Given a 3 x 3 matrix with two equal columns and rel_tol 1e-12, when `lstsq` runs, then it returns `RankDeficient { rank: 2 }`; given a 2 x 3 matrix, `qr` returns `BadShape`.
- **AC-5** Given AC-1's fixture on x86-64, ARM64 and wasm32 under wasmtime, then the digest of all R, permutation and `lstsq` outputs is identical.

## Design

- **Registry command(s):** none; library API `fid_num::la::{qr, qr_pivoted, Qr, lstsq, LstsqResult}`.
- **Data structures:** `Qr { packed: Mat, tau: Vec<f64>, perm: Vec<usize>, diag: Factorization }` (compact reflectors below the diagonal, LAPACK-style storage); `LstsqResult { x: Vec<f64>, residual_norm: f64, diag: Factorization }`.
- **Algorithm:** for k in 0..n: (pivoted) compute norms of columns k..n by `RED::norm2` and swap in the total-order maximum, lowest index on ties; build the reflector with `FN::hypot`-safe norm, v and tau; apply to the trailing columns in ascending column order with `RED::dot`. `lstsq` applies Q^T to b, checks rank, solves R x = (Q^T b)[..n] with `solve_upper`, and un-permutes. Reference: Golub and Van Loan, Matrix Computations, 4th ed., section 5.2 and 5.4.
- **Errors:** `LaError::{BadShape, RankDeficient{rank}, Singular{index}}` in `NumError`, plus non-finite errors.
- **Determinism:** D0.
- **Performance budget:** 1000 x 6 QR (a typical fit Jacobian) < 0.5 ms; 200 x 200 < 30 ms [unverified until measured].
- **Crate:** `fid-num` (module `la::qr`).

## Test fixtures

`fixtures/num/la/qr-v1` and `fixtures/num/la/lstsq-v1`: committed offline generator with seeds and `mpmath` references (CC-BY-4.0); NIST StRD problems only if their terms allow redistribution [unverified], otherwise referenced by download script.

## Build prompt

> In crate `fid-num`, module `la::qr`, implement Householder `qr(A, rel_tol)` and `qr_pivoted(A, rel_tol)` for m x n, m >= n, storing reflectors compactly (`Qr { packed, tau, perm, diag }`), plus `apply_qt`, `apply_q`, `thin_q` and `lstsq(A, b, rel_tol)`. Reflector sign: r_kk opposite to a_kk's sign (positive when a_kk == 0). Pivoting: at each step recompute all remaining column norms with `fid_num::red::norm2`, pick the total-order maximum, ties to the lowest column index. Every inner product via `fid_num::red::dot` in ascending order; no FMA. Diagnostics: rank by `la::rank_by_rule` on |r_kk|, cond by `la::cond_triangular`. `lstsq` returns `RankDeficient { rank }` when rank < n. Reject m < n and non-finite input. Tests: `fixtures/num/la/qr-v1` (reconstruction <= 10 n u ||A||_F, orthogonality <= 10 n u), the tie-break case, `fixtures/num/la/lstsq-v1`, error cases, and an identical output digest on x86-64, ARM64 and wasm32 under wasmtime. Constraints (CONSTITUTION §4 KNP-1, §8): no HashMap/HashSet, no SIMD dispatch, no `unsafe`, no I/O; deny `unwrap`/`expect`/`panic`/`todo`/`unimplemented`/`unreachable`. Acceptance: AC-1..AC-5 of spec node KRN.NUM.LA.QR.

## Open questions

- Whether the NIST StRD data may be redistributed in the fixture directory [unverified]; `UNC.RDL` may already hold it as a reference dataset, in which case the lint's dataset ban applies only to kernel source, not to tests outside it.

## Children

_Leaf: none._
