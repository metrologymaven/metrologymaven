---
id: HAL.IPP.MEAS
title: Point and scan measurement mapping, including laser line sensors
level: 3
parent: HAL.IPP
kind: module
status: open
phase: P2
size: L
owners: []
claimed_by:
satisfies: [REQ-HAL.IPP-001, REQ-HAL.IPP-004]
covers: [PW.PRB.05, PCD.LAS.01]
depends_on: [HAL.IPP.SESS, HAL.IPP.COMP, HAL.ACQ.STAMP]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# HAL.IPP.MEAS: Point and scan measurement mapping, including laser line sensors

## Purpose

Turns what the controller measures into stamped observations: single points from point-measurement moves, scan streams from scanning probes, and line profiles from laser line sensors exposed through the controller. Each point carries position, direction, tip identity and radius, and a flag saying whether the controller already compensated it.

## Scope

**In:**
- Point-measurement and scan instructions issued for the Motion Executive (M1)
- Mapping each measured point to a `TactileProbe` batch: position (m), direction, tip ID, tip radius, timestamps, compensation flag
- Mapping scan streams to batched `TactileProbe` data with sequence numbers
- Laser line sensors on a CMM exposed by the controller, mapped to `LineScan` batches with pose references (optional feature)
- Unit conversion from controller units to SI as a single journaled rounding

**Out** (and which node owns it instead):
- Compensation ownership rules (owned by `HAL.IPP.COMP`)
- Stamping and journaling (owned by `HAL.ACQ.STAMP`)
- Measurement strategy and path (owned by `CMM`)

## Requirements

- **REQ-HAL.IPP.MEAS-001** For every point the controller reports, the client shall emit a `TactileProbe` record with position, unit direction vector, tip ID, tip radius and a compensation flag taken from the session's compensation mode.
- **REQ-HAL.IPP.MEAS-002** Scan streams shall be emitted as contiguous sequence-numbered batches, and any lost points shall be recorded as gaps.
- **REQ-HAL.IPP.MEAS-003** When the controller reports values in a unit other than metres, the client shall convert them once to SI and journal the source unit and value.
- **REQ-HAL.IPP.MEAS-004** When the controller exposes a laser line sensor, the client shall emit `LineScan` batches with a pose reference to the machine position at each profile.
- **REQ-HAL.IPP.MEAS-005** Point-measurement and scan instructions shall be issued only as M1 instructions from the Motion Executive.

## Interfaces

- **Offers:** `TactileProbe`, `CncMotion` scan and `LineScan` streams into `HAL.ACQ`.
- **Consumes:** `HAL.IPP.SESS`, `HAL.IPP.COMP`, `HAL.ACQ.STAMP`.

## Decomposition hints

PTMEAS (point mapping; likely a leaf), SCAN (scan streams), LASER (laser line sensors via the controller), UNITS (controller unit handling).

## Open questions

- Line-laser streaming through I++ 3.0 is unverified on shipping servers (parent open question).

## Children

_None yet: status `open`._
