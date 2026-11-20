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
- Probabilistically symmetric interval and shortest interval at coverage probability p, by order statistics with the index rule of JCGM 101:2008 §7.7.1 to §7.7.3
- `mean_minus_fx` = MC mean minus the kernel value f(x)
- Per-block summaries for `UNC.PROP.MCM.ADAPT`

**Out:**
- Stopping (owned by `UNC.PROP.MCM.ADAPT`)
- Choosing the reported value (owned by `UNC.PROP.SELECT`)

## Requirements

- **REQ-UNC.PROP.MCM.INTVL-001** The function shall return mean, standard uncertainty, symmetric interval, shortest interval and `mean_minus_fx` for a value set, a coverage probability p and f(x).
- **REQ-UNC.PROP.MCM.INTVL-002** With the integer q = floor(p M + 1/2) and r = ceil((M - q) / 2) (JCGM 101:2008 §7.7.1, §7.7.2), the symmetric interval shall be bounded by the order statistics at 1-based ranks r and r + q, and the shortest interval shall be the shortest window [x_(s), x_(s + q)] over start ranks s = 1 .. M - q (§7.7.3), with the lowest start rank winning ties.
- **REQ-UNC.PROP.MCM.INTVL-003** If the value set has fewer than 2 values or fewer than ceil(1 / (1 - p)) values, then the function shall return `too_few_trials`.

## Acceptance criteria

- **AC-1** Given `fixtures/unc/mcm/intvl/uniform_1000.json` (the values 1..1000) and p = 0.95, then the symmetric interval is [the 25th, the 975th value] and the shortest interval has the same length with the lowest start.
- **AC-2** Given `fixtures/unc/mcm/intvl/skewed.json` (an exponential sample with seed 3), then the shortest interval is shorter than the symmetric one and both match the expected file.
- **AC-3** Given values with mean 1.5 and f(x) = 1.0, then `mean_minus_fx = 0.5`.
- **AC-4** (error path) Given 10 values and p = 0.95, then `too_few_trials`.
- **AC-H1** Hypothesis: the symmetric and shortest intervals at p = 0.95 from M = 10^6 values converge to the output distributions known in closed form (the rectangular-input additive model of JCGM 101:2008 [unverified: clause 9.2 variant] and an exponential distribution), for at least 99 of 100 seeds per check: each symmetric endpoint within 3 times the asymptotic order-statistic standard error sqrt(p' (1 - p') / M) / f(x_p') of the analytic endpoint, with p' = (1 - p) / 2 or (1 + p) / 2; the shortest interval's width within 3 sqrt(p (1 - p) / M) / f(xi) of the analytic shortest width (xi its upper analytic endpoint), and its realised coverage F(high) - F(low) within 3 sqrt(p (1 - p) / M) of p. Shortest-interval endpoints are not tested per endpoint (they converge at about M^(-1/3), shorth behaviour); they are matched bit for bit against the oracle by AC-1, AC-2 and the rank cases. Falsified if any check passes for fewer seeds.

## Design

- **Registry command(s):** none. Algorithm ID `unc.prop.mcm_interval@1.0.0`.
- **Data structures:** `McmSummary { mean, u, sym: CoverageInterval, shortest: CoverageInterval, mean_minus_fx, m }`.
- **Algorithm:** stable sort with `total_cmp`; sliding window for shortest; reductions in sorted order.
- **Errors:** `too_few_trials`, `invalid_p`.
- **Determinism:** D0.
- **Performance budget:** 10^6 values in under 200 ms [unverified until measured].
- **Crate:** `fid-kunc` (module `prop::mcm::interval`; KNP-1 applies).

## Test fixtures

`fixtures/unc/mcm/intvl/`: generated value sets with hand-checked results (CC-BY-4.0), including rank-rule cases where p M is not an integer (M = 1012 and M = 51 at p = 0.95, M = 1000 at p = 0.6827), and `crate_bits.txt`, the crate's own D0 bit pins checked on every target.

## Build prompt

> In crate `fid-kunc`, module `prop::mcm::interval`, implement `summarize(values, p, fx) -> Result<McmSummary, McmError>`: total-order sort; mean and standard deviation by fixed-order reduction in sorted order; integer q = floor(pM + 1/2) and r = ceil((M - q)/2) (JCGM 101:2008 §7.7.1, §7.7.2); symmetric interval from the order statistics at 1-based ranks r and r + q; shortest interval as the shortest window [x_(s), x_(s+q)], s = 1..M-q, with the lowest start winning ties (§7.7.3); `mean_minus_fx` = mean - fx; `too_few_trials` when M < 2 or M < ceil(1/(1-p)). Tests AC-1..AC-4. Constraints: KNP-1, CONSTITUTION 8.

## Open questions

- **Resolved (#306 review):** the rank rule was checked against JCGM 101:2008 §7.7.1 to §7.7.3. The earlier rule r = round(M (1 - p) / 2), with clamping, disagreed with it for many M (for example M = 1012, p = 0.95 gave ranks (25, 986) instead of (26, 987)); REQ-002 now uses the integer q and r. The algorithm ID stays `unc.prop.mcm_interval@1.0.0` because the earlier rule never shipped. Lowest start wins is the documented deterministic tie-break for the shortest interval.
- **AC-H1 mis-specified (corrected, #306 review).** History: the first statement tested every endpoint against 3 asymptotic order-statistic standard errors. At M = 10^6 over 100 seeds the symmetric endpoints passed (rect4 100 and 100, exp1 100 and 99), but the shortest-interval endpoints did not: rect4 50/100 per endpoint (exact two-sided 95 % bounds [0.398, 0.602]), and the exp1 boundary endpoint 96/100 ([0.901, 0.989]) under a substitute tolerance of 3 / (M f(0)). This was a mis-specification, not an engine fault: shortest-interval endpoints converge at about M^(-1/3), so a per-endpoint standard-error test does not apply to them. AC-H1 now tests the shortest interval through its width and realised coverage. Measured with the restated criterion (fid-kunc `tests/interval.rs`, M = 10^6, release): every check passes for 99 or 100 of 100 seeds (worst: exp1 symmetric high, shortest width and shortest coverage, 99/100 each, bounds [0.946, 0.9997]).

## Children

_Leaf: none._
