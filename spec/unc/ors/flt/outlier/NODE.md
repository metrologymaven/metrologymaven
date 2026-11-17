---
id: UNC.ORS.FLT.OUTLIER
title: Planted-outlier cases with exact excluded index sets
level: 4
parent: UNC.ORS.FLT
kind: task
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-UNC.ORS.FLT-002, REQ-UNC.ORS.FLT-003]
covers: []
depends_on: [UNC.ORS.TRACE, UNC.RDL.GEN]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# UNC.ORS.FLT.OUTLIER: Planted-outlier cases with exact excluded index sets

## Purpose

Generates ORS-FLT cases for every outlier policy offered by `fit.reject_outliers@1` (`KRN.OUT`: n-sigma, robust scale, distance-to-nominal gate, tolerance band, seeded RANSAC) and `cloud.remove_outliers@1` (`SCAN.CLOUD.FILTER`: statistical and radius removal): inlier points with bounded, constructed residuals and outliers planted far beyond every threshold, so the excluded index set is exact regardless of iteration details. Near-threshold cases are built where the policy's decision is decidable in closed form.

## Scope

**In:** generator `ors_flt::outlier`; per policy ≥ 3 cases: clear separation (outliers ≥ 50× threshold), near-threshold (one point at threshold × (1 ± 1e-6) where the policy definition makes the decision exact), and no-outlier (expected empty set); expected excluded index set and refit result where applicable; policy parameters recorded; `citations: none`, `basis: closed_form_math` (policies are project-defined).
**Out:** filter and resampling cases (owned by `UNC.ORS.FLT.FILTER`, `UNC.ORS.FLT.RESAMPLE`).

## Requirements

- **REQ-UNC.ORS.FLT.OUTLIER-001** For each outlier policy of `fit.reject_outliers@1` and `cloud.remove_outliers@1`, the generator shall emit at least a clear-separation, a near-threshold and a no-outlier case.
- **REQ-UNC.ORS.FLT.OUTLIER-002** Every case shall state the expected excluded index set exactly (tolerance: set equality) and, for `fit.reject_outliers@1`, the expected refit parameters with tolerance 1e-9 m.
- **REQ-UNC.ORS.FLT.OUTLIER-003** RANSAC cases shall state the seed, and their expected set shall be one that every consensus run with that seed must return because the inlier set is the unique maximum consensus by a margin of at least 10 % of points.
- **REQ-UNC.ORS.FLT.OUTLIER-004** The generator shall be byte-reproducible per seed and the suite shall pass `rdl.check_suite@1`.

## Acceptance criteria

- **AC-1** Given seed `0x464C5431`, then two runs give identical files and the suite check reports zero findings.
- **AC-2** Given the suite, every policy listed in Scope has the three case kinds.
- **AC-3** Given each near-threshold case, the 50-digit reference evaluation of the policy statistic places the planted point on the stated side of the threshold.

## Design

- **Registry command(s):** none; generator `ors-gen flt-outlier`.
- **Algorithm:** inliers on analytic features with residuals drawn from a bounded symmetric distribution; outliers planted at chosen indices; statistics recomputed at 50 digits; policy parameter names taken from the published command manifests (read as artifacts).
- **Determinism:** D0.
- **Performance budget:** generation under 20 s.
- **Crate:** `fid-ors` (Tooling).

## Test fixtures

`fixtures/ors/flt/` (generated, CC-BY-4.0).

## Build prompt

> In tooling crate `fid-ors`, add generator `ors_flt::outlier` (`ors-gen flt-outlier`), seeded. For every policy of `fit.reject_outliers@1` (n-sigma, robust scale, distance-to-nominal gate, tolerance band, seeded RANSAC) and `cloud.remove_outliers@1` (statistical, radius), emit clear-separation (outliers ≥ 50× threshold), near-threshold (one point at threshold × (1 ± 1e-6), decided at 50 digits) and no-outlier cases. Expected excluded index set (exact) and refit parameters (1e-9 m) for `fit.reject_outliers@1`; RANSAC cases record the seed and have a unique maximum consensus by ≥ 10 % of points. `citations: none`, `basis: closed_form_math`. Byte-reproducible; passes `rdl.check_suite@1`. Tests AC-1..AC-3.

## Open questions

- Policy parameter names must follow `KRN.OUT.POL` and `SCAN.CLOUD.FILTER` as built.

## Children

_Leaf: none._
