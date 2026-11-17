---
id: LVM.WATCH.VECTOR
title: Vector groups
level: 3
parent: LVM.WATCH
kind: module
status: open
phase: P3
size: L
owners: []
claimed_by:
satisfies: [REQ-LVM.WATCH-004]
covers: [SA.BLD.03]
depends_on: [GEO.DIST, LVM.STATION.OBS]
cli: [fid build create-vector-group, fid build update-vector-group, fid build list-vector-groups]
mcp: [build_create_vector_group, build_update_vector_group, build_list_vector_groups]
journal: [build.create_vector_group@1, build.update_vector_group@1]
updated: 2026-11-16
---

# LVM.WATCH.VECTOR: Vector groups

## Purpose

A vector group is a set of deviation vectors from measured points to nominal geometry: the classic picture of how a built structure departs from design. This node defines vector groups, computes each vector on the kernel as the deviation from the measured point to its nearest nominal surface or feature along the normal, and keeps magnification and colouring as display-only settings.

## Scope

**In:**
- Vector-group definition: source points (captured observations or a point set), nominal surfaces or features, frame, display settings
- Deviation vectors to the nearest nominal along the surface normal, signed
- Magnification and colour-by-tolerance as display settings that never change values
- Live updating when a member point is live

**Out** (and which node owns it instead):
- Closest-point projection (`GEO.DIST`)
- Live sample delivery (`LVM.WATCH.PIPE`)
- Drawing vectors (`VIZ.OVERLAY`)

## Requirements

- **REQ-LVM.WATCH.VECTOR-001** When `build.create_vector_group@1` runs, the system shall store a vector group with its source points, nominal geometry, frame and display settings.
- **REQ-LVM.WATCH.VECTOR-002** For each member point the system shall compute the signed deviation along the nominal surface normal at the closest point, through `GEO.DIST`, on the CPU kernel.
- **REQ-LVM.WATCH.VECTOR-003** Changing magnification or colouring shall not change any stored or reported deviation value.
- **REQ-LVM.WATCH.VECTOR-004** If a point's closest-point projection is ambiguous (two nominal faces within 1e-9 m of equal distance) [unverified], then the vector shall be marked ambiguous and both candidates reported.

## Interfaces

- **Offers:** vector-group commands (W2); vector data for VIZ and `LVM.RPT`.
- **Consumes:** `GEO.DIST`; `LVM.STATION.OBS` captured points; `LVM.WATCH.PIPE` for live members.

## Decomposition hints

DEF (definitions), DEV (deviation computation), LIVE (live members).

## Open questions

- Should a vector group also support deviations to nominal points (not surfaces), or is that a watch-window case?

## Children

_None yet: status `open`._
