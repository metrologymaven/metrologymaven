---
id: UNC.RDL.GEN.STRESS
title: Noise-level and outlier stress families
level: 4
parent: UNC.RDL.GEN
kind: task
status: leaf
phase: P0
size: S
owners: []
claimed_by:
satisfies: [REQ-UNC.RDL.GEN-004]
covers: []
depends_on: [UNC.RDL.GEN.LSNULL]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# UNC.RDL.GEN.STRESS: Noise-level and outlier stress families

## Purpose

Null-space least-squares cases with large residuals: high noise levels relative to feature size, and a few gross outliers. Because the residuals are built in the orthogonal complement of the Jacobian's column space, the least-squares reference stays exact even with outliers. These cases test the conditioning and convergence of the fits, not outlier rejection.

## Scope

**In:**
- Families `ls.<primitive>.noise` (RMS residual from 1e-7 to 1e-2 of feature size) and `ls.<primitive>.outlier` (1 to 5 points with residuals 100 times the RMS, still in the complement) for circle, sphere and cylinder
- Metadata: relative noise level and outlier indices

**Out:**
- Outlier-rejection test data (owned by KRN.OUT tests); the null-space builder (owned by `UNC.RDL.GEN.LSNULL`)

## Requirements

- **REQ-UNC.RDL.GEN.STRESS-001** Each stress family shall build its residuals in the orthogonal complement of the Jacobian's column space, so the reference stays the constructed solution before rounding.
- **REQ-UNC.RDL.GEN.STRESS-002** Outlier cases shall record the outlier point indices and their residuals in the case metadata.

## Acceptance criteria

- **AC-1** Given seed 3, when `ls.circle.outlier` is generated, then every case passes the optimality check of LSNULL with outlier indices recorded.
- **AC-2** Given the mpmath oracle file `fixtures/rdl/gen/stress/oracle_seed3.json`, then references agree to 1e-40 relative.
- **AC-3** Given seed 3 twice, then byte-identical records.

## Design

- **Registry command(s):** none; families registered for `UNC.RDL.GEN.CMD`.
- **Data structures:** `StressSpec { rel_rms, outliers }`.
- **Algorithm:** complement projection with a residual vector whose chosen entries are scaled up and then re-projected iteratively until the outlier entries keep at least 90 % of their target magnitude.
- **Errors:** `outlier_projection_failed` if that is not reached in 20 iterations.
- **Determinism:** D0.
- **Performance budget:** each family in under 60 s [unverified until measured].
- **Crate:** `fid-rdl-gen` (module `stress`).

## Test fixtures

`fixtures/rdl/gen/stress/` oracle file for seed 3 (CC-BY-4.0).

## Build prompt

> In crate `fid-rdl-gen`, module `stress`, add families `ls.{circle,sphere,cylinder}.noise` (relative RMS 1e-7 to 1e-2) and `ls.{circle,sphere,cylinder}.outlier` (1 to 5 outliers at 100 x RMS) using the LSNULL builder. Keep residuals in the orthogonal complement of the Jacobian's column space (re-project iteratively, up to 20 times, until outliers keep >= 90 % of target magnitude, else `outlier_projection_failed`). Record noise level and outlier indices. Tests AC-1..AC-3. CONSTITUTION 8.

## Open questions

- None.

## Children

_Leaf: none._
