---
id: UNC.XFM.TCOV
title: Transform parameter covariance
level: 3
parent: UNC.XFM
kind: module
status: open
phase: P1b
size: L
owners: []
claimed_by:
satisfies: [REQ-UNC.XFM-001, REQ-UNC.XFM-005, REQ-UNC.XFM-006]
covers: [SA.UNC.03]
depends_on: [UNC.PROP.LPU, UNC.PROP.MCM, UNC.INST.WEIGHT, ALN]
cli: [fid unc compute-transform-uncertainty]
mcp: [unc_compute_transform_uncertainty]
journal: [unc.compute_transform_uncertainty@1]
updated: 2026-11-16
---

# UNC.XFM.TCOV: Transform parameter covariance

## Purpose

Computes the 6x6 (rigid) or 7x7 (with scale) covariance of a transform's parameters from the alignment or point-pair fit that produced it, in one declared parameterisation (a local perturbation in the Lie algebra of SE(3) or Sim(3) around the estimate). The LPU route uses the alignment solver's normal equations weighted by instrument covariance; the MCM route re-runs the alignment on perturbed observations. When an input set has no instrument model, the transform uncertainty is UEC-0 and no covariance is reported.

## Scope

**In:**
- Parameterisation `se3-left@1` (rotation vector then translation; scale as log-scale for Sim(3)), stated in every output
- LPU transform covariance from the solver Jacobian at the solution and observation weights
- MCM transform covariance by re-running the ALN solver per trial
- UEC-0 handling when observations lack a bound model
- `unc.compute_transform_uncertainty@1` (R1); commit as W2 (journal `unc.compute_transform_uncertainty@1`) through the same command with `--commit`, or a separate commit command decided at leaf level

**Out:**
- The alignment solvers (owned by `ALN`)
- Applying covariance to transformed entities (owned by `UNC.XFM.APPLY`)
- Chains (owned by `UNC.XFM.CHAIN`)

## Requirements

- **REQ-UNC.XFM.TCOV-001** When an alignment or point-pair transform is computed with bound instrument models for all its observations, the system shall produce a 6x6 (or 7x7 with scale) covariance in parameterisation `se3-left@1` and state the parameterisation in the output.
- **REQ-UNC.XFM.TCOV-002** For least-squares alignments, the LPU covariance shall equal (JᵀWJ)⁻¹ scaled as declared, with J the solver Jacobian at the solution and W the observation weights from `UNC.INST.WEIGHT`.
- **REQ-UNC.XFM.TCOV-003** On the ORS alignment cases, every standard deviation from LPU shall agree with an MCM evaluation within 10 % [unverified].
- **REQ-UNC.XFM.TCOV-004** If any observation has no bound instrument model, then the transform uncertainty shall be UEC-0 with reason `instrument_missing` and no covariance shall be reported.
- **REQ-UNC.XFM.TCOV-005** For alignments whose solver reports a degenerate (rank-deficient) configuration, the system shall report the unconstrained directions instead of a covariance.

## Interfaces

- **Offers:** `TransformCovariance { params: Vec<f64>, cov: SymMatrix, parameterisation, class, flags }`; command `unc.compute_transform_uncertainty@1`.
- **Consumes:** ALN solver outputs (Jacobian, observations, active constraints); weights (UNC.INST.WEIGHT); engines (UNC.PROP.LPU, UNC.PROP.MCM).

## Decomposition hints

PARAM (parameterisation and conversions to other conventions for reporting), LSQ (LPU from normal equations), MC (MCM route), CMD.

## Open questions

- Constrained, RPS and 3-2-1 alignments are not least squares; their LPU route needs ALN to expose a local linearisation. Raise with ALN owners.
- Whether reports should also offer Euler-angle standard deviations for users used to them is a display choice for RPT.

## Children

_None yet: status `open`._
