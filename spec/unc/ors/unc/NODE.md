---
id: UNC.ORS.UNC
title: ORS-UNC: coverage of uncertainty intervals on known-truth parts
level: 3
parent: UNC.ORS
kind: module
status: split
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-UNC.ORS-005]
covers: []
depends_on: [UNC.ORS.TRACE, UNC.ORS.RUN, UNC.RDL.GEN]
uses_artifacts: [UNC.PROP.SELECT]
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# UNC.ORS.UNC: ORS-UNC: coverage of uncertainty intervals on known-truth parts

## Purpose

Tests whether the reported uncertainty intervals are honest. For simulated parts with a known true value, many seeded repetitions of simulated measurement and full propagation are run, and the fraction of reported 95 % intervals that contain the true value is compared with 95 % within a binomial tolerance. It includes the hard cases where estimators are biased: form and position deviations whose true value is near zero.

## Scope

**In:**
- Known-truth case definitions: geometry, simulated instrument and noise model, bound uncertainty model, true value
- Coverage harness: N seeded repetitions (N ≥ 1000), each simulating a measurement and running `unc.propagate_auto@1`, counting coverage
- Binomial acceptance band for the observed coverage
- Near-zero cases for non-negative quantities (form, radial position) with the estimator policy's bias disclosure

**Out:**
- The propagation engines (owned by `UNC.PROP`)
- Case format and checks (owned by `UNC.ORS.TRACE`)

## Requirements

- **REQ-UNC.ORS.UNC-001** Each ORS-UNC case shall define a known true value, a simulation model with seed, and a bound uncertainty model, and the harness shall run at least 1000 seeded repetitions.
- **REQ-UNC.ORS.UNC-002** A case shall pass when the observed coverage of the reported 95 % intervals lies within the two-sided 99 % binomial acceptance band around 0.95 for the number of repetitions [unverified: band choice].
- **REQ-UNC.ORS.UNC-003** The suite shall include at least one LPU-selected case, one MCM-selected case, one multivariate case with a correlated datum, and one non-negative quantity whose true value is zero.
- **REQ-UNC.ORS.UNC-004** The harness shall be deterministic class D2: the same seeds shall give the same coverage count on the same platform class.
- **REQ-UNC.ORS.UNC-005** If a case fails because the reported intervals undercover, then the run record shall state the observed coverage and the band so that the miscalibrated model can be identified.

## Research hypotheses

D-036 rule 4 (AGENTS.md rule 10): the task-specific uncertainty research risk. Children turn these into `AC-H` criteria. Thresholds marked [unverified] are proposals, not results.

- **H-1 (calibration)** On cases whose bound model matches the simulation exactly, `unc.propagate_auto@1` gives coverage of reported 95 % intervals within the exact two-sided 99 % binomial band for N ≥ 1000, for LPU, MCM and multivariate propagation, measured by `UNC.ORS.UNC.HARNESS` on `ors-unc` at each released version; falsified if any such case falls outside its band.
- **H-2 (fault discrimination)** The suite detects each of four seeded model faults (correlation dropped, standard uncertainty halved, rectangular input treated as normal with the same half-width, coverage factor 1 instead of the reported k) in ≥ 95 % [unverified] of 20 independent base seeds by at least one case, measured by running the suite against the faulted bound models; falsified if any fault is detected less often.
- **H-3 (public anchor)** `unc.propagate_auto@1` reproduces the worked-example results of JCGM 101:2008 (BIPM, freely published) [unverified: examples chosen] within that document's stated numerical tolerance, measured by encoding the examples as cited `ors-unc` cases; falsified if any reported value lies outside.
- **H-4 (harness calibration)** The harness false-failure rate on a calibrated stub is ≤ 1.5 % and its detection of true coverage 0.90 is ≥ 99 % at N = 1000 [unverified], measured with `test.interval_stub` over independent base seeds; falsified if either bound fails.
- **H-5 (near-zero disclosure)** For non-negative quantities at true value zero, the simulated mean bias of the estimator agrees with the bias that `UNC.PROP.SELECT`'s estimator policy discloses, within 10 % [unverified], measured by the harness over N ≥ 1000 repetitions; falsified if any near-zero case differs by more. The pass convention itself is expert work.
- **Gap:** no public dataset of known-truth GD&T measurements with uncertainty statements was found [unverified]; only the JCGM 101:2008 examples are public reference values, and they do not cover GD&T quantities or near-zero form.

## Interfaces

- **Offers:** suite `ors-unc@<semver>`; coverage statistics per case for the dashboard.
- **Consumes:** `unc.propagate_auto@1` (UNC.PROP.SELECT); simulation generators (UNC.RDL.GEN); case format (UNC.ORS.TRACE).

## Decomposition hints

HARNESS (repetition loop and binomial band, likely a leaf), CASES (the known-truth case set, content work with an uncertainty expert).

## Open questions

- Split: the harness (repetition loop, binomial band, reporting) and the cases whose correct coverage follows from standard theory (linear-Gaussian models where LPU is exact, a smooth nonlinear model for MCM, a correlated-datum linear model) are leaves. Near-zero non-negative cases (form, radial position) need an uncertainty expert to fix the pass convention under estimator bias, so NZERO stays open. ORS is Tooling: propagation is reached by command ID (`unc.propagate_auto@1`) through `UNC.ORS.RUN`'s bus dispatch, and `UNC.PROP.SELECT` is recorded as an artifact dependency (its published statement format), not linked.

- Near-zero form cases are expected to show bias even when intervals cover; whether the pass criterion for those cases uses the disclosed-bias convention needs an uncertainty expert's review [unverified].
- Runtime of 1000 repetitions with MCM per repetition may exceed the CI budget of `UNC.ORS.RUN`; a nightly job may be needed.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| UNC.ORS.UNC.HARNESS | Coverage harness, binomial band and coverage reporting (leaf) | 001 002 004 005 | none |
| UNC.ORS.UNC.BASE | Known-truth cases for LPU, MCM and correlated multivariate propagation (leaf) | 001 003 | none |
| UNC.ORS.UNC.NZERO | Non-negative quantities with true value zero | 001 003 | none |
