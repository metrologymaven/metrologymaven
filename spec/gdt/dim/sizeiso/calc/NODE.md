---
id: GDT.DIM.SIZEISO.CALC
title: ISO calculated sizes (circumference, area, volume)
level: 4
parent: GDT.DIM.SIZEISO
kind: feature
status: open
phase: P1b
size: L
owners: []
claimed_by:
satisfies: [REQ-GDT.DIM.SIZEISO-004]
covers: []
depends_on: [GDT.RES.RESOLVE, KRN.NUM]
uses_artifacts: [GDT.RES.ROWISO, UNC.ORS.SIZE]
cli: []
mcp: []
journal: []
updated: 2026-11-22
---

# GDT.DIM.SIZEISO.CALC: ISO calculated sizes (circumference, area, volume)

## Purpose

Computes the ISO 14405-1 calculated sizes: a diameter derived from a measured circumference (CC), cross-sectional area (CA) or volume (CV). The final formula from a perimeter, area or volume to a diameter is elementary, but how the perimeter, area or volume is obtained from discrete points (reconstruction, filtering, which section, which length for a volume) is interpretation content of the ISO rows (D-009, D-016). This node executes those definitions.

## Scope

**In:**
- Executing the reconstruction and integration operators the ISO chain names
- Converting perimeter, area or volume to a diameter with the formula the row names
- Recording each operator's algorithm ID

**Out** (and which node owns it instead):
- Local, global and rank-order sizes (owned by sibling modules)
- Definitions as content (owned by `GDT.RES.ROWISO`)

## Requirements

- **REQ-GDT.DIM.SIZEISO.CALC-001** When the resolved ISO chain names a calculated-size operator, the system shall compute the perimeter, area or volume with the chain's reconstruction operator and convert it to a diameter with the formula the row names.
- **REQ-GDT.DIM.SIZEISO.CALC-002** If the chain names a reconstruction operator that is not implemented, then the system shall return `UNSUPPORTED` with its algorithm ID.
- **REQ-GDT.DIM.SIZEISO.CALC-003** Calculated sizes shall match every ORS-SIZE ISO calculated-size case within its stated tolerance.

## Interfaces

- **Offers:** `calculated_size(points, feature_kind, chain) -> CalcSize` to `GDT.DIM.SIZEISO.DISP`.
- **Consumes:** resolved chain (GDT.RES.RESOLVE); `fid-num` transcendentals (KRN.NUM); ISO rows and ORS-SIZE cases (read as data).

## Decomposition hints

FORMULA (perimeter, area or volume to diameter; a plain-math leaf), RECON (reconstruction and integration operators named by rows; open until rows exist).

## Open questions

- Not a leaf: reconstruction and integration rules are ISO row content (`GDT.RES.ROWISO`, P1b).

## Children

_None yet: status `open`._
