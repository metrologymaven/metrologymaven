---
id: GDT.ZONE.MATL
title: Material-condition modifiers, bonus and virtual-condition boundaries
level: 3
parent: GDT.ZONE
kind: module
status: open
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-GDT.ZONE-002, REQ-GDT.ZONE-007]
covers: [GOM.GDT.15, PCD.DIM.04]
depends_on: [GDT.ZONE.POSE, GDT.DIM.SIZEASME]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# GDT.ZONE.MATL: Material-condition modifiers, bonus and virtual-condition boundaries

## Purpose

Handles MMC and LMC on toleranced features of size: bonus tolerance from the actual size the resolved DMT row defines (for example the actual mating envelope under ASME), and the alternative surface-interpretation check against a virtual-condition boundary located in the DRF. Both are interpretation-dependent (which actual size, which boundary, which interpretation decides), so the choices come from DMT rows; this module executes them on top of `GDT.ZONE.POSE`.

## Scope

**In:**
- Bonus tolerance = |actual size − MMC (or LMC) size| as defined by the row, reported separately from stated tolerance
- Virtual-condition boundary zone (surface interpretation): containment of the toleranced surface by a boundary located in the DRF
- Axis-interpretation variant with bonus; reporting both when requested, with the row's deciding variant marked
- `local_optimum_only` propagation from the actual-size fit

**Out** (and which node owns it instead):
- Actual size computation (owned by `GDT.DIM.SIZEASME` and `GDT.DIM.SIZEISO`)
- Datum features at MMB / LMB (owned by `GDT.DRF.MOB` and `GDT.ZONE.MOBSRCH`)
- Position-specific payloads (owned by `GDT.LOC.POS`)

## Requirements

- **REQ-GDT.ZONE.MATL-001** When a toleranced feature of size carries MMC or LMC, the engine shall compute the bonus from the actual size named by the resolved row and report stated tolerance and bonus as separate fields.
- **REQ-GDT.ZONE.MATL-002** When the row's variant is surface interpretation, the engine shall evaluate containment of the toleranced points by the virtual-condition boundary and report the minimum boundary clearance over the free pose.
- **REQ-GDT.ZONE.MATL-003** If the actual-size fit carries `local_optimum_only`, then the bonus and the result shall carry the same flag.
- **REQ-GDT.ZONE.MATL-004** The engine shall reproduce every ORS-GDT position/MMC case within its stated tolerance, including cases where axis and surface interpretations disagree.

## Interfaces

- **Offers:** `solve_with_material_condition(...)` to `GDT.LOC.POS`, `GDT.FORM.STRAIGHT` (derived median line at MMC) and orientation checks at MMC.
- **Consumes:** pose solve (GDT.ZONE.POSE); actual size (GDT.DIM.SIZEASME); DMT rows.

## Decomposition hints

BONUS (bonus arithmetic given an actual size; a leaf), VCB (virtual-condition boundary containment), BOTH (dual-variant reporting).

## Open questions

- Surface versus axis precedence for ASME position is DMT content (concept doc 05 §2.5 example row, to be confirmed against the purchased text, D-016).

## Children

_None yet: status `open`._
