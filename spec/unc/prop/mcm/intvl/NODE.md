---
id: UNC.PROP.MCM.INTVL
title: Coverage intervals and estimate outputs from samples
level: 4
parent: UNC.PROP.MCM
kind: task
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
built_in: [fid-kunc]
built_pr: 306
satisfies: [REQ-UNC.PROP.MCM-006]
covers: []
depends_on: [KRN.NUM.RED, KRN.NUM.FN]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# UNC.PROP.MCM.INTVL: Coverage intervals and estimate outputs from samples

## Purpose

Turns a set of Monte Carlo output values into the summary JCGM 101:2008 describes: the mean, the standard uncertainty, the probabilistically symmetric coverage interval and the shortest coverage interval, from the values sorted in total order. It also produces the difference between the MC mean and the kernel value at the measured inputs, which the estimator policy uses for bias disclosure.

## Scope

**In:**
- Total-order sort of values
- Mean and standard deviation by fixed-order reduction
- Probabilistically symmetric interval and shortest interval at coverage probability p, by order statistics with the index rule of JCGM 101:2008 [unverified clause 7.7]
- `mean_minus_fx` = MC mean minus the kernel value f(x)
- Per-block summaries for `UNC.PROP.MCM.ADAPT`

**Out:**
- Stopping (owned by `UNC.PROP.MCM.ADAPT`)
- Choosing the reported value (owned by `UNC.PROP.SELECT`)

## Requirements

- **REQ-UNC.PROP.MCM.INTVL-001** The function shall return mean, standard uncertainty, symmetric interval, shortest interval and `mean_minus_fx` for a value set, a coverage probability p and f(x).
- **REQ-UNC.PROP.MCM.INTVL-002** The symmetric interval shall be bounded by the order statistics at ranks round(M (1 - p) / 2) and that rank plus round(p M), with indices clamped to the sample, and the shortest interval shall be the shortest such window over all start ranks, with the lowest start rank winning ties.
- **REQ-UNC.PROP.MCM.INTVL-003** If the value set has fewer than ceil(1 / (1 - p)) values, then the function shall return `too_few_trials`.

## Acceptance criteria

- **AC-1** Given `fixtures/unc/mcm/intvl/uniform_1000.json` (the values 1..1000) and p = 0.95, then the symmetric interval is [the 25th, the 975th value] and the shortest interval has the same length with the lowest start.
- **AC-2** Given `fixtures/unc/mcm/intvl/skewed.json` (an exponential sample with seed 3), then the shortest interval is shorter than the symmetric one and both match the expected file.
- **AC-3** Given values with mean 1.5 and f(x) = 1.0, then `mean_minus_fx = 0.5`.
- **AC-4** (error path) Given 10 values and p = 0.95, then `too_few_trials`.
- **AC-H1** Hypothesis: the symmetric and shortest intervals at p = 0.95 from M = 10^6 values converge to the analytic endpoints of output distributions known in closed form (the rectangular-input additive model of JCGM 101:2008 [unverified: clause 9.2 variant] and an exponential distribution) with absolute error at most 3 times the asymptotic order-statistic standard error [unverified] for at least 99 of 100 seeds, measured per endpoint; falsified if fewer seeds pass.

## Design

- **Registry command(s):** none. Algorithm ID `unc.prop.mcm_interval@1.0.0`.
- **Data structures:** `McmSummary { mean, u, sym: CoverageInterval, shortest: CoverageInterval, mean_minus_fx, m }`.
- **Algorithm:** stable sort with `total_cmp`; sliding window for shortest; reductions in sorted order.
- **Errors:** `too_few_trials`, `invalid_p`.
- **Determinism:** D0.
- **Performance budget:** 10^6 values in under 200 ms [unverified until measured].
- **Crate:** `fid-kunc` (module `prop::mcm::interval`; KNP-1 applies).

## Test fixtures

`fixtures/unc/mcm/intvl/`: generated value sets with hand-checked results (CC-BY-4.0).

## Build prompt

> In crate `fid-kunc`, module `prop::mcm::interval`, implement `summarize(values, p, fx) -> Result<McmSummary, McmError>`: total-order sort; mean and standard deviation by fixed-order reduction in sorted order; symmetric interval from order statistics at ranks round(M(1-p)/2) and that rank plus round(pM), clamped; shortest interval as the shortest window of round(pM) ranks with the lowest start winning ties; `mean_minus_fx` = mean - fx; `too_few_trials` when M < ceil(1/(1-p)). Tests AC-1..AC-4. Constraints: KNP-1, CONSTITUTION 8.

## Open questions

- The rank rule must be checked against the free JCGM 101 text [unverified].
- **AC-H1 measured (fid-kunc `tests/interval.rs`, M = 10^6, 100 seeds, release build):** the symmetric-interval endpoints meet the hypothesis (rect4 100/100 and 100/100; exp1 100/100 and 99/100 seeds within 3 asymptotic standard errors). The shortest-interval endpoints do not: rect4 50/100 per endpoint (exact two-sided 95 % bounds [0.398, 0.602]), and the exp1 boundary endpoint 96/100 ([0.901, 0.989]) under a substitute tolerance of 3 / (M f(0)), because the order-statistic standard error is zero at a support boundary. At M = 10^4 rect4 passes 86/100, so the shortfall grows with M, which is consistent with shortest-window endpoints converging more slowly than M^(-1/2) [unverified: cube-root rate]. The hypothesis is falsified for shortest intervals as stated; its tolerance should be restated (for example against a long-run reference) by an uncertainty expert.

## Children

_Leaf: none._
