---
id: ALN.BFIT.ICP
title: Surface best-fit iteration loop, metrics and command
level: 3
parent: ALN.BFIT
kind: module
status: split
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-ALN.BFIT-001, REQ-ALN.BFIT-002, REQ-ALN.BFIT-006, REQ-ALN.BFIT-008, REQ-ALN.BFIT-009]
covers: [GOM.ALN.02, PW.ALN.02]
depends_on: [ALN.BFIT.LOCAL, ALN.BFIT.CONSTR, ALN.BFIT.STOP, ALN.MODEL.ELEM, ALN.QA.REPORT, GEO.DIST, GEO.INDEX, KRN.SOLV, KRN.OUT]
cli: [fid align run-bestfit]
mcp: [align_run_bestfit]
journal: [align.run_bestfit@1]
updated: 2026-11-17
---

# ALN.BFIT.ICP: Surface best-fit iteration loop, metrics and command

## Purpose

The iterative closest point (ICP) family solver that refines a pose by minimising deviations between measured data and a reference mesh or CAD surface: correspondence search, the point-to-point and point-to-plane objectives, the linearised pose step, in-fit outlier handling, and the `align.run_bestfit@1` command plus the pure R1 entry point that SCAN, GDT and CMM call. Scope, locks, weights and stopping rules plug in from sibling leaves.

## Scope

**In:**
- Correspondence search against mesh or CAD (closest point via GEO), with a deterministic tie-break
- Metrics: point-to-point and point-to-plane (normal-projected) objectives; the metric recorded in parameters
- Linearised pose step through KRN.SOLV (orthogonal factorisation), composed with the lock parameterisation from `ALN.BFIT.CONSTR`
- In-fit outlier rule hook (KRN.OUT policies) with the rule recorded
- `align.run_bestfit@1` (W2) and `bestfit(measured, reference, params) -> BestfitOutput` (R1)
- Global best-fit (the default scope)

**Out** (and which node owns it instead):
- Local scope resolution (`ALN.BFIT.LOCAL`); DoF locks and weights (`ALN.BFIT.CONSTR`); stopping and non-convergence (`ALN.BFIT.STOP`)
- Feature-to-nominal best-fit (`ALN.BFIT.FEAT`)
- Accuracy and timing corpus (`ALN.BFIT.BENCH`)
- Distance, projection and spatial index (`GEO.DIST`, `GEO.INDEX`); outlier policies (`KRN.OUT`)

## Requirements

- **REQ-ALN.BFIT.ICP-001** When `align.run_bestfit` runs with scope `global`, the system shall use every measured point that has a correspondence on the reference.
- **REQ-ALN.BFIT.ICP-002** The run shall use the metric named in the `metric` parameter (`point_to_point` or `point_to_plane`), and the alignment parameters shall record it.
- **REQ-ALN.BFIT.ICP-003** When several reference points are equally close to a measured point, the correspondence shall be chosen by a documented deterministic rule (lowest triangle or face stable ID).
- **REQ-ALN.BFIT.ICP-004** Given noise-free synthetic data with a known transform and a start inside the convergence basin, the fit shall recover translation within 1e-9 m and rotation within 1e-9 rad [unverified].
- **REQ-ALN.BFIT.ICP-005** The fit shall run in the CPU kernel under KNP-1, single-threaded or with a fixed-order parallel reduction, and the registry shall declare class D0 for a fixed thread-independent schedule or D3 otherwise.
- **REQ-ALN.BFIT.ICP-006** Global best-fit of 1 million points to a 1 million-triangle reference from a prealigned start shall finish in under 5 s on reference hardware [unverified].
- **REQ-ALN.BFIT.ICP-007** When an outlier rule is set, the fit shall apply it each iteration and record the rule's algorithm ID and the final excluded-index-set digest.

## Interfaces

- **Offers:** `align.run_bestfit@1` (W2; params `measured`, `reference`, `scope`, `metric`, `dof_locks`, `weights`, `outlier_rule`, `stop_criteria`, `parent`); R1 `bestfit(...)` for SCAN, GDT and CMM.
- **Consumes:** `ALN.BFIT.LOCAL`, `ALN.BFIT.CONSTR`, `ALN.BFIT.STOP`; GEO closest point and normals; KRN.SOLV step solve; KRN.OUT policies; `ALN.MODEL.ELEM`; `ALN.QA.REPORT`.

## Decomposition hints

CORR (correspondence search and tie-break), METRIC (point-to-point and point-to-plane residuals and Jacobians), STEP (linearised step and pose update), CMD (command, R1 entry, element wrapping). Thread-parallel correspondence with fixed-order reduction is a separate later task.

## Open questions

- Split follows the hints: CORR, METRIC, LOOP (hint STEP folded into LOOP, since the step is one QR call), CMD. The R1 `bestfit` entry lives in LOOP (kernel layer); scope resolution needs the document and is called by CMD.
- Phase conflict: GEO.DIST (mesh and CAD projection) and KRN.OUT.POL are P1b while this node is P1a; see the children's open questions. A phase decision should settle whether surface best-fit is P1b.
- A min-max (Chebyshev) surface best-fit stays deferred until GDT.PROF is expanded (parent question).

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| ALN.BFIT.ICP.CORR | Surface best-fit correspondence search and tie-break (leaf) | 001 003 005 006 | none |
| ALN.BFIT.ICP.METRIC | Point-to-point and point-to-plane residuals and Jacobians (leaf) | 002 005 | none |
| ALN.BFIT.ICP.LOOP | Surface best-fit iteration loop and R1 entry point (leaf) | 001 004 005 006 007 | none |
| ALN.BFIT.ICP.CMD | Surface best-fit command and alignment wrapping (leaf) | 001 002 007 | GOM.ALN.02, PW.ALN.02 |
