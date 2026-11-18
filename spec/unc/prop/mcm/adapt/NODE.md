---
id: UNC.PROP.MCM.ADAPT
title: Adaptive stopping rule and trial cap
level: 4
parent: UNC.PROP.MCM
kind: task
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-UNC.PROP.MCM-002, REQ-UNC.PROP.MCM-003]
covers: []
depends_on: [UNC.PROP.MCM.INTVL, KRN.NUM.RED, KRN.NUM.FN, KRN.MAN.CARD]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# UNC.PROP.MCM.ADAPT: Adaptive stopping rule and trial cap

## Purpose

The pure decision, after each completed block, whether Monte Carlo has converged. Rule `mcm-adapt@1` paraphrases the adaptive procedure of JCGM 101:2008 [unverified: clause 7.9]. For each block, take the estimate, standard uncertainty and both coverage-interval endpoints. Compute the standard deviation of their block averages. Stop when twice each of those standard deviations is within the numerical tolerance, which is derived from the number of significant decimal digits wanted in u(y). Stop at the trial cap otherwise and flag `not_converged`.

## Scope

**In:**
- Spec card `mcm-adapt@1` in our own words, citing JCGM 101:2008 by designator and clause only
- Block size M = max(10 000, ceil(100 / (1 - p))) [unverified]; minimum two blocks
- Tolerance delta = half of one unit in the last significant digit of u(y) at `n_dig` digits (default 2)
- Stop test over per-block summaries; trial cap (default 10^6 [unverified]); `not_converged` with achieved tolerance
- Profile field `gum_suite_profile` (default `jcgm101-2008`), so a revised Supplement 1 can be adopted as a new profile version

**Out:**
- Per-block summaries (computed with `UNC.PROP.MCM.INTVL`)
- Running blocks (owned by `UNC.PROP.MCM.BLOCK` and `UNC.PROP.MCM.CMD`)

## Requirements

- **REQ-UNC.PROP.MCM.ADAPT-001** After each completed block from the second onward, the rule shall return `stop` exactly when twice the standard deviation of the block averages of the estimate, u(y), the lower endpoint and the upper endpoint are each no greater than delta.
- **REQ-UNC.PROP.MCM.ADAPT-002** The rule shall compute delta from the current overall u(y) expressed with `n_dig` significant decimal digits, as half a unit of the last retained digit.
- **REQ-UNC.PROP.MCM.ADAPT-003** If the next block would exceed the trial cap, then the rule shall return `cap_reached` with the achieved values of the four standard deviations and delta, and the result shall carry flag `not_converged`.
- **REQ-UNC.PROP.MCM.ADAPT-004** The rule shall record `mcm-adapt@1`, `gum_suite_profile`, block size, `n_dig` and cap in its output.

## Acceptance criteria

- **AC-1** Given `fixtures/unc/mcm/adapt/summaries_converge.json` (hand-computed block summaries), when the rule is applied block by block, then it returns `stop` at the expected block count.
- **AC-2** Given `fixtures/unc/mcm/adapt/summaries_slow.json` with cap 30 000 and M = 10 000, then it returns `cap_reached` after block 3 with the achieved values.
- **AC-3** Given u(y) = 0.0123456 and n_dig = 2, then delta = 0.0005.
- **AC-4** Given p = 0.99, then M = 10 000; given p = 0.9999, then M = 1 000 000.
- **AC-H1** Hypothesis: the `mcm-adapt@1` stop rule delivers its promised accuracy: with n_dig = 2, the stopped interval endpoints and u(y) lie within delta of a reference (analytic where the guide gives one, otherwise a 10^8-trial run [unverified]) in at least 95 % [unverified] of 100 independent seeds on each of the JCGM 101:2008 worked examples (additive model, mass calibration, comparison loss, gauge-block calibration [unverified: clause 9 numbering]), encoded from their published input definitions with the published results as reference values, measured by counting seeds within delta; falsified if any example falls below 95 %.
- **AC-H2** Hypothesis: the trial count at stop is within a factor of 2 [unverified] of the trial count the guide reports for its adaptive runs on the same JCGM 101:2008 examples, measured by the median over 100 seeds; falsified if any example's median lies outside that factor.

## Design

- **Registry command(s):** none. Algorithm ID `unc.prop.mcm_adapt@1.0.0`.
- **Data structures:** `AdaptProfile { version, gum_suite_profile, p, n_dig, cap, block_size }`; `BlockSummary { y_mean, u, low, high }`; `AdaptDecision { Continue | Stop | CapReached { sd: [f64; 4], delta } }`.
- **Algorithm:** fixed-order sums over block summaries; delta via decimal exponent from `fid-num` log10 and power functions with an exact correction step at digit boundaries.
- **Errors:** `invalid_profile` (p outside (0, 1), n_dig < 1, cap < 2 M).
- **Determinism:** D0.
- **Performance budget:** trivial [unverified until measured].
- **Crate:** `fid-kunc` (module `prop::mcm::adapt`; KNP-1 applies).

## Test fixtures

`fixtures/unc/mcm/adapt/`: hand-computed summary sequences (CC-BY-4.0).

## Build prompt

> In crate `fid-kunc`, module `prop::mcm::adapt`, implement rule `mcm-adapt@1` (algorithm `unc.prop.mcm_adapt@1.0.0`) as a pure function over per-block summaries (estimate, u, lower and upper interval endpoints). Block size M = max(10 000, ceil(100/(1-p))), minimum two blocks. delta = half a unit of the last of `n_dig` (default 2) significant digits of the overall u(y). Return Stop when 2 x standard deviation of the block averages of each of the four quantities <= delta; CapReached (with achieved values) when the next block would exceed the cap (default 10^6); else Continue. Record version, `gum_suite_profile` (default `jcgm101-2008`), M, n_dig and cap. Write a spec card in your own words citing JCGM 101:2008 by designator and clause only. Tests AC-1..AC-4. Constraints: KNP-1, CONSTITUTION 8.

## Open questions

- Clause numbering and the block-size rule must be checked against the free JCGM 101 text by an uncertainty expert [unverified].

## Children

_Leaf: none._
