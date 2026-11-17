---
id: CMM.PATH.CLEAR
title: Clearance surfaces, move points and transits
level: 3
parent: CMM.PATH
kind: module
status: open
phase: P2
size: L
owners: []
claimed_by:
satisfies: [REQ-CMM.PATH-003, REQ-CMM.PATH-004]
covers: [PCD.PTH.01]
depends_on: [GEO]
cli: [fid cmm set-clearance, fid cmm *-move-point]
mcp: [cmm_set_clearance, cmm_*_move_point]
journal: [cmm.set_clearance@1, cmm.*_move_point@1]
updated: 2026-11-16
---

# CMM.PATH.CLEAR: Clearance surfaces, move points and transits

## Purpose

Owns the safe-travel geometry of a program: clearance planes and clearance cylinders (per program and per feature), user move points, and the transit generator that routes every move between features through the active clearance surface unless a move point overrides it. Editing a move point recompiles only the affected plan segment and is one journaled command.

## Scope

**In:**
- `ClearanceSurface` (plane with offset, cylinder with axis and radius), program-level and feature-level assignment
- Move points: insert, edit, delete; ordered within a transit
- Transit generation: retract to clearance surface, travel on or beyond it, descend to the next approach point
- Segment-level recompilation after a move-point change
- `cmm.set_clearance@1`, `cmm.insert_move_point@1`, `cmm.edit_move_point@1`, `cmm.delete_move_point@1` (W2)

**Out** (and which node owns it instead):
- The plan model and full compilation (owned by `CMM.PATH.PLAN`)
- Collision checks of transits (owned by `CMM.COLL`)
- Automatic avoidance when a transit collides (owned by `CMM.PATH.AUTO`)

## Requirements

- **REQ-CMM.PATH.CLEAR-001** The system shall support clearance planes (normal and offset) and clearance cylinders (axis and radius) at program level and per feature, with the feature-level surface taking precedence.
- **REQ-CMM.PATH.CLEAR-002** When generating a transit between two features, the system shall retract to the active clearance surface, travel on it, and descend to the next approach point, unless one or more user move points are defined for that transit, in which case it shall pass through them in order.
- **REQ-CMM.PATH.CLEAR-003** When a move point is inserted, edited or deleted, the system shall recompile only the transit segment that contains it and journal the change as one command.
- **REQ-CMM.PATH.CLEAR-004** If a clearance surface definition is degenerate (zero normal, non-positive cylinder radius, non-finite values), then the system shall reject it with `CLEARANCE_INVALID`.
- **REQ-CMM.PATH.CLEAR-005** Transit generation shall be deterministic (class D0).

## Interfaces

- **Offers:** `cmm.set_clearance@1`, `cmm.insert_move_point@1`, `cmm.edit_move_point@1`, `cmm.delete_move_point@1` (W2); transit generator used by `CMM.PATH.PLAN`; ClearanceSurface, MovePoint types.
- **Consumes:** `GEO` frames and projections.
- **Journal:** `cmm.set_clearance@1`, `cmm.*_move_point@1`.

## Decomposition hints

SURF (clearance surfaces), MOVEPT (move-point commands), TRANSIT (transit generator and segment recompilation).

## Open questions

- Transits around a clearance cylinder (arc versus polyline) need a choice at leaf level; polyline with a maximum chord error is the deterministic default.

## Children

_None yet: status `open`._
