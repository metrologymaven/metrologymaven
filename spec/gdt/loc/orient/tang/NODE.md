---
id: GDT.LOC.ORIENT.TANG
title: Tangent-plane modifier for orientation
level: 4
parent: GDT.LOC.ORIENT
kind: feature
status: open
phase: P1b
size: L
owners: []
claimed_by:
needs_expert: gdt
satisfies: [REQ-GDT.LOC.ORIENT-001, REQ-GDT.LOC.ORIENT-004]
covers: [PW.GDT.03]
depends_on: [GDT.LOC.ORIENT.ZONE, KRN.ENV, GDT.RES.RESOLVE]
uses_artifacts: [GDT.RES.ROWASME, GDT.RES.ROWISO, UNC.ORS.GDT]
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# GDT.LOC.ORIENT.TANG: Tangent-plane modifier for orientation

## Purpose

With the tangent-plane modifier, an orientation tolerance applies to a plane associated to the high points of the surface rather than to every surface point. Which association forms that plane (outer tangent under which objective, with which orientation constraints) and how its orientation is then evaluated against the zone are interpretation content of the DMT rows (D-009, D-016). This node executes those rules and hands the resulting plane to the shared orientation evaluation.

## Scope

**In:**
- Associating the tangent plane with the fit the chain names
- Evaluating the tangent plane's orientation through `GDT.LOC.ORIENT.ZONE` (for example over its contact points or its extent, as the row says)
- Recording the association's algorithm ID and contact set

**Out** (and which node owns it instead):
- Plain orientation evaluation (owned by `GDT.LOC.ORIENT.ZONE`)
- Commands (owned by `GDT.LOC.ORIENT.CMDS`, which call this when the chain carries the modifier)
- Tangent fits (owned by `KRN.ENV`)

## Requirements

- **REQ-GDT.LOC.ORIENT.TANG-001** When the chain carries the tangent-plane modifier, the system shall associate the tangent plane with the fit the row names and evaluate its orientation as the row defines, recording the algorithm IDs and contact set.
- **REQ-GDT.LOC.ORIENT.TANG-002** If the row names an association or evaluation that is not implemented, then the system shall return `UNSUPPORTED` with the algorithm ID.
- **REQ-GDT.LOC.ORIENT.TANG-003** Values shall match every ORS-GDT tangent-plane orientation case within its stated tolerance.

## Interfaces

- **Offers:** `tangent_plane_orientation(points, chain, drf) -> OrientValue` to `GDT.LOC.ORIENT.CMDS`.
- **Consumes:** orientation evaluation (GDT.LOC.ORIENT.ZONE); tangent fits (KRN.ENV); chain (GDT.RES.RESOLVE); rows and ORS-GDT cases (read as data).

## Decomposition hints

One leaf once a signed-off row names the association and evaluation.

## Open questions

- Not a leaf: tangent-plane semantics are DMT content (D-009, D-016). Placed in P1b because the P1a inventory in `GDT.RES.ROWASME` does not list the modifier.

## Children

_None yet: status `open`._
