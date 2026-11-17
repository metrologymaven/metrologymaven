---
id: GDT.LOC.POS
title: Position of features and patterns
level: 3
parent: GDT.LOC
kind: module
status: open
phase: P1a
size: XL
owners: []
claimed_by:
satisfies: [REQ-GDT.LOC-002, REQ-GDT.LOC-003, REQ-GDT.LOC-006]
covers: [GOM.GDT.08, PW.GDT.04]
depends_on: [GDT.LOC.DERIV, GDT.LOC.LRES, GDT.ZONE.MATL, GDT.ZONE.GROUP, GDT.DRF.BUILD]
cli: [fid check eval-position]
mcp: [check_eval_position]
journal: [check.eval_position@1]
updated: 2026-11-16
---

# GDT.LOC.POS: Position of features and patterns

## Purpose

Evaluates position of single features of size and patterns, RFS, MMC and LMC: the centre of the P1a product. Under ASME the default is the surface interpretation (virtual-condition boundary), with the axis interpretation as a named variant; when both run, the row designates which one decides. The zone math is in `GDT.ZONE`; this module assembles the problem per callout, applies the variant, and fills the position payload.

## Scope

**In:**
- Single feature position (holes, pins, slots later) to a DRF, RFS, MMC and LMC
- Pattern position as a group (via `GDT.ZONE.GROUP`)
- Surface and axis interpretation variants (via `GDT.ZONE.MATL`), reporting both when requested with the deciding variant marked
- Position value conventions (diametral zone value, per-axis deviations) in `LocPayload`

**Out** (and which node owns it instead):
- Composite position tiers (owned by `GDT.ZONE.COMP`)
- Bonus and boundaries (owned by `GDT.ZONE.MATL`)
- Datum mobility (owned by `GDT.DRF.MOB`, `GDT.ZONE.MOBSRCH`)

## Requirements

- **REQ-GDT.LOC.POS-001** The system shall evaluate position of a single feature of size and of a pattern, RFS, MMC and LMC, and report the position value, per-axis deviations in the DRF, and stated and bonus tolerance in `LocPayload`.
- **REQ-GDT.LOC.POS-002** The system shall evaluate the interpretation variant selected by the resolver; when both variants are requested it shall report both and mark the row's deciding variant.
- **REQ-GDT.LOC.POS-003** A pattern shall be evaluated with one shared pose and each member's result shall reference the group ID.
- **REQ-GDT.LOC.POS-004** The P1a subset (holes and pins, RFS and MMC, no mobility) shall match every ORS-GDT position case within its stated tolerance before P1a exit, including cases where axis and surface interpretations disagree.

## Interfaces

- **Offers:** `check.eval_position@1` (W2, dry-run R1).
- **Consumes:** zone engine (GDT.ZONE.MATL, GDT.ZONE.GROUP); derived axes (GDT.LOC.DERIV); payload (GDT.LOC.LRES); DRF (GDT.DRF.BUILD).

## Decomposition hints

RFS (single feature RFS; first leaf), MMC (bonus and virtual condition), PATTERN, VARIANT (dual reporting), CMD.

## Open questions

- Surface versus axis precedence and the position-value convention for each zone shape are DMT content (D-009, D-016).

## Children

_None yet: status `open`._
