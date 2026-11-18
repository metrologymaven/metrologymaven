---
id: UNC.PROP.MVAR
title: Multivariate propagation of feature parameter vectors
level: 3
parent: UNC.PROP
kind: module
status: split
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-UNC.PROP-006]
covers: []
depends_on: [UNC.PROP.LPU, UNC.PROP.MCM]
uses_artifacts: [UNC.ORS.UNC]
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# UNC.PROP.MVAR: Multivariate propagation of feature parameter vectors

## Purpose

Propagates vector-valued quantities with their full covariance (JCGM 102:2011): a fitted feature's parameter vector (position, orientation, size) and its covariance flow into every downstream characteristic, so that correlated datum and feature uncertainty is not treated as independent. Both the LPU form (Jacobian sandwich) and the MCM form (joint samples) are covered.

## Scope

**In:**
- `FeatureCovariance` type: parameter vector in a declared parameterisation per feature type, covariance matrix, units per component
- Multivariate LPU: output covariance from the Jacobian and input covariance
- Multivariate MCM: joint sample sets and sample covariance
- Coverage regions for vector outputs where needed (ellipsoidal from LPU, from samples for MCM)
- Correlation bookkeeping when several characteristics share a datum feature

**Out:**
- Scalar LPU and MCM engines (owned by `UNC.PROP.LPU` and `UNC.PROP.MCM`)
- Transform (rigid-body) covariance on the manifold (owned by `UNC.XFM`)

## Requirements

- **REQ-UNC.PROP.MVAR-001** For every fitted feature in a model, the engine shall carry its parameter vector with a full covariance matrix in a parameterisation declared per feature type.
- **REQ-UNC.PROP.MVAR-002** When two characteristics depend on the same feature, the engine shall propagate their joint covariance and shall not treat their contributions from that feature as independent.
- **REQ-UNC.PROP.MVAR-003** On the ORS-UNC position case with a correlated datum, the multivariate LPU standard uncertainty shall agree with MCM within 5 % [unverified], while the independence assumption shall show a larger deviation.
- **REQ-UNC.PROP.MVAR-004** If a declared covariance matrix is not symmetric positive semi-definite within tolerance, then the engine shall fail with `invalid_covariance`.

## Research hypotheses

Measurable hypotheses against public reference data (D-036 rule 4). Thresholds marked [unverified] are targets, not results. The children named in each item must state it as `AC-H` acceptance criteria.

- **H-1** On ORS-UNC `mvar-datum`, multivariate LPU agrees with multivariate MCM within 5 % [unverified] and the independence assumption does not (`UNC.PROP.MVAR.VLPU`, `UNC.PROP.MVAR.VMCM`).
- **H-2** Both forms reproduce the published covariances and correlations of the JCGM 102:2011 worked examples (`UNC.PROP.MVAR.VLPU`, `UNC.PROP.MVAR.VMCM`).
- **H-3** Ellipsoidal and Mahalanobis coverage regions cover at the nominal rate within the exact binomial band (`UNC.PROP.MVAR.VLPU`, `UNC.PROP.MVAR.VMCM`).
- **H-4** The feature parameterisation stays full rank and well conditioned on tier G partial-coverage geometry down to a stated coverage angle (`UNC.PROP.MVAR.PARAM`).

## Interfaces

- **Offers:** `FeatureCovariance`; `propagate_vector_lpu`, `propagate_vector_mcm`; joint covariance for characteristic groups.
- **Consumes:** LPU and MCM engines; fit Jacobians from KRN.

## Decomposition hints

PARAM (parameterisations per feature type), VLPU, VMCM, REGION (coverage regions).

## Open questions

- **Applied (cones and tori):** `UNC.PROP.MVAR.PARAM` uses the kernel fit parameterisations of `KRN.LSQ.TRAIT` (axis by two tangent-plane parameters, cone by reference point at the foot of the centroid, radius there and half-angle, with no apex parameter). Tori are not in `fcov-param@1` and return `parameterisation_missing` until a later version.
- Coverage regions (hint REGION) are folded into VLPU (ellipsoidal) and VMCM (sample-based), because each is a small function of its engine's output.
- The ORS-UNC position case is read as an artifact from `UNC.ORS.UNC` (still open).

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| UNC.PROP.MVAR.PARAM | Feature covariance type and parameterisations (leaf) | 001 004 | none |
| UNC.PROP.MVAR.VLPU | Multivariate LPU and joint covariance of characteristic groups (leaf) | 002 003 | none |
| UNC.PROP.MVAR.VMCM | Joint Monte Carlo samples and sample covariance (leaf) | 002 003 | none |
