---
id: ALN.BFIT
title: Global, local and constrained best-fit
level: 2
parent: ALN
kind: subsystem
status: split
phase: P1a
size: XL
owners: []
claimed_by:
satisfies: [REQ-ALN-003]
covers: [GOM.ALN.02, GOM.ALN.03, GOM.ALN.10, PCD.ALN.02, PW.ALN.02, PW.ALN.07, SA.XFM.04]
depends_on: [ALN.MODEL, ALN.QA, GEO, KRN]
cli: [fid align run-bestfit*]
mcp: [align_run_bestfit*]
journal: [align.run_bestfit*]
updated: 2026-11-16
---

# ALN.BFIT: Global, local and constrained best-fit

## Purpose

Refines a pose by minimising deviations between measured data and a reference (CAD, mesh or nominal features), over the whole part or selected regions, with locked degrees of freedom and weights. This is the ICP family plus feature-to-nominal best-fit. It is a separate unit because it is the single iterative surface solver every other domain reuses (SCAN, GDT profile best-fit options, CMM best-fit alignments).

## Scope

**In:**
- Global best-fit of measured data to a reference
- Local best-fit restricted to selected regions or elements
- Metrics: point-to-point and point-to-plane (normal-projected); further metrics as later additions
- Degree-of-freedom locks per axis of a named frame (translation and rotation separately)
- Per-region and per-point weights
- Feature-based best-fit of measured features to their nominals (PC-DMIS-style best-fit alignments)
- Outlier rejection inside the fit, with the rejection rule recorded
- Convergence criteria, iteration limits and their documentation

**Out** (and which node owns it instead):
- Coarse initial pose (`ALN.PRE`)
- 3-2-1, RPS and sheet-metal iterative methods (`ALN.RPS`)
- Datum-reference-frame alignment (`ALN.DATUM`)
- Closed-form point-pair transforms with scale (`ALN.XFM`)
- Residual statistics and conditioning warnings (`ALN.QA`)
- Distance and nearest-neighbour queries (`GEO`); least-squares primitives (`KRN`)
- Profile tolerance evaluation that uses a best-fit option (`GDT`)

## Requirements

- **REQ-ALN.BFIT-001** The system shall provide global best-fit and local best-fit, where local best-fit uses only the selected regions or elements.
- **REQ-ALN.BFIT-002** The user shall select the metric (point-to-point or point-to-plane) per run, and the chosen metric shall be recorded in the alignment's parameters.
- **REQ-ALN.BFIT-003** When degrees of freedom are locked, the resulting transform shall change the locked components by no more than 1e-12 m or 1e-12 rad relative to the start pose.
- **REQ-ALN.BFIT-004** When per-region or per-point weights are supplied, the solver shall minimise the weighted objective and record the weights' digest.
- **REQ-ALN.BFIT-005** The solver shall stop on documented criteria (maximum iterations, transform increment and objective change), all user-settable with documented defaults, and shall report which criterion stopped it.
- **REQ-ALN.BFIT-006** Given noise-free synthetic data with a known transform and a start within the convergence basin, best-fit shall recover translation within 1e-9 m and rotation within 1e-9 rad [unverified].
- **REQ-ALN.BFIT-007** If the solver does not converge within the iteration limit, then the system shall still create the alignment marked `not_converged` and shall block release of dependent results.
- **REQ-ALN.BFIT-008** Best-fit shall be computed only by the CPU kernel under KNP-1 and declare determinism class D0 or D3 in the registry.
- **REQ-ALN.BFIT-009** Global best-fit of 1 million points to a 1 million-triangle reference from a prealigned start shall finish in under 5 s on reference hardware [unverified].

## Interfaces

- **Offers:** `align.run_bestfit@1` (W2) with parameters `scope` (global or local selection), `metric`, `dof_locks`, `weights`, `outlier_rule`, `stop_criteria`; a pure solver entry point that SCAN, GDT and CMM can call as R1 compute.
- **Consumes:** GEO distance and projection queries; KRN least-squares and optimisation primitives; `ALN.MODEL` element; `ALN.QA` report type.

## Decomposition hints

CORE (iteration loop and metrics), LOCAL (region selection), CONSTR (DoF locks and weights), FEAT (feature-to-nominal best-fit), STOP (convergence and non-convergence), BENCH (reference corpus and accuracy numbers).

## Split rationale

The split puts the **two solver loops** (surface ICP and feature-to-nominal) in their own children, and pulls out everything both loops share as small, independently testable leaves: **scope** (LOCAL), **locks and weights** (CONSTR), **stopping** (STOP) and the **accuracy and timing corpus** (BENCH). The loops stay open because correspondence search, metrics and per-feature deviation definitions each need their own leaves.

Alternatives considered:
- **One child per metric** (point-to-point, point-to-plane). Rejected: metrics share the loop, scope, locks and stopping; they become leaves under ICP.
- **Locks inside ICP.** Rejected: FEAT uses the same parameterisation, and the 1e-12 lock guarantee is best tested on its own.
- **Feature best-fit under ALN.DATUM.** Rejected: DATUM applies features in priority order with DoF accounting; FEAT is one simultaneous least-squares objective (PC-DMIS best-fit alignments).

## Open questions

- Should a min-max (Chebyshev) surface best-fit be offered alongside least squares? It overlaps GDT profile evaluation; defer until GDT's profile node is expanded.

- Global and local best-fit stay P1a as the parent states, although surface best-fit to CAD mainly serves P1b scan-to-CAD.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| ALN.BFIT.ICP | Surface best-fit iteration loop, metrics and command | 001 002 006 008 009 | GOM.ALN.02, PW.ALN.02 |
| ALN.BFIT.LOCAL | Best-fit scope resolution for local best-fit (leaf) | 001 | GOM.ALN.03 |
| ALN.BFIT.CONSTR | Degree-of-freedom locks and weights for best-fit (leaf) | 003 004 | GOM.ALN.10, PW.ALN.07, SA.XFM.04 |
| ALN.BFIT.FEAT | Feature-to-nominal best-fit alignment | 001 008 | PCD.ALN.02 |
| ALN.BFIT.STOP | Best-fit stopping criteria and non-convergence handling (leaf) | 005 007 | none |
| ALN.BFIT.BENCH | Best-fit accuracy and timing corpus (leaf) | 006 009 | none |
