---
id: UNC.ORS.UNC
title: ORS-UNC: coverage of uncertainty intervals on known-truth parts
level: 3
parent: UNC.ORS
kind: module
status: open
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-UNC.ORS-005]
covers: []
depends_on: [UNC.ORS.TRACE, UNC.PROP, UNC.RDL.GEN]
cli: []
mcp: []
journal: []
updated: 2026-11-16
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

## Interfaces

- **Offers:** suite `ors-unc@<semver>`; coverage statistics per case for the dashboard.
- **Consumes:** `unc.propagate_auto@1` (UNC.PROP.SELECT); simulation generators (UNC.RDL.GEN); case format (UNC.ORS.TRACE).

## Decomposition hints

HARNESS (repetition loop and binomial band, likely a leaf), CASES (the known-truth case set, content work with an uncertainty expert).

## Open questions

- Near-zero form cases are expected to show bias even when intervals cover; whether the pass criterion for those cases uses the disclosed-bias convention needs an uncertainty expert's review [unverified].
- Runtime of 1000 repetitions with MCM per repetition may exceed the CI budget of `UNC.ORS.RUN`; a nightly job may be needed.

## Children

_None yet: status `open`._
