---
id: UNC.PROP.MVAR.VLPU
title: Multivariate LPU and joint covariance of characteristic groups
level: 4
parent: UNC.PROP.MVAR
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-UNC.PROP.MVAR-002, REQ-UNC.PROP.MVAR-003]
covers: []
depends_on: [UNC.PROP.MVAR.PARAM, UNC.PROP.LPU.COMB, KRN.NUM.RED, KRN.NUM.FN]
uses_artifacts: [UNC.ORS.UNC]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# UNC.PROP.MVAR.VLPU: Multivariate LPU and joint covariance of characteristic groups

## Purpose

Propagates vector-valued quantities by the Jacobian sandwich (JCGM 102:2011): the output covariance is J V J^T, where J stacks the sensitivities of all outputs in a characteristic group to all model inputs. Characteristics that share a datum or feature therefore get their correlation from the shared inputs instead of being treated as independent. It also gives the ellipsoidal coverage region for a vector output.

## Scope

**In:**
- `propagate_vector_lpu(model, inputs, outputs, profile) -> VectorLpuResult { y, cov, regions }`
- Characteristic groups: the outputs of several characteristics evaluated jointly over one model, with the joint covariance and correlation matrix
- Ellipsoidal region: {y : (y - y0)^T cov^-1 (y - y0) <= chi2_q(p, dim)}, with the quantile computed by a `fid-num`-only inverse of the regularised gamma function
- Comparison mode for tests: the same group with shared inputs duplicated as independent (the independence assumption)

**Out:**
- Scalar LPU (owned by `UNC.PROP.LPU.COMB`, whose sensitivities this node stacks)
- Sample-based regions (owned by `UNC.PROP.MVAR.VMCM`)

## Requirements

- **REQ-UNC.PROP.MVAR.VLPU-001** The engine shall compute the joint output covariance of a characteristic group as J V J^T over all model inputs with fixed-order reductions.
- **REQ-UNC.PROP.MVAR.VLPU-002** When two outputs depend on the same model input, the engine shall include their cross-covariance from that input in the joint covariance.
- **REQ-UNC.PROP.MVAR.VLPU-003** On the ORS-UNC position case with a correlated datum, the standard uncertainty of the position deviation shall agree with the MCM result within 5 % [unverified], and the independence-assumption comparison shall deviate by more than that.
- **REQ-UNC.PROP.MVAR.VLPU-004** The engine shall return the ellipsoidal coverage region with its quantile, dimension and coverage probability.

## Acceptance criteria

- **AC-1** Given `fixtures/unc/mvar/vlpu/two_holes_datum.json` (two hole positions relative to one fitted datum plane, closed-form joint covariance), then the joint covariance matches to relative 1e-12 and the cross term is non-zero.
- **AC-2** Given the ORS-UNC position case (artifact from `UNC.ORS.UNC`) and its MCM reference, then AC criterion of REQ-003 holds.
- **AC-3** Given dim = 2 and p = 0.95, then the quantile equals 5.991464547 to 1e-9.
- **AC-4** Given AC-1 on three targets, then results are byte-identical.
- **AC-H1** Hypothesis: on the ORS-UNC known-truth case `mvar-datum` of `UNC.ORS.UNC.BASE` (the `fid-ors` release named in the run record), the multivariate LPU standard uncertainty of the position deviation agrees with the `UNC.PROP.MVAR.VMCM` reference within 5 % [unverified] while the independence assumption deviates by more than 5 %, measured on the same seed and model digest; falsified if either part fails.
- **AC-H2** Hypothesis: the Jacobian sandwich reproduces the published output covariance and correlation coefficients of the JCGM 102:2011 worked examples with vector outputs (bivariate additive model, resistance and reactance [unverified: clause numbering]), encoded from their published inputs, with agreement to the number of significant digits the guide publishes (at most one unit in the last published digit), measured by comparing the joint covariance with the published values; falsified if any entry disagrees beyond that.
- **AC-H3** Hypothesis: the ellipsoidal 95 % coverage region achieves coverage within the exact two-sided 99 % binomial band around p = 0.95 computed by `UNC.ORS.UNC.HARNESS` with N ≥ 1000 seeded repetitions on the ORS-UNC known-truth case `mvar-datum` of `UNC.ORS.UNC.BASE` (the `fid-ors` release named in the run record), measured by the harness rule for multivariate cases; falsified if the count lies outside the band.

## Design

- **Registry command(s):** none; used by `UNC.PROP.SELECT` and GDT-facing evaluation.
- **Data structures:** `CharGroup { outputs: Vec<OutputRef> }`; `VectorLpuResult { y, cov: SymMat, corr, region: Ellipsoid { center, cov, q, dim, p } }`.
- **Algorithm:** stacked sensitivities from `propagate_lpu` per output; dense products in fixed order; quantile by Newton on the regularised gamma with a bracketed fallback.
- **Errors:** `invalid_covariance` (from PARAM), engine errors passed through.
- **Determinism:** D0.
- **Performance budget:** group of 20 outputs over 30 000 inputs in under 300 ms [unverified until measured].
- **Crate:** `fid-kunc` (module `prop::mvar::vlpu`; KNP-1 applies).

## Test fixtures

`fixtures/unc/mvar/vlpu/` hand-built (CC-BY-4.0); the ORS-UNC position case from `UNC.ORS.UNC` (artifact).

## Build prompt

> In crate `fid-kunc`, module `prop::mvar::vlpu`, implement `propagate_vector_lpu(model, inputs, outputs, profile)`. Stack the sensitivities of each output from `prop::lpu::propagate_lpu` into J, compute the joint covariance J V J^T and correlation with fixed-order reductions, and return an ellipsoidal coverage region using a chi-square quantile implemented with `fid-num` operations only (Newton on the regularised gamma with a bracketed fallback). Provide a test-only independence comparison. Tests AC-1..AC-4, including the ORS-UNC position case within 5 % of MCM. Constraints: KNP-1, CONSTITUTION 8.

## Open questions

- The 5 % agreement target is from the parent and unmeasured [unverified].

## Children

_Leaf: none._
