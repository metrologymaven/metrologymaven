---
id: UNC.PROP.MVAR
title: Multivariate propagation of feature parameter vectors
level: 3
parent: UNC.PROP
kind: module
status: open
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-UNC.PROP-006]
covers: []
depends_on: [UNC.PROP.LPU, UNC.PROP.MCM]
cli: []
mcp: []
journal: []
updated: 2026-11-16
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

## Interfaces

- **Offers:** `FeatureCovariance`; `propagate_vector_lpu`, `propagate_vector_mcm`; joint covariance for characteristic groups.
- **Consumes:** LPU and MCM engines; fit Jacobians from KRN.

## Decomposition hints

PARAM (parameterisations per feature type), VLPU, VMCM, REGION (coverage regions).

## Open questions

- Parameterisations for cones and tori with near-degenerate apex or radius need a stable choice; coordinate with KRN fit outputs.

## Children

_None yet: status `open`._
