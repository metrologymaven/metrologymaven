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
built_in: [fid-kunc]
built_pr: 311
satisfies: [REQ-UNC.PROP.MCM-002, REQ-UNC.PROP.MCM-003]
covers: []
depends_on: [UNC.PROP.MCM.INTVL, KRN.NUM.RED, KRN.NUM.FN, KRN.MAN.CARD]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-20
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
- **AC-H1** Implementation check (restated on #311 by maintainer decision, see Open questions): with p = 0.95, n_dig = 2, the default cap and the probabilistically symmetric interval, run the rule over 1000 seeds on each of the two JCGM 101:2008 additive-model examples with closed-form output distributions (Y = X1 + X2 + X3 + X4 with standard normal inputs, and with rectangular inputs of unit standard deviation [unverified: clause 9.2 numbering]). For each of the four quantities (the estimate, analytic value 0; u(y); the lower and upper endpoint, all from all trials of the run), count the runs that end within delta of the analytic value. Each count must agree, by a two-sided exact binomial test at level 0.01, with the rate predicted for the §7.9.4 rule by an independent reference simulation (`fixtures/unc/mcm/adapt/make_fixtures.py --simulate`, Python's own generator, 10 000 runs per example) [simulated]. Because the predicted rate is itself a simulated count, the test is the exact conditional (Fisher) comparison of the two binomial counts; its acceptance bands are in `ac_h_reference.json`. The check fails if any count lies outside its band. §7.9 gives no coverage-of-truth guarantee; the shortfall below 0.95 is a property of the rule (s with h−1 dof, optional stopping). The full count comparison runs only with `UNC_MCM_ADAPT_FULL=1` (release, about 30 s), not in CI; CI runs a 4-seed smoke of the same code path and the D0 bit pins.
- **AC-H2** Hypothesis (restated on #311): the trial count at stop is within a factor of 2 [unverified] of the count predicted from the asymptotic per-block standard deviation of each block statistic (mean sigma / sqrt(M); standard deviation sigma sqrt((kappa - 1) / (4 M)); order statistic sqrt(q (1 - q) / M) / f(x_q)) as h* M with h* = max(2, ceil(max (2 sigma_k / delta)^2)), on the same two examples, measured by the median over the same 1000 seeds; the check fails if either example's median lies outside that factor. Like AC-H1, it is measured only with `UNC_MCM_ADAPT_FULL=1`, not in CI.

## Design

- **Registry command(s):** none. Algorithm ID `unc.prop.mcm_adapt@1.0.0`.
- **Data structures:** `AdaptProfile { version, gum_suite_profile, p, n_dig, cap, block_size }`; `BlockSummary { y_mean, u, low, high }`; `AdaptDecision { Continue | Stop | CapReached { sd: [f64; 4], delta } }`.
- **Algorithm:** fixed-order sums over block summaries; delta via decimal exponent from `fid-num` log10 and power functions with an exact correction step at digit boundaries.
- **Errors:** `invalid_profile` (p outside (0, 1), n_dig < 1, cap < 2 M).
- **Determinism:** D0.
- **Performance budget:** trivial [unverified until measured].
- **Crate:** `fid-kunc` (module `prop::mcm::adapt`; KNP-1 applies).

## Test fixtures

`fixtures/unc/mcm/adapt/`: hand-chosen synthetic summary sequences with oracle-computed decisions, tolerance and block-size tables, and the AC-H analytic references and acceptance bands, all from `make_fixtures.py` (stdlib Python, exact rationals); `ac_h_simulated.json` holds the [simulated] reference-simulation counts (`make_fixtures.py --simulate`, not run in CI); `crate_bits.txt` pins the crate's own output bits (CC-BY-4.0). The spec card is `crates/fid-kunc/cards/unc.prop.mcm_adapt@1.0.0.toml`.

## Build prompt

> In crate `fid-kunc`, module `prop::mcm::adapt`, implement rule `mcm-adapt@1` (algorithm `unc.prop.mcm_adapt@1.0.0`) as a pure function over per-block summaries (estimate, u, lower and upper interval endpoints). Block size M = max(10 000, ceil(100/(1-p))), minimum two blocks. delta = half a unit of the last of `n_dig` (default 2) significant digits of the overall u(y). Return Stop when 2 x standard deviation of the block averages of each of the four quantities <= delta; CapReached (with achieved values) when the next block would exceed the cap (default 10^6); else Continue. Record version, `gum_suite_profile` (default `jcgm101-2008`), M, n_dig and cap. Write a spec card in your own words citing JCGM 101:2008 by designator and clause only. Tests AC-1..AC-4. Constraints: KNP-1, CONSTITUTION 8.

## Open questions

- Clause numbering and the block-size rule must be checked against the free JCGM 101 text by an uncertainty expert [unverified].
- **Applied (AC-H1 restated as an implementation check, #311 maintainer decision).** §7.9 gives no coverage-of-truth guarantee; the shortfall below 0.95 is a property of the rule (s with h−1 dof, optional stopping). The review checked the implementation against §7.9 clause by clause and recomputed delta exactly. In the reference simulation, 23 % (normal inputs) and 28 % (rectangular inputs) of runs stop at h = 2, where s has one degree of freedom and optional stopping favours a small s. The expectation of at least 95 % was ours, not the rule's. AC-H1 therefore now checks that the crate reproduces the rate the rule itself produces, as predicted by an independent reference simulation, rather than a coverage target. The estimate was added as a fourth quantity.
- **Measured (simulated, #311; `UNC_MCM_ADAPT_FULL=1`, release, 1000 seeds per example).** AC-H1: every count agrees with its predicted rate at level 0.01. Normal inputs: estimate 999 (predicted rate 1.0000, band [999, 1000]), u(y) 1000 (1.0000, [999, 1000]), low 935 (0.9304, [908, 951]), high 940 (0.9298, [907, 951]). Rectangular inputs: estimate 1000, u(y) 1000 (both 1.0000, [999, 1000]), low 938 (0.9445, [924, 963]), high 950 (0.9412, [920, 960]). The predicted rates come from 10 000 Python reference runs per example [simulated]. AC-H2: median trials at stop 50 000 (normal inputs, predicted 50 000) and 40 000 (rectangular inputs, predicted 40 000), supported. No run reached the cap.
- **History (AC-H1 and AC-H2, kept for the record).** The leaf first stated AC-H1 as a hypothesis that the rule meets a 95 % accuracy target: at least 95 of 100 seeds on four JCGM 101:2008 worked examples. AC-H2 compared trial counts with the counts the guide reports. The first build restated both: an exact one-sided binomial test of a rate of at least 0.95 over 1000 seeds, on the two closed-form additive-model examples. Under that restatement the normal-input lower endpoint (935/1000, exact two-sided 95 % bounds [0.918, 0.949]) fell below the one-sided threshold. Under the original 100-seed wording the normal-input endpoints scored 91 and 94 of 100. Review on #311 found the 95 % expectation to be a misreading of §7.9, which promises a numerical tolerance, not coverage of the true value. The mass-calibration, comparison-loss and gauge-block examples have no closed form, and their published reference values are not encoded or verified here. Those checks, and the comparison with the guide's reported adaptive trial counts, are AC-H3 of `UNC.PROP.MCM.CMD`, which already runs all four examples end to end.
- **Applied (block size).** p is read as the decimal it was written as (its shortest round-trip form), so AC-4's p = 0.9999 gives M = 10^6. The nearest binary64 to 0.9999 is slightly above it, and would give 10^6 + 1.
- **Applied (spread).** The procedure's standard deviation of the block values is the standard deviation associated with their average (divisor h (h - 1)), as in JCGM 101:2008 §7.9.4 [unverified: clause numbering]. It is not the plain standard deviation of the block values.
- **Applied (exactness).** delta's digit position and the test 2 s <= delta are exact decimal comparisons (a small big-integer compare), not fid-num log10 and power functions as the Design suggested. The result is the same rule without rounding at digit boundaries. n_dig is limited to 1..17 (`invalid_profile` above 17, the most a binary64 holds). u(y) = 0 gives delta = 0, which stops only when every s is 0.
- **Applied (overall u(y)).** The rule takes the overall u(y) of all trials so far as an input (the caller computes it from all values, as the procedure does) rather than pooling it from the block summaries.

## Children

_Leaf: none._
