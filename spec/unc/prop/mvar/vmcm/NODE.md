---
id: UNC.PROP.MVAR.VMCM
title: Joint Monte Carlo samples and sample covariance
level: 4
parent: UNC.PROP.MVAR
kind: task
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-UNC.PROP.MVAR-002, REQ-UNC.PROP.MVAR-003]
covers: []
depends_on: [UNC.PROP.MVAR.PARAM, UNC.PROP.MCM.BLOCK, UNC.PROP.MCM.SAMP, KRN.NUM.RED]
uses_artifacts: [UNC.ORS.UNC]
side_effect: R1
determinism: D2
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# UNC.PROP.MVAR.VMCM: Joint Monte Carlo samples and sample covariance

## Purpose

The Monte Carlo form of multivariate propagation (JCGM 102:2011): all outputs of a characteristic group are evaluated in the same trial, so their joint sample carries the correlation induced by shared inputs. From it this node computes the sample covariance and a sample-based coverage region (the smallest Mahalanobis-distance ball, in the sample covariance metric, that contains a fraction p of the trials).

## Scope

**In:**
- Joint per-trial vectors from `UNC.PROP.MCM.BLOCK` outputs of a group (failed trials excluded jointly)
- Sample mean and covariance, fixed order
- Region: the order statistic of Mahalanobis distances at rank ceil(p M) [unverified rule per JCGM 102]
- Joint sample sets in `mcm-samples@1` with several outputs

**Out:**
- Block evaluation and stopping (owned by `UNC.PROP.MCM`)
- LPU form (owned by `UNC.PROP.MVAR.VLPU`)

## Requirements

- **REQ-UNC.PROP.MVAR.VMCM-001** The engine shall compute the sample covariance of a group from per-trial joint output vectors, excluding every trial in which any output failed.
- **REQ-UNC.PROP.MVAR.VMCM-002** The engine shall return a coverage region defined by the sample mean, sample covariance and the Mahalanobis-distance order statistic at rank ceil(p M).
- **REQ-UNC.PROP.MVAR.VMCM-003** On the ORS-UNC position case with a correlated datum, the joint sample covariance shall be the MCM reference against which `UNC.PROP.MVAR.VLPU` is checked.

## Acceptance criteria

- **AC-1** Given `fixtures/unc/mvar/vmcm/bivariate.json` (a linear model with known covariance, seed 11, 10^6 trials), then the sample covariance is within 1 % of the known values.
- **AC-2** Given a group where one output fails in trial 5, then trial 5 is excluded for both outputs.
- **AC-3** Given the same seed twice, then the region and covariance bytes are identical.
- **AC-H1** Hypothesis: the joint sample covariance and correlation coefficients from 10^6 trials reproduce the published Monte Carlo results of the JCGM 102:2011 worked examples with vector outputs (bivariate additive model, resistance and reactance [unverified: clause numbering]), encoded from their published inputs, with agreement to the number of significant digits the guide publishes (at most one unit in the last published digit), measured with the seed recorded; falsified if any entry disagrees beyond that.
- **AC-H2** Hypothesis: the Mahalanobis coverage region achieves coverage within the exact two-sided 99 % binomial band around p = 0.95 computed by `UNC.ORS.UNC.HARNESS` with N ≥ 1000 seeded repetitions on the ORS-UNC known-truth case `mvar-datum` of `UNC.ORS.UNC.BASE` (the `fid-ors` release named in the run record), measured by the harness; falsified if the count lies outside the band.

## Design

- **Registry command(s):** none.
- **Data structures:** `VectorMcmResult { mean, cov, region: SampleRegion { r_p, p, m }, samples_ref }`.
- **Algorithm:** Cholesky of the sample covariance (`fid-num`); distances in trial order; total-order selection.
- **Errors:** `invalid_covariance` when the sample covariance is singular; `too_few_trials`.
- **Determinism:** D2.
- **Performance budget:** 10^6 trials of a 6-vector in under 1 s [unverified until measured].
- **Crate:** `fid-kunc` (module `prop::mvar::vmcm`; KNP-1 applies).

## Test fixtures

`fixtures/unc/mvar/vmcm/` (CC-BY-4.0); the ORS-UNC position case (artifact).

## Build prompt

> In crate `fid-kunc`, module `prop::mvar::vmcm`, compute from MCM block outputs of a characteristic group the joint sample mean and covariance (fixed order; exclude any trial where any output failed) and a sample-based coverage region: Mahalanobis distances via Cholesky from `fid-num`, radius = the order statistic at rank ceil(pM) in total order. Encode joint samples with `prop::mcm::samples`. Tests AC-1..AC-3. Constraints: KNP-1, CONSTITUTION 8.

## Open questions

- The region rule must be checked against JCGM 102:2011 by an expert [unverified].

## Children

_Leaf: none._
