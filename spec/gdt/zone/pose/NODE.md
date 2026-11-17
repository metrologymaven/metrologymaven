---
id: GDT.ZONE.POSE
title: Zone-pose solve and certificates
level: 3
parent: GDT.ZONE
kind: module
status: open
phase: P1a
size: XL
owners: []
claimed_by:
satisfies: [REQ-GDT.ZONE-001, REQ-GDT.ZONE-006, REQ-GDT.ZONE-007]
covers: []
depends_on: [GDT.ZONE.ZTYPE, GDT.DRF.BUILD, KRN.SOLV, KRN.MZ]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# GDT.ZONE.POSE: Zone-pose solve and certificates

## Purpose

The core optimizer: find the pose of a zone, within the free DOFs of the DRF, that minimizes zone usage for the toleranced points, or prove the stated tolerance infeasible. It linearizes the zone-shape usage functions into LP or SOCP problems for the KRN solvers, recomputes the final value from the active set, and passes the certificate (and any `local_optimum_only` flag) into the result. Matl, group, composite and mobility children add constraints to this one problem.

## Scope

**In:**
- Problem formulation over the DRF's free pose parameters for each `ZoneShape`
- Calls to KRN solvers (sequential LP/SOCP) with fixed iteration limits
- Final value recomputed from the active set; certificate pass-through
- Infeasibility result with the violating points
- `local_optimum_only` propagation
- Algorithm IDs `gps.zone.<method>@x.y.z`

**Out** (and which node owns it instead):
- Zone shapes (owned by `GDT.ZONE.ZTYPE`)
- Bonus and boundaries (owned by `GDT.ZONE.MATL`)
- Shared poses across features (owned by `GDT.ZONE.GROUP`)
- Tiered solves (owned by `GDT.ZONE.COMP`)
- Mobility search (owned by `GDT.ZONE.MOBSRCH`)
- Solvers (owned by `KRN.SOLV`, `KRN.MZ`)

## Requirements

- **REQ-GDT.ZONE.POSE-001** Given a zone shape, toleranced points, a DRF and a list of free DOFs, the engine shall return the minimum usage over those DOFs, the optimal pose, and the solver certificate, or a typed infeasibility result listing the points that cannot be contained at the stated size.
- **REQ-GDT.ZONE.POSE-002** When the DRF leaves no free DOF, the engine shall evaluate usage at the DRF pose without calling a solver.
- **REQ-GDT.ZONE.POSE-003** The reported value shall be recomputed from the active set at the returned pose using `GDT.ZONE.ZTYPE`, not taken from the solver's objective.
- **REQ-GDT.ZONE.POSE-004** If the solver flags a local optimum only, then the engine shall return `local_optimum_only` and never label the value global.
- **REQ-GDT.ZONE.POSE-005** The engine shall reproduce every ORS-GDT orientation and RFS position case within its stated tolerance.

## Interfaces

- **Offers:** `solve_zone(shape, points, drf, free_dofs, extra_constraints) -> ZoneSolution` used by FORM, LOC, PROF and by the sibling modules.
- **Consumes:** solvers and certificates (KRN.SOLV, KRN.MZ); DRF (GDT.DRF.BUILD); shapes (GDT.ZONE.ZTYPE).

## Decomposition hints

FIXED (no free DOF; a leaf), LIN (linearization per shape), SOLVE (solver loop and active-set recompute), INFEAS (infeasibility certificates).

## Open questions

- In P1a, form zones may be referenced to least squares until KRN minimum zone is green (declared conservative upper bound); the flag is owned by `GDT.FORM.FCORE`, and this module must accept a least-squares reference pose as an input.

## Children

_None yet: status `open`._
