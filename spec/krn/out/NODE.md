---
id: KRN.OUT
title: Point selection and outlier policies for fits
level: 2
parent: KRN
kind: subsystem
status: open
phase: P1b
size: L
owners: []
claimed_by:
satisfies: [REQ-KRN-005, REQ-KRN-008]
covers: [GOM.CON.21, GOM.CON.23, PCD.CON.02, PW.CON.06, SA.REL.04]
depends_on: [KRN.LSQ, KRN.NUM, KRN.MAN]
cli: [fid fit select-points, fid fit reject-outliers]
mcp: [fit_select_points, fit_reject_outliers]
journal: [fit.select_points, fit.reject_outliers]
updated: 2026-11-16
---

# KRN.OUT: Point selection and outlier policies for fits

## Purpose

Decides which points a fit uses, and records that decision. Removing points changes every downstream value, so selection and outlier rejection are explicit, parameterised operators whose excluded set and impact are journaled. Silent cleaning is never allowed. This is separate from the fits so that one policy model serves every fit method and so that the audit trail is designed once.

## Scope

**In:**
- The `OutlierPolicy` operator types: n-sigma on residuals, robust M-estimator weights, RANSAC consensus (seeded), distance-to-nominal gate, tolerance-band selection
- Point-selection operators for fit inputs: by tolerance band around a nominal, by index set, by extraction source (probing hits, section points, surface patch points supplied by other domains)
- Feature filters on fitted results (for example, PC-DMIS-style filters applied before a fit)
- Iteration of reject-and-refit to a fixed point or iteration limit
- Excluded-point index set and its digest
- Impact disclosure: the fitted values with and without the removal
- Spec cards and algorithm IDs `filter.outlier.<method>@<semver>` and `filter.select.<method>@<semver>`

**Out** (and which node owns it instead):
- The fits themselves (`KRN.LSQ`, `KRN.MZ`, `KRN.ENV`)
- Noise and outlier removal on whole point clouds and meshes (`SCAN`)
- Producing section or surface point sets from scan data (`SCAN`) or probing hits (`HAL`, `CMM`)
- ISO 16610 profile filtration for form evaluation (`GDT`)
- GUI picking and lasso resolution into index sets (`VIZ`, `GEO` selection model)

## Requirements

- **REQ-KRN.OUT-001** The subsystem shall provide n-sigma, robust M-estimator, seeded RANSAC, distance-to-nominal and tolerance-band policies, each with explicit typed parameters and no hidden defaults.
- **REQ-KRN.OUT-002** When a policy is applied to a fit, the result shall record the policy ID, its parameters, the excluded-point index set and the digest of that set.
- **REQ-KRN.OUT-003** When a policy excludes at least one point, the result shall carry an impact disclosure with the fitted parameters and residual statistics both with and without the exclusion.
- **REQ-KRN.OUT-004** No fit in the kernel shall exclude, down-weight or reorder input points except through a recorded policy of this subsystem.
- **REQ-KRN.OUT-005** RANSAC and any other randomised policy shall draw only from the `fid-num` counter-based RNG with a journaled seed, so a replay with the same seed gives the identical excluded set.
- **REQ-KRN.OUT-006** If a policy would leave fewer points than the target primitive needs, or would exclude more than a configured fraction of the input (default 25 % [unverified]), then it shall return a typed error instead of a fit.
- **REQ-KRN.OUT-007** Iterative reject-and-refit shall stop at a fixed point or at a configured iteration limit, and the result shall report the number of iterations and which stop condition fired.
- **REQ-KRN.OUT-008** Point-selection operators shall accept index sets or semantic queries already resolved by the CPU, and shall reject any selection that refers to points outside the input set.
- **REQ-KRN.OUT-009** When a point-selection or outlier command is committed, the system shall journal one `fit.select_points` or `fit.reject_outliers` event with the policy ID, parameters, seed and excluded-set digest.

## Interfaces

- **Offers:** `OutlierPolicy` and `PointSelection` types consumed by every `Fit`; commands `fit.select_points@1` and `fit.reject_outliers@1` with matching CLI, MCP and journal names; excluded-set digests and impact disclosures for `RPT` and `TRUST`.
- **Consumes:** `KRN.LSQ` (trait and refits), `KRN.NUM` (RNG, reductions), `KRN.MAN` (spec cards); index sets from the `GEO` selection model.

## Decomposition hints

POL (policy types and parameters), STAT (n-sigma and M-estimators), RANSAC, GATE (distance-to-nominal and tolerance-band), SEL (point selection by index set or source), DISC (excluded set, digest and impact disclosure).

## Open questions

- C5 says agent-authored filters need human adoption before release; adoption is a CORE/TRUST mechanism, but this node must expose the provenance tag. Confirm the field name with CORE.
- The 25 % exclusion ceiling is a placeholder [unverified]; it may belong in a defaults profile.

## Children

_None yet: status `open`._
