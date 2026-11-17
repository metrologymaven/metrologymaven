---
id: GDT.ZONE.MOBSRCH
title: Zone solve within datum mobility
level: 3
parent: GDT.ZONE
kind: module
status: split
phase: P1b
size: L
owners: []
claimed_by:
satisfies: [REQ-GDT.ZONE-005, REQ-GDT.ZONE-007]
covers: []
depends_on: [GDT.ZONE.POSE, GDT.DRF.MOB, GDT.DRF.CHECK]
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# GDT.ZONE.MOBSRCH: Zone solve within datum mobility

## Purpose

Extends the pose solve so the DRF itself may move within its mobility envelope (datum features at MMB or LMB) while the zone is placed, and reports the transform used so `GDT.DRF.CHECK` can confirm it lies inside the envelope. It is separate from POSE because it adds envelope constraints and a second set of pose variables, and it is P1b on the scope ladder.

## Scope

**In:**
- Adding envelope constraints from `GDT.DRF.MOB` to the zone problem
- Joint search over zone pose and DRF transform
- Reporting the transform used and the envelope reference

**Out** (and which node owns it instead):
- Envelope construction (owned by `GDT.DRF.MOB`)
- Membership check and result fields (owned by `GDT.DRF.CHECK`)

## Requirements

- **REQ-GDT.ZONE.MOBSRCH-001** When the DRF has datum mobility, the engine shall minimize usage jointly over the zone pose and the DRF transforms in the mobility envelope.
- **REQ-GDT.ZONE.MOBSRCH-002** The reported transform used shall satisfy the envelope membership check of `GDT.DRF.CHECK`; if it does not, the engine shall return `envelope_violation` rather than a value.
- **REQ-GDT.ZONE.MOBSRCH-003** The engine shall reproduce every ORS-GDT mobility case within its stated tolerance.

## Interfaces

- **Offers:** mobility-aware variant `solve_zone_mobile(...)` (JOINT) with a mandatory post-check (VERIFY).
- **Consumes:** envelope (GDT.DRF.MOB); membership (GDT.DRF.CHECK); pose solve (GDT.ZONE.POSE).

## Decomposition hints

CONSTR (envelope constraints into the LP/SOCP), JOINT (joint solve), VERIFY (post-check).

## Open questions

- Non-convex envelopes (see `GDT.DRF.MOB`) would make this search non-convex; the `local_optimum_only` convention then applies.
- `GDT.DRF.MOB` is still open, so the `MobilityEnvelope` canonical form (linear rows and cones over the six DRF pose parameters in the order tx, ty, tz, rx, ry, rz) is assumed here from its Scope; re-check JOINT when MOB becomes a leaf.

## Split rationale

Hints CONSTR and JOINT are merged into JOINT: the envelope rows are only meaningful inside the joint problem, and the DRF-constrained DOFs that become mobile are disjoint from the zone's own free DOFs, so one pose vector with extra rows covers both. VERIFY stays separate because it must run on every result, including results from the pattern and material-condition solvers.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| GDT.ZONE.MOBSRCH.JOINT | Joint zone and datum-mobility solve (leaf) | 001 003 | none |
| GDT.ZONE.MOBSRCH.VERIFY | Envelope post-check of the transform used (leaf) | 002 | none |
