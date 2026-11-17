---
id: GDT.LOC.ORIENT
title: Parallelism, perpendicularity and angularity
level: 3
parent: GDT.LOC
kind: module
status: open
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-GDT.LOC-001, REQ-GDT.LOC-006]
covers: [GOM.GDT.05, GOM.GDT.06, GOM.GDT.07, PW.GDT.03]
depends_on: [GDT.LOC.DERIV, GDT.LOC.LRES, GDT.ZONE.POSE, GDT.DRF.BUILD]
cli: [fid check eval-parallelism, fid check eval-perpendicularity, fid check eval-angularity]
mcp: [check_eval_parallelism, check_eval_perpendicularity, check_eval_angularity]
journal: [check.eval_parallelism@1, check.eval_perpendicularity@1, check.eval_angularity@1]
updated: 2026-11-16
---

# GDT.LOC.ORIENT: Parallelism, perpendicularity and angularity

## Purpose

Evaluates the three orientation tolerances: the toleranced surface, line or axis must lie in a zone whose orientation is fixed by the DRF (parallel, perpendicular or at a basic angle) while its location floats. All three are the same zone problem with orientation-only constraints and a different basic angle, so they form one module. The P1a subset is orientation of planes and axes to planar datums.

## Scope

**In:**
- Toleranced elements: planar surface, line elements, axis (from `GDT.LOC.DERIV`)
- Zones: two parallel planes, cylinder (axis with diameter symbol), two parallel lines
- Basic angle: 0 (parallelism), 90 degrees (perpendicularity), stated basic angle (angularity)
- Orientation-only constraint set from the DRF; optional MMC on axes via `GDT.ZONE.MATL`
- Three commands sharing one implementation

**Out** (and which node owns it instead):
- Pose solve (owned by `GDT.ZONE.POSE`)
- Derivation (owned by `GDT.LOC.DERIV`)
- Profile used as an orientation control (owned by `GDT.PROF`)

## Requirements

- **REQ-GDT.LOC.ORIENT-001** The system shall evaluate parallelism, perpendicularity and angularity of a planar, linear or axial toleranced element to a DRF with the zone's orientation fixed at the basic angle and its location free, and report the value and the toleranced element used.
- **REQ-GDT.LOC.ORIENT-002** The three commands shall share one evaluation path that differs only in the basic angle, verified by a test that angularity at 0 and 90 degrees equals parallelism and perpendicularity bit for bit.
- **REQ-GDT.LOC.ORIENT-003** If the DRF leaves the orientation of the zone undetermined (for example a single axis datum for a plane toleranced in rotation about it), then the system shall use the free rotation as the row directs or return `UNSUPPORTED`.
- **REQ-GDT.LOC.ORIENT-004** Values shall match every ORS-GDT orientation case within the P1a subset within its stated tolerance before P1a exit.

## Interfaces

- **Offers:** three `check.eval_*` commands (W2, dry-run R1).
- **Consumes:** derived elements, payload (siblings); pose solve (GDT.ZONE.POSE); DRF (GDT.DRF.BUILD).

## Decomposition hints

PLANE (planar surfaces to planar datums; first leaf), AXIS (axes, including MMC), LINE (line elements), CMDS.

## Open questions

- Tangent-plane modifier and line-element orientation defaults are DMT content.

## Children

_None yet: status `open`._
