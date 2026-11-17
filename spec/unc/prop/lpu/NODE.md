---
id: UNC.PROP.LPU
title: Linear propagation with analytic sensitivities
level: 3
parent: UNC.PROP
kind: module
status: open
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-UNC.PROP-001]
covers: []
depends_on: [UNC.MODEL.GRAPH, UNC.MODEL.DIST, KRN]
cli: [fid unc propagate-lpu]
mcp: [unc_propagate_lpu]
journal: [unc.propagate_lpu@1]
updated: 2026-11-16
---

# UNC.PROP.LPU: Linear propagation with analytic sensitivities

## Purpose

First-order law of propagation of uncertainty (JCGM 100:2008) over an Uncertainty Model: sensitivity coefficients are obtained analytically through the kernel pipeline by forward-mode differentiation, and by implicit differentiation at optimizer solutions (fits) using the solver's active set, never by finite differences. Combines them with input standard uncertainties and correlations into the combined standard uncertainty and an expanded U.

## Scope

**In:**
- Forward-mode differentiation contract for kernel operators (each operator in the manifest exposes a Jacobian or tangent function)
- Implicit differentiation of least-squares and constrained fits at the solution (using the solver certificate's active set)
- Combination with the input covariance (from distributions and correlations)
- Coverage factor choice (fixed k or from effective degrees of freedom, declared per profile)
- `unc.propagate_lpu@1` (R1)

**Out:**
- Nonlinearity assessment (owned by `UNC.PROP.NLIN`)
- Vector-valued outputs with full covariance (owned by `UNC.PROP.MVAR`)
- Statement commit (owned by `UNC.PROP.SELECT`)

## Requirements

- **REQ-UNC.PROP.LPU-001** The engine shall obtain every sensitivity coefficient from operator tangent functions or implicit differentiation, and if an operator in the model provides neither, then the engine shall fail with `no_derivative` naming the operator rather than fall back to finite differences.
- **REQ-UNC.PROP.LPU-002** For least-squares fits, the sensitivities of fit parameters to input points shall be computed by implicit differentiation at the converged solution and shall agree with a 50-digit finite-difference oracle to a relative 1e-8 on the RDL tier G cases [unverified].
- **REQ-UNC.PROP.LPU-003** The engine shall combine sensitivities with the full input covariance, including declared correlations, in a fixed-order reduction.
- **REQ-UNC.PROP.LPU-004** If the model contains a non-smooth operator at the evaluation point (for example an active-set change within the input uncertainty), then the engine shall flag `lpu_not_applicable` in its output.
- **REQ-UNC.PROP.LPU-005** The engine shall be deterministic class D0 for the same model digest and inputs.

## Interfaces

- **Offers:** `propagate_lpu(model, inputs) -> LpuResult { y, u_c, sensitivities, covariance_used, flags }`; command `unc.propagate_lpu@1`.
- **Consumes:** validated models (UNC.MODEL.GRAPH); distributions (UNC.MODEL.DIST); operator tangents and solver certificates (KRN).

## Decomposition hints

TANGENT (operator tangent contract and coverage of the P1a operator set), IMPLICIT (implicit differentiation at fit solutions), COMBINE (covariance combination and coverage factor), CMD.

## Open questions

- Whether effective degrees of freedom (Welch–Satterthwaite) are offered or only fixed k is a profile choice for reviewers [unverified].
- Tangent coverage depends on KRN exposing tangents for every P1a operator; this must be raised with KRN owners.

## Children

_None yet: status `open`._
