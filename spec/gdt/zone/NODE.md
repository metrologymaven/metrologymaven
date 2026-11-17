---
id: GDT.ZONE
title: Tolerance-zone engine, modifiers and composite frames
level: 2
parent: GDT
kind: subsystem
status: open
phase: P1a
size: XL
owners: []
claimed_by:
satisfies: [REQ-GDT-004, REQ-GDT-005, REQ-GDT-007]
covers: [GOM.GDT.15, PCD.DIM.03, PCD.DIM.04, PW.GDT.09]
depends_on: [GDT.RES, GDT.DRF, KRN]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# GDT.ZONE: Tolerance-zone engine, modifiers and composite frames

## Purpose

The shared core of every geometric tolerance: one small constrained optimizer that finds the pose of a tolerance zone, within the free degrees of freedom of the DRF and the allowed datum mobility, that minimizes zone usage for the toleranced feature or feature set (or proves infeasibility). Material modifiers (MMC / LMC and bonus tolerance), patterns and simultaneous requirements (one shared pose), and composite frames (the same problem solved per tier with different constraints) all live here, so each tolerance family (FORM, LOC, PROF) only supplies its zone geometry and toleranced points.

## Scope

**In:**
- Zone types: two parallel planes / lines, cylinder, circle (annulus), sphere, uniform and non-uniform surface or line offset zones; boundary (virtual-condition) zones
- Zone-pose solve within DRF free DOFs and the mobility envelope; infeasibility proof
- Material-condition modifiers MMC / LMC on toleranced features: bonus tolerance from the actual size, and virtual-condition boundary containment
- Patterns and simultaneous requirements: a set of features sharing one zone-framework pose
- Composite feature control frames: per-tier solve (pattern-locating and feature-relating tiers) with tier-specific constraints
- Zone usage value and its sign convention; certificate (optimality or feasibility) passed through from KRN solvers

**Out** (and which node owns it instead):
- Zone geometry per symbol and the toleranced-points extraction (owned by `GDT.FORM`, `GDT.LOC`, `GDT.PROF`)
- DRF construction and the mobility envelope (owned by `GDT.DRF`)
- Optimization primitives and minimum-zone solvers (owned by `KRN`)
- Which interpretation applies (owned by `GDT.RES`)

## Requirements

- **REQ-GDT.ZONE-001** Given a zone type, toleranced points, a DRF and its mobility envelope, the engine shall return the zone usage minimized over the free DOFs and the envelope, the optimal pose, and a certificate, or a typed infeasibility result.
- **REQ-GDT.ZONE-002** When a toleranced feature of size carries an MMC or LMC modifier, the engine shall compute bonus tolerance from the actual size defined by the resolved DMT row and report stated and bonus tolerance as separate fields.
- **REQ-GDT.ZONE-003** When features form a pattern or a simultaneous-requirement group, the engine shall solve all their zones with one shared pose, and the ResultRecord of each feature shall reference the group ID.
- **REQ-GDT.ZONE-004** When a composite frame is evaluated, the engine shall solve each tier separately with the constraints the DMT row assigns to that tier and report one value per tier.
- **REQ-GDT.ZONE-005** When the DRF has datum mobility, the engine shall search the mobility envelope as part of the pose, and the reported transform used shall lie within the envelope (checked by `GDT.DRF`).
- **REQ-GDT.ZONE-006** If the underlying solver flags a local optimum only (non-convex case), then the engine shall carry `local_optimum_only` into the ResultRecord rather than report the value as global.
- **REQ-GDT.ZONE-007** The engine shall reproduce every ORS-GDT position / MMC, composite and simultaneous-requirement case within its stated tolerance, including cases where axis and surface interpretations disagree.

## Interfaces

- **Offers:** an internal kernel API (no registry command of its own; the checks in `GDT.FORM`, `GDT.LOC` and `GDT.PROF` call it) taking (zone spec, points, DRF, envelope, group) and returning (usage, pose, certificate, tier values). Algorithm IDs `gps.zone.<method>@x.y.z`.
- **Consumes:** constrained solvers and minimum-zone primitives (KRN); DRF and mobility envelope (GDT.DRF); operator chain (GDT.RES).

## Decomposition hints

POSE (core solve and certificates), MATL (MMC / LMC, bonus, virtual condition), GROUP (patterns and simultaneous requirements), COMP (composite tiers), MOB (search over mobility, P1b).

## Open questions

- In P1a, form zones may be referenced to least-squares rather than minimum-zone until KRN's MZ is green (declared conservative upper bound per concept doc 05 §13); confirm the declaration format.

## Children

_None yet: status `open`._
