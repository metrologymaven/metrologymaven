---
id: GDT.ZONE.MOBSRCH
title: Zone solve within datum mobility
level: 3
parent: GDT.ZONE
kind: module
status: open
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
updated: 2026-11-16
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

- **Offers:** mobility-aware variant of `solve_zone`.
- **Consumes:** envelope (GDT.DRF.MOB); membership (GDT.DRF.CHECK); pose solve (GDT.ZONE.POSE).

## Decomposition hints

CONSTR (envelope constraints into the LP/SOCP), JOINT (joint solve), VERIFY (post-check).

## Open questions

- Non-convex envelopes (see `GDT.DRF.MOB`) would make this search non-convex; the `local_optimum_only` convention then applies.

## Children

_None yet: status `open`._
