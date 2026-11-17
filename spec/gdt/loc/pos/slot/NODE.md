---
id: GDT.LOC.POS.SLOT
title: Position of slots, tabs and width features
level: 4
parent: GDT.LOC.POS
kind: feature
status: open
phase: P1b
size: L
owners: []
claimed_by:
satisfies: [REQ-GDT.LOC.POS-001, REQ-GDT.LOC.POS-004]
covers: [GOM.GDT.08]
depends_on: [GDT.LOC.POS.RFS, GDT.LOC.DERIV.TYPES, GDT.ZONE.MATL]
uses_artifacts: [GDT.RES.ROWASME, GDT.RES.ROWISO, UNC.ORS.GDT]
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# GDT.LOC.POS.SLOT: Position of slots, tabs and width features

## Purpose

Position of features of size that are not round: width features (slots, tabs) located by a median plane, elongated holes with bidirectional or rectangular zones, and their boundary (surface) interpretation at MMC. The derived element, the zone shape per direction and the boundary shape are interpretation content of the DMT rows (D-009, D-016); P1a scope is holes and pins only, so this node follows in P1b.

## Scope

**In:**
- Width-feature position from a median plane or median points (from `GDT.LOC.DERIV.MSURF`)
- Bidirectional and rectangular zones where rows define them
- Boundary interpretation for non-round features via `GDT.ZONE.MATL`

**Out** (and which node owns it instead):
- Round holes and pins (owned by `GDT.LOC.POS.RFS`, `GDT.LOC.POS.MMC`)
- The command (owned by `GDT.LOC.POS.CMD`, which dispatches to the non-round evaluator this node registers, so the P1a command does not depend on this P1b node)

## Requirements

- **REQ-GDT.LOC.POS.SLOT-001** The system shall evaluate position of width features and elongated holes with the derived element, zone shapes and boundary the resolved row names, and report per-direction values where the zone is bidirectional.
- **REQ-GDT.LOC.POS.SLOT-002** If the row names a zone or boundary shape `GDT.ZONE.ZTYPE` or `GDT.ZONE.MATL` cannot represent, then the system shall return `UNSUPPORTED` with the shape.
- **REQ-GDT.LOC.POS.SLOT-003** Values shall match every ORS-GDT non-round position case within its stated tolerance.

## Interfaces

- **Offers:** `position_width(...)` and `position_elongated(...)` to `GDT.LOC.POS.CMD`.
- **Consumes:** derived median surfaces and points (GDT.LOC.DERIV); RFS machinery (GDT.LOC.POS.RFS); boundaries (GDT.ZONE.MATL); rows and ORS-GDT cases (read as data).

## Decomposition hints

WIDTH (median-plane position, two-planes zone; likely a leaf once MSURF rows exist), BIDIR (bidirectional and rectangular zones), BOUND (non-round boundaries at MMC).

## Open questions

- Not a leaf: derived elements and boundaries for non-round features are DMT content.

## Children

_None yet: status `open`._
