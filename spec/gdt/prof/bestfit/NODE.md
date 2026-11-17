---
id: GDT.PROF.BESTFIT
title: Profile best-fit options
level: 3
parent: GDT.PROF
kind: module
status: split
phase: P1b
size: L
owners: []
claimed_by:
satisfies: [REQ-GDT.PROF-003]
covers: [PCD.DIM.06]
depends_on: [GDT.ZONE.POSE, GDT.ZONE.MOBSRCH, GDT.DRF.BUILD, ALN.BFIT.CONSTR, KRN.SOLV.NLLS]
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# GDT.PROF.BESTFIT: Profile best-fit options

## Purpose

Implements the best-fit options incumbents offer on profile checks (none, free best fit within the zone, constrained to named DRF DOFs, and with datum mobility through `GDT.ZONE.MOBSRCH`), and records the chosen option and resulting transform. Which options are legitimate for a given callout is an interpretation question; unconstrained best fit on a datum-referenced profile can contradict the drawing, so each option maps to a DMT row variant and non-default choices are flagged.

## Scope

**In:**
- Options: `none`, `free`, `constrained{dofs}`, `mobility`
- Objective: minimum zone usage (default) or least squares where the variant says so
- Recording option, objective, DOFs freed and transform
- Flag `bestfit_beyond_drf` when the option frees DOFs the DRF constrains

**Out** (and which node owns it instead):
- Pose solver (owned by `GDT.ZONE.POSE`)
- Mobility search (owned by `GDT.ZONE.MOBSRCH`)
- Alignment best fits (owned by `ALN.BFIT`)
- Building the profile problem and calling the solver (owned by `GDT.PROF.SURF.PROB` and `GDT.PROF.LINE.EVAL`, which call this module)

## Requirements

- **REQ-GDT.PROF.BESTFIT-001** When a profile check has a best-fit option, the system shall evaluate with the selected option (none, free, constrained to named DRF DOFs, or mobility) and record the option and the resulting transform in the ResultRecord.
- **REQ-GDT.PROF.BESTFIT-002** If the option frees DOFs that the characteristic's DRF constrains, then the result shall carry `bestfit_beyond_drf` and list the freed DOFs.
- **REQ-GDT.PROF.BESTFIT-003** Results shall match every ORS-GDT profile best-fit case within its stated tolerance.

## Interfaces

- **Offers:** `pose_dofs(option, baseline, drf, chain)` and `BestFitRecord` (OPTS); `lsq_pose(...)` (LSQ); both called inside `check.eval_surface_profile@1` and `check.eval_line_profile@1`.
- **Consumes:** pose solve (GDT.ZONE.POSE); mobility search (GDT.ZONE.MOBSRCH); DRF (GDT.DRF.BUILD); DOF-lock parameterisation (ALN.BFIT.CONSTR); nonlinear least squares (KRN.SOLV.NLLS).

## Decomposition hints

OPTS (option model and flags; a leaf), OBJ (objective variants).

## Open questions

- Whether `free` best fit on a datum-referenced profile should be allowed at all, or only flagged; an expert call through the DMT. Resolved mechanically: a row variant may forbid an option with limitation code `bestfit_disallowed:<option>` and `GDT.PROF.BESTFIT.OPTS` then refuses it; otherwise the option runs and is flagged. Which rows forbid which options stays expert content.
- The earlier dependency on `GDT.PROF.SURF` is dropped: SURF and LINE call this module, so the reverse edge would form a cycle.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| GDT.PROF.BESTFIT.OPTS | Best-fit option model, DOF selection and flags (leaf) | 001 002 003 | PCD.DIM.06 |
| GDT.PROF.BESTFIT.LSQ | Least-squares objective variant (leaf) | 001 003 | PCD.DIM.06 |
