---
id: UNC.PROP.LPU
title: Linear propagation with analytic sensitivities
level: 3
parent: UNC.PROP
kind: module
status: split
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
updated: 2026-11-17
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

## Research hypotheses

Measurable hypotheses against public reference data (D-036 rule 4). Thresholds marked [unverified] are targets, not results. The children named in each item must state it as `AC-H` acceptance criteria.

- **H-1** Fit sensitivities by implicit differentiation agree with 50-digit references on RDL tier G least-squares families, reported per family and coverage angle (`UNC.PROP.LPU.IMPL`); active-set sensitivities do the same on minimum-zone constructions (`UNC.PROP.LPU.ASET`, blocked on `UNC.RDL.GEN.MZEQ` [gap]).
- **H-2** The combination reproduces JCGM 100:2008 Annex H budgets, including correlations and Welch-Satterthwaite degrees of freedom, and its intervals cover at the nominal rate on ORS-UNC `lin-gauss` (`UNC.PROP.LPU.COMB`, `UNC.PROP.LPU.CMD`).
- **H-3** Kink hints from active sets, constructions and formulas flag every case where LPU fails validation against MCM (`UNC.PROP.LPU.ASET`, `UNC.PROP.LPU.TCONS`, `UNC.PROP.LPU.TFORM`, `UNC.PROP.LPU.COMB`).
- **H-4** The tangent harness tolerance separates correct providers from faulty ones (`UNC.PROP.LPU.TAN`).

## Interfaces

- **Offers:** `propagate_lpu(model, inputs) -> LpuResult { y, u_c, sensitivities, covariance_used, flags }`; command `unc.propagate_lpu@1`.
- **Consumes:** validated models (UNC.MODEL.GRAPH); distributions (UNC.MODEL.DIST); operator tangents and solver certificates (KRN).

## Decomposition hints

TANGENT (operator tangent contract and coverage of the P1a operator set), IMPLICIT (implicit differentiation at fit solutions), COMBINE (covariance combination and coverage factor), CMD.

## Open questions

- **Applied (Welch-Satterthwaite):** `UNC.PROP.LPU.COMB` offers both under the versioned profile `lpu-profile@1`: fixed k = 2 by default, and k from effective degrees of freedom when every type-A input declares its degrees of freedom. The default is for review by an uncertainty expert [unverified].
- **Applied (tangent coverage):** tangents are supplied through one contract and table (`UNC.PROP.LPU.TAN`) in `fid-kunc`, which may depend on KRN (both L0). A CI coverage report lists every kernel-manifest operator without a tangent; such operators make LPU fail with `no_derivative`, and `UNC.PROP.SELECT` then needs MCM. KRN owners need not change their fits.
- **Cross-domain (open):** if GDT registers characteristic-evaluation operators (distance, angle, size, form value) in the kernel manifest, their tangents must be supplied through the same contract. `UNC.PROP.LPU.TCONS` covers KRN constructions and evaluation operators that are in the kernel; GDT owners should confirm where evaluation operators live.
- **D-027:** formula results start at UEC-0 (`model_missing`) until a model is bound; LPU can then propagate through formulas using `UNC.PROP.LPU.TFORM`. Stated in that child.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| UNC.PROP.LPU.TAN | Operator tangent contract, table and coverage report (leaf) | 001 | none |
| UNC.PROP.LPU.TCONS | Tangents for kernel constructions and evaluation operators | 001 | none |
| UNC.PROP.LPU.TFORM | Tangents of kernel formula IR and formula UEC start (leaf) | 001 004 | none |
| UNC.PROP.LPU.IMPL | Implicit differentiation at least-squares fit solutions (leaf) | 001 002 | none |
| UNC.PROP.LPU.ASET | Active-set sensitivities and non-smoothness flag for certified fits (leaf) | 001 004 | none |
| UNC.PROP.LPU.COMB | Graph accumulation, covariance combination and coverage factor (leaf) | 003 005 | none |
| UNC.PROP.LPU.CMD | The unc.propagate_lpu@1 command (leaf) | 005 | none |
