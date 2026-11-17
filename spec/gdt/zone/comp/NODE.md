---
id: GDT.ZONE.COMP
title: Composite feature-control-frame tiers
level: 3
parent: GDT.ZONE
kind: module
status: open
phase: P1b
size: L
owners: []
claimed_by:
satisfies: [REQ-GDT.ZONE-004, REQ-GDT.ZONE-007]
covers: [PCD.DIM.03, PW.GDT.09]
depends_on: [GDT.ZONE.GROUP, GDT.ZONE.MATL]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# GDT.ZONE.COMP: Composite feature-control-frame tiers

## Purpose

Evaluates composite feature control frames (composite position and composite profile): the same zone problem solved once per tier with tier-specific constraints, for example the pattern-locating tier using all datum DOFs and the feature-relating tier constrained only in orientation to the referenced datums (concept doc 05 §2.5 example row). Which DOFs each tier keeps is DMT content.

## Scope

**In:**
- Per-tier problem construction from the row's tier constraints
- One value per tier, with tier index and constraint set recorded
- Multi-segment (stacked single-segment) frames as independent tiers

**Out** (and which node owns it instead):
- Tier constraint definitions (owned by DMT rows in `GDT.RES`)
- Profile-specific composite payloads (owned by `GDT.PROF.SURF`)
- Position payloads (owned by `GDT.LOC.POS`)

## Requirements

- **REQ-GDT.ZONE.COMP-001** When a composite frame is evaluated, the engine shall solve each tier separately with the DOF constraints the resolved row assigns to that tier and report one value per tier.
- **REQ-GDT.ZONE.COMP-002** Each tier result shall record its tier index, the DOFs it held fixed, and the pose found.
- **REQ-GDT.ZONE.COMP-003** If the row assigns no constraint set to a tier present in the frame, then the engine shall return `UNSUPPORTED` for the whole frame.
- **REQ-GDT.ZONE.COMP-004** The engine shall reproduce every ORS-GDT composite case within its stated tolerance.

## Interfaces

- **Offers:** `solve_composite(frame, ...) -> [TierSolution]`.
- **Consumes:** group and material-condition solves (siblings); DMT rows.

## Decomposition hints

TIERS (tier problem construction), REPORT (per-tier result fields).

## Open questions

- None beyond D-009 / D-016 row authorship.

## Children

_None yet: status `open`._
