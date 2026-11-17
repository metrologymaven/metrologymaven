---
id: RPT.STATS.CAP.SIGMA
title: Within-subgroup spread estimators
level: 4
parent: RPT.STATS.CAP
kind: task
status: leaf
phase: P1b
size: S
owners: []
claimed_by:
satisfies: [REQ-RPT.STATS.CAP-001, REQ-RPT.STATS.CAP-005]
covers: []
depends_on: [RPT.STATS.SPC.LIM, RPT.STATS.SPC.CONST, KRN.NUM]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# RPT.STATS.CAP.SIGMA: Within-subgroup spread estimators

## Purpose

Named, versioned estimators of the within-subgroup standard deviation used for Cp and Cpk. They reuse the subgroup statistics of `RPT.STATS.SPC.LIM` and the constants of `RPT.STATS.SPC.CONST`, so capability and control charts agree on subgroups and constants. Which estimator is the default is an expert choice and is flagged.

## Scope

**In:**
- Estimators `rbar_d2@1`, `sbar_c4@1`, `mrbar_d2@1` and `pooled_sd@1`
- Estimator IDs and the constants table reference they record

**Out:**
- Subgroup statistics (`RPT.STATS.SPC.LIM`) and subgrouping (`RPT.STATS.SPC.SUB`)
- Index formulas (`RPT.STATS.CAP.INDEX`)

## Requirements

- **REQ-RPT.STATS.CAP.SIGMA-001** The estimator `rbar_d2@1` shall return R̄/d2(n), `sbar_c4@1` shall return s̄/c4(n), and `mrbar_d2@1` shall return MR̄/d2(2), with constants from `RPT.STATS.SPC.CONST`.
- **REQ-RPT.STATS.CAP.SIGMA-002** The estimator `pooled_sd@1` shall return √(Σ (n_j − 1)·s_j² / Σ (n_j − 1)) over the used subgroups, with sums by `fid-num` fixed-order reductions in subgroup order and no c4 correction [unverified whether a correction is expected].
- **REQ-RPT.STATS.CAP.SIGMA-003** Every estimate shall record its estimator ID and the constants table ID, version and review status it used (none for `pooled_sd@1`).
- **REQ-RPT.STATS.CAP.SIGMA-004** If an estimator needs subgroups and the subgrouping is `INDIVIDUALS`, or needs individuals and subgroups have n > 1, then it shall fail with `stats.estimator_mismatch`.
- **REQ-RPT.STATS.CAP.SIGMA-005** Estimates shall be bit-identical on every supported target for the same inputs (D0).

## Acceptance criteria

- **AC-1** Given fixture `fixtures/rpt/stats/spc_xbar_r_25x5`, when each subgroup estimator runs, then `rbar_d2@1` equals R̄/2.326 and `sbar_c4@1` equals s̄/0.9400 using the seed constants [unverified constants], bit for bit with a reference computation in the test.
- **AC-2** Given fixture `fixtures/rpt/stats/spc_imr_30`, when `mrbar_d2@1` runs, then it equals MR̄/1.128.
- **AC-3** Given subgroups of equal size, when `pooled_sd@1` runs, then it equals the square root of the mean of the subgroup variances within 1 ulp.
- **AC-4** (error path) Given an `INDIVIDUALS` subgrouping, when `rbar_d2@1` runs, then it fails with `stats.estimator_mismatch`.

## Design

- **Registry commands:** none (kernel functions). `side_effect: R1` is that of `stats.compute_capability@1`.
- **Kernel function:** `fn within_sigma(stats: &SubgroupStats, estimator: EstimatorId) -> Result<SigmaEstimate, StatsError>` where `SubgroupStats` comes from `subgroup_stats` (SPC.LIM).
- **Data structures:** `SigmaEstimate { sigma: f64, estimator: EstimatorId, constants: Option<TableRef> }`.
- **Algorithm:** REQ-001 and REQ-002; division and sqrt via IEEE and `fid-num`.
- **Errors:** `stats.estimator_mismatch`, `stats.subgroup_size_unsupported`.
- **Determinism:** D0.
- **Crate:** `fid-stats` (kernel graph; KNP-1).

## Test fixtures

- `fixtures/rpt/stats/spc_xbar_r_25x5` and `spc_imr_30` (shared with `RPT.STATS.SPC.LIM`).

## Build prompt

> In crate `fid-stats` (kernel graph, KNP-1), implement `within_sigma(stats: &SubgroupStats, estimator) -> Result<SigmaEstimate, StatsError>` using `subgroup_stats` (module SPC.LIM) and `constants(n)` (module SPC.CONST). Estimators: `rbar_d2@1` = R̄/d2(n); `sbar_c4@1` = s̄/c4(n); `mrbar_d2@1` = MR̄/d2(2) (individuals only); `pooled_sd@1` = sqrt(Σ(n_j − 1)s_j² / Σ(n_j − 1)) with `fid-num` fixed-order sums. Record estimator ID and constants table ID, version and review status. Wrong subgrouping for the estimator -> `stats.estimator_mismatch`.
>
> **Tests:** AC-1 to AC-4 above with fixtures `fixtures/rpt/stats/spc_xbar_r_25x5` and `spc_imr_30`.
>
> **Constraints:** CONSTITUTION §8; KNP-1; C1. Spec: `spec/rpt/stats/cap/sigma/NODE.md`.

## Open questions

- **Expert choice (open):** the default within-subgroup estimator (this tree proposes `rbar_d2@1` for subgroups and `mrbar_d2@1` for individuals [unverified]) and whether `pooled_sd@1` should carry a bias correction.

## Children

_Leaf: none._
