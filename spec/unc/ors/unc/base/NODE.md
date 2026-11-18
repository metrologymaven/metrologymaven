---
id: UNC.ORS.UNC.BASE
title: Known-truth cases for LPU, MCM and correlated multivariate propagation
level: 4
parent: UNC.ORS.UNC
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-UNC.ORS.UNC-001, REQ-UNC.ORS.UNC-003]
covers: []
depends_on: [UNC.ORS.UNC.HARNESS, UNC.RDL.GEN]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# UNC.ORS.UNC.BASE: Known-truth cases for LPU, MCM and correlated multivariate propagation

## Purpose

The ORS-UNC cases whose correct coverage follows from standard theory: a linear model with Gaussian inputs (LPU is exact, so the reported interval must cover at the nominal rate), a smooth nonlinear model with a non-Gaussian input where method selection picks MCM, and a linear multivariate case where a hole position is measured relative to a datum whose estimate is correlated with it. Each case ships with its generator parameters, true value and bound uncertainty model.

## Scope

**In:**
- `lin-gauss`: length L = L_ref + d with L_ref ~ normal (certificate) and d the mean of 5 readings with normal noise; true value fixed; LPU-selected
- `nonlin-rect`: circle radius from 8 points with rectangular probing error and a rectangular thermal input; MCM-selected [unverified: selection outcome depends on `UNC.PROP.SELECT` thresholds]
- `mvar-datum`: 2D hole position relative to a datum line fitted from noisy points, sharing noise sources, correlation between datum and hole estimates ≥ 0.5 by construction; multivariate
- Uncertainty models for each (`uncertainty-model/v1`) and case files with N = 1000 (LPU, multivariate) and N = 1000 tagged `nightly` (MCM)

**Out:** harness (owned by `UNC.ORS.UNC.HARNESS`); near-zero cases (owned by `UNC.ORS.UNC.NZERO`).

## Requirements

- **REQ-UNC.ORS.UNC.BASE-001** The suite shall contain the three cases in Scope, each with true value, generator parameters, base seed, bound model reference and N ≥ 1000.
- **REQ-UNC.ORS.UNC.BASE-002** The bound model of each case shall describe exactly the simulated error sources (same distributions and parameters), so that miscoverage indicates a propagation fault, not a modelling gap.
- **REQ-UNC.ORS.UNC.BASE-003** The `mvar-datum` case shall have a constructed correlation of at least 0.5 between the datum and hole estimates, verified analytically in the case notes' supporting script.
- **REQ-UNC.ORS.UNC.BASE-004** Every case shall carry `citations: none` with basis `closed_form_math` unless it cites JCGM 100:2008 or JCGM 101:2008 by clause.

## Acceptance criteria

- **AC-1** Given the case files, then `rdl.check_suite@1` reports zero findings and each case's bound model passes `unc.validate_model@1`.
- **AC-2** Given `lin-gauss` with the harness and an exact LPU stub, then coverage lies in the band.
- **AC-3** Given `mvar-datum` with correlation deliberately dropped from the bound model (test variant), then the harness reports a failure, showing the case is sensitive to correlation [unverified: sensitivity strength; tune geometry until it fails reliably].
- **AC-H1** Hypothesis: `unc.propagate_auto@1` achieves coverage within the exact two-sided 99 % binomial band around 0.95 (about [931, 967] of 1000 by our calculation [unverified]) on the `lin-gauss` and `mvar-datum` cases of `ors-unc` at the first released suite version, with N = 1000 and the case's base seed, measured by `UNC.ORS.UNC.HARNESS`; falsified if either coverage count lies outside the band.
- **AC-H2** Hypothesis: MCM selected by `unc.propagate_auto@1` achieves coverage within the same band on `nonlin-rect` (N = 1000, nightly run) at the first released suite version, measured by `UNC.ORS.UNC.HARNESS`; falsified if the count lies outside the band or the case is not selected for MCM.
- **AC-H3** Hypothesis: `unc.propagate_auto@1` reproduces the estimate, standard uncertainty and 95 % coverage-interval endpoints of the worked examples in the BIPM-published JCGM 101:2008 [unverified: which examples are encoded; clause numbers to be cited by a person] to within the numerical tolerance that document's adaptive Monte Carlo procedure defines for the published digits, measured by encoding each example as an `ors-unc` case citing its clause (REQ-004) and comparing reported values; falsified if any reported value lies outside that tolerance.
- **AC-H4** Hypothesis: the `mvar-datum` case discriminates the seeded fault "correlation dropped" in ≥ 95 % [unverified: threshold] of 20 independent base seeds (published with the suite version), while the full bound model passes in at least 19 of 20, measured by `UNC.ORS.UNC.HARNESS` on both model variants; falsified if the fault is detected in fewer than 19 of 20 seeds or the full model fails in more than 1.

## Design

- **Registry command(s):** none; case files plus generator parameters.
- **Algorithm:** generators in `UNC.RDL.GEN` style, seeded per repetition by the harness.
- **Determinism:** D0 for the case files.
- **Performance budget:** n/a.
- **Crate:** `fid-ors` (Tooling; suite data under `suites/ors-unc/`).

## Test fixtures

`suites/ors-unc/` case files and models (project-authored, CC-BY-4.0).

## Build prompt

> In tooling crate `fid-ors`, add suite `ors-unc` cases `lin-gauss` (LPU-selected linear Gaussian length), `nonlin-rect` (8-point circle radius with rectangular errors, MCM-selected, tag `nightly`) and `mvar-datum` (2D hole position relative to a fitted datum line with constructed correlation ≥ 0.5), each with true value, generator parameters, base seed, N = 1000 and a bound `uncertainty-model/v1` that matches the simulated error sources exactly. `citations: none`, `basis: closed_form_math` (or JCGM clause citations). Tests AC-1..AC-3 with the `ors.coverage` harness; include the correlation-dropped variant as a negative test.

## Open questions

- Whether `nonlin-rect` is selected for MCM depends on `UNC.PROP.SELECT`'s nonlinearity thresholds; if not, adjust the case until it is.

## Children

_Leaf: none._
