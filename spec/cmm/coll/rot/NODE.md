---
id: CMM.COLL.ROT
title: Rotary tables in the machine model
level: 3
parent: CMM.COLL
kind: module
status: open
phase: P3
size: L
owners: []
claimed_by:
satisfies: [REQ-CMM.COLL-001, REQ-CMM.COLL-003]
covers: []
depends_on: [CMM.COLL.MACH]
cli: [fid cmm set-machine-rotary]
mcp: [cmm_set_machine_rotary]
journal: [cmm.set_machine_rotary@1]
updated: 2026-11-16
---

# CMM.COLL.ROT: Rotary tables in the machine model

## Purpose

Adds rotary tables to the machine model: axis pose (centre and direction, from a rotary-axis qualification), angular range and index step, and the transform of the part and fixture bodies mounted on it for a given table angle. Planning, simulation and collision checks need it to place the part correctly at every rotary-index step.

## Scope

**In:**
- Rotary axis definition: axis point and direction in the machine frame, range, index step or continuous, mounted bodies
- Validation of rotary targets against range and step
- Transform of mounted fixture and part bodies at a table angle
- Rotary axis digest as part of the machine model digest
- `cmm.set_machine_rotary@1` (W2)

**Out** (and which node owns it instead):
- Measuring the rotary axis (qualification run) (owned by `CMM.PROBE.QUAL` for the reduction pattern and `CMM.EXEC` for motion; a leaf here may need a reduction of its own)
- Rotary-index moves in plans (owned by `CMM.PATH.PLAN`)
- Driving the table (owned by `HAL`)

## Requirements

- **REQ-CMM.COLL.ROT-001** The system shall model a rotary table with an axis point and unit direction in the machine frame, an angular range in radians, and either an index step or continuous positioning.
- **REQ-CMM.COLL.ROT-002** If a rotary target lies outside the range, or off the index grid by more than 1e-9 rad for an indexing table, then the system shall reject it with `ROTARY_TARGET_INVALID`.
- **REQ-CMM.COLL.ROT-003** For a given table angle, the system shall return the pose of every body mounted on the table by rotating it about the axis.
- **REQ-CMM.COLL.ROT-004** Any change to the rotary axis or mounted bodies shall change the machine model digest.

## Interfaces

- **Offers:** `cmm.set_machine_rotary@1` (W2); RotaryAxis type; mounted-body transform used by `CMM.COLL.CHECK` and `CMM.PATH.PLAN`.
- **Consumes:** `CMM.COLL.MACH` model and digest.
- **Journal:** `cmm.set_machine_rotary@1`.

## Decomposition hints

AXIS (definition and validation), MOUNT (mounted bodies and transforms), CAL (axis calibration reduction, if not owned elsewhere).

## Open questions

- **Phase:** tagged P3 because the concept docs place rotary and articulating kinematics beyond 3-axis Cartesian in P3; proposed for ratification in D-018 (option C would keep it at P2).
- Who reduces the rotary-axis qualification hits (this node or `CMM.PROBE.QUAL`) is open.

## Children

_None yet: status `open`._
