---
id: UNC.ORS.UNC.NZERO
title: Non-negative quantities with true value zero
level: 4
parent: UNC.ORS.UNC
kind: task
status: open
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-UNC.ORS.UNC-001, REQ-UNC.ORS.UNC-003]
covers: []
depends_on: [UNC.ORS.UNC.HARNESS, UNC.RDL.GEN]
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# UNC.ORS.UNC.NZERO: Non-negative quantities with true value zero

## Purpose

Coverage cases for quantities that cannot be negative (form deviations, radial position) on parts whose true value is zero or near zero. The estimator is biased upwards there, so intervals can cover poorly even when the propagation is correct; the estimator policy of `UNC.PROP.SELECT` discloses that bias. Whether such cases pass on the disclosed-bias convention, on a different interval construction, or on a separate criterion needs an uncertainty expert, so this node stays open.

## Scope

**In:**
- Flatness and roundness of form-perfect parts, and radial position of a perfectly located hole, measured with noise
- The pass convention for biased non-negative estimators
- Cases at true value zero and at small positive true values (for example 1σ and 3σ of the noise)

**Out:**
- Harness (owned by `UNC.ORS.UNC.HARNESS`); unbiased cases (owned by `UNC.ORS.UNC.BASE`)

## Requirements

- **REQ-UNC.ORS.UNC.NZERO-001** The suite shall contain at least one non-negative quantity case with true value zero, with true value, generator, base seed, bound model and N ≥ 1000.
- **REQ-UNC.ORS.UNC.NZERO-002** Every case shall state the pass convention it is judged by and the estimator policy's disclosed bias.
- **REQ-UNC.ORS.UNC.NZERO-003** The pass convention shall be reviewed and recorded by an uncertainty expert before the case is released [unverified: convention].

## Research hypotheses

D-036 rule 4. Children state these as `AC-H` criteria. They measure bias and coverage only; the pass convention stays with the uncertainty expert (REQ-003).

- **H-1 (bias disclosure)** For flatness, roundness and radial position at true value zero, the bias disclosed by `UNC.PROP.SELECT`'s estimator policy agrees with the simulated mean bias within 10 % [unverified: threshold] over N ≥ 1000 repetitions, measured by `UNC.ORS.UNC.HARNESS` with each case's base seed; falsified if the relative difference exceeds the threshold for any case.
- **H-2 (hard-case discrimination)** At true value zero, the naive symmetric 95 % interval undercovers outside the binomial band, so that the case actually tests the biased regime, measured by the harness on the same seeds; falsified if the naive interval covers within the band (the case is then not hard enough and needs new geometry or noise).
- **H-3 (transition)** Observed coverage under the expert-recorded convention moves into the binomial band as the true value grows to 3σ of the noise [unverified: 3σ], measured at true values 0, 1σ and 3σ; falsified if coverage at 3σ lies outside the band.

## Interfaces

- **Offers:** near-zero cases for suite `ors-unc`.
- **Consumes:** harness (UNC.ORS.UNC.HARNESS); generators (UNC.RDL.GEN).

## Decomposition hints

FORM (flatness and roundness near zero), POS (radial position near zero); each a content task once the convention is fixed.

## Open questions

- Near-zero form cases are expected to show bias even when intervals cover; whether the pass criterion uses the disclosed-bias convention needs an uncertainty expert's review [unverified].
- The harness may need a second band type for one-sided intervals; if so it is a change to `UNC.ORS.UNC.HARNESS`.

## Children

_None yet: status `open`._
