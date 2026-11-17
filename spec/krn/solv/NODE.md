---
id: KRN.SOLV
title: Deterministic optimisation solvers and certificates
level: 2
parent: KRN
kind: subsystem
status: open
phase: P0
size: XL
owners: []
claimed_by:
satisfies: [REQ-KRN-001, REQ-KRN-005]
covers: []
depends_on: [KRN.NUM]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# KRN.SOLV: Deterministic optimisation solvers and certificates

## Purpose

The optimisation engines that fits, constructions and alignments share: nonlinear least squares (Gauss-Newton and Levenberg-Marquardt), and LP, QP and SOCP for Chebyshev and envelope problems. It also owns the solver audit: any vendored third-party solver must meet KNP-1, and every solver result is reduced to a checkable certificate so that iterates never become values of record. It is its own subsystem because ALN and GDT also use it, and because auditing a vendored solver is a distinct, reviewable job (this answers the parent's open question).

## Scope

**In:**
- Nonlinear least-squares driver (Gauss-Newton and Levenberg-Marquardt) using orthogonal factorizations, never normal equations
- LP, QP and SOCP solver, vendored and audited for KNP-1 (the baseline names a fork of Clarabel; adoption of the dependency itself goes through the allowlist)
- Sequential linearisation driver for non-convex problems (linearise, solve, repeat until the active set is stable)
- Deterministic multi-start with seeded, journaled start points and an agreement flag
- Active-set extraction, final-value recomputation from the active set, and optimality (KKT) checks
- Deterministic tie-break for degenerate or tied active sets, with a flag on the result
- Convergence, iteration-limit and conditioning diagnostics as typed errors
- Solver audit record: KNP-1 lint result, patched call sites, pinned version and tolerances

**Out** (and which node owns it instead):
- Problem formulations per primitive (`KRN.LSQ`, `KRN.MZ`, `KRN.ENV`)
- Dense linear algebra and transcendentals (`KRN.NUM`)
- Alignment objectives and DoF locks (`ALN`)
- GD&T zone optimisation formulations (`GDT`)
- Out-of-kernel cross-check solvers in CI (`KRN.MAN` runs them as test harness tools)

## Requirements

- **REQ-KRN.SOLV-001** The solvers shall be single-threaded, use only `fid-num` for floating-point primitives, and pass the KNP-1 lint.
- **REQ-KRN.SOLV-002** The nonlinear least-squares driver shall solve each step by an orthogonal factorization and shall never form normal equations.
- **REQ-KRN.SOLV-003** The LP, QP and SOCP solver shall run with fixed iteration limits and tolerances that are recorded in the kernel manifest.
- **REQ-KRN.SOLV-004** When a solver converges, the subsystem shall return a certificate containing the active (contact) set, the multipliers, and the result of an independent optimality check.
- **REQ-KRN.SOLV-005** The reported optimum shall be recomputed from the active set by a deterministic direct solve, so that two iterate paths reaching the same active set give bit-identical reported values.
- **REQ-KRN.SOLV-006** Solver iterates shall never enter any output digest; only the recomputed active-set result shall.
- **REQ-KRN.SOLV-007** When an active set is degenerate or tied, the subsystem shall apply a documented deterministic tie-break and set a `tie_broken` flag on the certificate.
- **REQ-KRN.SOLV-008** When a problem is non-convex, the subsystem shall run a deterministic multi-start from a seeded set of starts and shall set a `local_optimum_only` flag unless all starts agree within 1e-9 m [unverified].
- **REQ-KRN.SOLV-009** If a solver reaches its iteration limit, detects infeasibility, or meets a condition number above its configured threshold, then it shall return a typed error naming the cause and the last diagnostics, and no value.
- **REQ-KRN.SOLV-010** Every vendored solver shall have an audit record listing its pinned source digest, each patch made for KNP-1, and the lint result, and the audit shall be repeated on every upstream update.

## Interfaces

- **Offers:** `Solver` APIs for NLLS, LP, QP and SOCP; `Certificate` and `SolverError` types; multi-start and sequential-linearisation drivers. Used by `KRN.LSQ`, `KRN.MZ`, `KRN.ENV`, `KRN.CONS`, `ALN` and `GDT`.
- **Consumes:** `KRN.NUM` (LA, reductions, RNG, numeric errors).

## Decomposition hints

NLLS (Gauss-Newton and Levenberg-Marquardt), CONIC (vendored LP/QP/SOCP and its KNP-1 audit), SEQ (sequential linearisation and multi-start), CERT (active set, recomputation, KKT check, tie-break).

## Open questions

- Vendoring Clarabel depends on its licence passing D-001 [unverified].
- The multi-start agreement tolerance may need to be per quantity kind; reuse the REP-1 table from the concept doc once it is adopted in the constitution.

## Children

_None yet: status `open`._
