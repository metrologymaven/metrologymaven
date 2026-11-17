---
id: GDT.DIM.SIZEISO.LOCAL
title: ISO local sizes (two-point and spherical)
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
depends_on: [GDT.RES.RESOLVE, KRN.LSQ]
uses_artifacts: [GDT.RES.ROWISO, UNC.ORS.SIZE]
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# GDT.DIM.SIZEISO.LOCAL: ISO local sizes (two-point and spherical)

## Purpose

Computes the ISO 14405-1 local sizes: two-point size (LP) and spherical size (LS), per edition. The definition of a local size (which cross-sections, which reference feature fixes the pairing direction, how a spherical local size is formed) is interpretation content of the ISO rows in `GDT.RES.ROWISO` (D-009, D-016). This node executes those definitions; its output also feeds the rank-order statistics.

## Scope

**In:**
- Executing the local-size operators the resolved ISO chain names, per edition
- Output `LocalSizes` (shared type with `GDT.DIM.SIZEASME.LOCAL`)
- Reuse of an ASME pairing operator where an ISO row names the same operator

**Out** (and which node owns it instead):
- Rank-order statistics (owned by `GDT.DIM.SIZEISO.RANK`)
- Global and calculated sizes (owned by `GDT.DIM.SIZEISO.GLOBAL`, `GDT.DIM.SIZEISO.CALC`)
- Definitions as content (owned by `GDT.RES.ROWISO`)

## Requirements

- **REQ-GDT.DIM.SIZEISO.LOCAL-001** When the resolved ISO chain names a local-size operator, the system shall compute every local size the operator defines and return them with their point indices.
- **REQ-GDT.DIM.SIZEISO.LOCAL-002** If the chain names a local-size operator that is not implemented, then the system shall return `UNSUPPORTED` with the operator's algorithm ID.
- **REQ-GDT.DIM.SIZEISO.LOCAL-003** Local sizes shall match every ORS-SIZE ISO local-size case within its stated tolerance, per edition.

## Interfaces

- **Offers:** `iso_local_sizes(points, feature_kind, chain) -> LocalSizes` to `GDT.DIM.SIZEISO.DISP` (and, through it, `GDT.DIM.SIZEISO.RANK`).
- **Consumes:** resolved chain (GDT.RES.RESOLVE); reference fits (KRN.LSQ); ISO rows (`GDT.RES.ROWISO`, read as data); ORS-SIZE cases (`UNC.ORS.SIZE`, read as data).

## Decomposition hints

LP (two-point), LS (spherical), one leaf each once signed-off rows name their operators.

## Open questions

- Not a leaf: the local-size definitions are ISO row content awaiting `GDT.RES.ROWISO` (P1b, D-009; texts per D-016). ISO 14405-1:2025 reportedly changed scope relative to 2016 [unverified]; edition differences must come from the purchased texts.

## Children

_None yet: status `open`._
