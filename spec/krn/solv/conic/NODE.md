---
id: KRN.SOLV.CONIC
title: Vendored LP, QP and SOCP solver with KNP-1 audit
level: 3
parent: KRN.SOLV
kind: module
status: split
phase: P0
size: L
owners: []
claimed_by:
satisfies: [REQ-KRN.SOLV-001, REQ-KRN.SOLV-003, REQ-KRN.SOLV-009, REQ-KRN.SOLV-010]
covers: []
depends_on: [KRN.NUM.FN, KRN.NUM.RED, KRN.NUM.ERR]
uses_artifacts: [KRN.NUM.LINT.FIND, ECO.RULES.SPDX]
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# KRN.SOLV.CONIC: Vendored LP, QP and SOCP solver with KNP-1 audit

## Purpose

The interior-point conic solver that minimum-zone, envelope, tangential and GD&T zone problems are posed in. The concept baseline names a vendored fork of Clarabel (`fid-clarabel`). This node owns vendoring it, patching it to pass KNP-1, pinning its iteration limits and tolerances, wrapping it behind a kernel API, and keeping the audit record current.

## Scope

**In:**
- Vendoring a pinned upstream source tree into the kernel repository with its source digest
- KNP-1 patches: replacing `powi` and any transcendental with `fid-num` calls, removing threading and runtime dispatch, replacing hash maps, routing linear algebra through deterministic factorizations
- A kernel wrapper API for LP, QP and SOCP problems in a fixed canonical form, with fixed iteration limits and tolerances exported for the kernel manifest
- Typed errors for infeasibility, unboundedness, iteration limit and numerical failure
- The audit record: pinned upstream digest, patch list with rationale, lint result, and re-audit on every upstream update

**Out** (and which node owns it instead):
- Extracting active sets, recomputing values and checking KKT conditions (`KRN.SOLV.CERT`)
- Linearisation loops and multi-start (`KRN.SOLV.SEQ`, `KRN.SOLV.MSTART`)
- Out-of-kernel cross-check solvers run in CI (`KRN.MAN`)
- Adding the dependency to the allowlist (`ECO.RULES`)

## Requirements

- **REQ-KRN.SOLV.CONIC-001** The vendored solver shall live in the kernel repository at a pinned upstream commit whose source digest is recorded in the audit record.
- **REQ-KRN.SOLV.CONIC-002** After patching, the vendored solver shall pass the KNP-1 lint with no waivers other than those listed and justified in the audit record.
- **REQ-KRN.SOLV.CONIC-003** The wrapper shall solve LP, convex QP and SOCP problems given in one canonical form, single-threaded, with iteration limits and tolerances fixed in code and exported as data for the kernel manifest.
- **REQ-KRN.SOLV.CONIC-004** If the solver reports primal or dual infeasibility, reaches its iteration limit, or fails numerically, then the wrapper shall return a typed `SolverError` with the solver status and last residuals, and no solution.
- **REQ-KRN.SOLV.CONIC-005** On a frozen set of LP, QP and SOCP test problems, the wrapper's outputs shall be bit-identical on x86-64, ARM64 and wasm32.
- **REQ-KRN.SOLV.CONIC-006** When the upstream version is updated, CI shall require a new audit record and shall fail if any patch from the previous record is missing without a justification.

## Interfaces

- **Offers:** `fid_solv::conic::{solve_lp, solve_qp, solve_socp, ConicProblem, ConicSolution, ConicLimits}`; the audit record file; limits and tolerances for `KRN.MAN` manifest generation.
- **Consumes:** `KRN.NUM.FN`, `KRN.NUM.RED`, `KRN.NUM.ERR` (code); the KNP-1 lint report from `KRN.NUM.LINT.FIND` and the SPDX allowlist from `ECO.RULES.SPDX` (artifacts, read by the audit check). The earlier code dependency on `KRN.NUM.LA` is dropped because the patched solver keeps its own sparse LDL (see `KRN.SOLV.CONIC.PATCH`), and the dependency on `KRN.NUM.LINT` became an artifact dependency (D-034).

## Decomposition hints

VEND (vendoring, digest, KNP-1 patches and audit record), WRAP (canonical problem form, wrapper API, limits export and error mapping), XTEST (frozen conic test set and cross-target digest check).

## Open questions

- Clarabel's licence (Apache-2.0 [unverified]) must be on the allowlist that `ECO.RULES` writes under D-001.
- Whether Clarabel's internal sparse LDL factorization can be made deterministic by patching, or must be replaced by `fid-num` dense factorizations for the problem sizes the kernel uses [unverified].

## Children

The split follows the hints, with vendoring separated from patching: VEND is a tooling check over the vendored tree and audit record, PATCH is the kernel-layer code change, WRAP is the kernel-facing API, and XTEST is the cross-target proof.

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| KRN.SOLV.CONIC.VEND | Vendored solver tree, source digest and audit record check | 001 002 006 | none |
| KRN.SOLV.CONIC.PATCH | KNP-1 patches to the vendored interior-point solver | 002 | none |
| KRN.SOLV.CONIC.WRAP | Kernel conic API, fixed limits and error mapping | 003 004 | none |
| KRN.SOLV.CONIC.XTEST | Frozen conic test set and cross-target bit-identity check | 005 | none |
