---
id: GDT.FORM.STRAIGHT
title: Straightness of line elements and of a derived median line
level: 3
parent: GDT.FORM
kind: module
status: open
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-GDT.FORM-001, REQ-GDT.FORM-005]
covers: [GOM.GDT.02, PW.GDT.02]
depends_on: [GDT.FORM.FCORE, GDT.ZONE.MATL, GDT.LOC.DERIV]
cli: [fid check eval-straightness]
mcp: [check_eval_straightness]
journal: [check.eval_straightness@1]
updated: 2026-11-16
---

# GDT.FORM.STRAIGHT: Straightness of line elements and of a derived median line

## Purpose

Evaluates straightness in its two forms: of surface line elements (each line element in a stated direction and plane; a 2D minimum-zone line problem per element) and of the derived median line of a cylindrical feature of size (a 3D problem with a cylindrical zone, optionally at MMC with bonus through `GDT.ZONE.MATL`). The two share a command but differ in extraction and zone shape; the derived case depends on interpretation-heavy derivation rules.

## Scope

**In:**
- Line-element straightness: extraction of line elements per the chain (direction, plane, spacing), per-element 2D form value, worst element
- Derived-median-line straightness: median line from `GDT.LOC.DERIV`, cylindrical zone, MMC bonus via `GDT.ZONE.MATL`
- `check.eval_straightness@1` dispatching between the two by the characteristic's FCF

**Out** (and which node owns it instead):
- Derived feature extraction (owned by `GDT.LOC.DERIV`)
- Bonus (owned by `GDT.ZONE.MATL`)
- Form policy (owned by `GDT.FORM.FCORE`)

## Requirements

- **REQ-GDT.FORM.STRAIGHT-001** For line-element straightness, the command shall extract line elements as the chain defines, evaluate each through the form core with element `Line2d`, and report the maximum element value and every element's value.
- **REQ-GDT.FORM.STRAIGHT-002** For derived-median-line straightness, the command shall use the median line produced by the reduction operator named in the chain and report the diameter of the smallest cylindrical zone containing it.
- **REQ-GDT.FORM.STRAIGHT-003** When the derived case carries MMC, the command shall report stated and bonus tolerance from `GDT.ZONE.MATL`.
- **REQ-GDT.FORM.STRAIGHT-004** Values shall match the synthetic fixtures and, for line elements, the TraCIM Chebyshev 2D line test within its MPE [unverified].

## Interfaces

- **Offers:** `check.eval_straightness@1` (W2, dry-run R1).
- **Consumes:** form core; derived median line (GDT.LOC.DERIV); bonus (GDT.ZONE.MATL); cylindrical zone (GDT.ZONE.ZTYPE via POSE).

## Decomposition hints

LINEEL (line-element straightness; a leaf once line-element extraction defaults exist), DERIVED (derived median line with optional MMC).

## Open questions

- Line-element extraction defaults (spacing, which planes) and median-line derivation are DMT content (D-009, D-016), so this node is not a leaf yet.

## Children

_None yet: status `open`._
