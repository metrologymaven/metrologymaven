---
id: IND.GEAR.SIZE
title: Runout, size over balls and tooth thickness
level: 3
parent: IND.GEAR
kind: module
status: open
phase: P3
size: L
owners: []
claimed_by:
satisfies: [REQ-IND.GEAR-001, REQ-IND.GEAR-007]
covers: [PCD.IND.02]
depends_on: [IND.GEAR.DEF]
cli: [fid pack evaluate-gear-size]
mcp: [pack_evaluate_gear_size]
journal: [pack.evaluate_gear_size@1]
updated: 2026-11-16
---

# IND.GEAR.SIZE: Runout, size over balls and tooth thickness

## Purpose

Evaluations that use a ball, pin or virtual probe sitting in the tooth spaces: radial runout, dimension over balls or pins, and tooth thickness derived from flank points. They share the contact geometry of a sphere or cylinder touching two flanks, so one module owns them.

## Scope

**In:**
- Virtual ball or pin contact in each tooth space from measured flank points
- Radial runout over all measured spaces
- Dimension over balls or pins (two-ball and, for odd tooth counts, the declared method)
- Tooth thickness at a declared diameter (chordal and arc)
- `pack.evaluate_gear_size@1` (R1)

**Out** (and which node owns it instead):
- Generic shaft runout tolerances (owned by `GDT`)
- Pitch deviations (owned by `IND.GEAR.PITCH`)

## Requirements

- **REQ-IND.GEAR.SIZE-001** The size command shall be registered only through the gear pack manifest.
- **REQ-IND.GEAR.SIZE-002** When flank points of every tooth space are given, the pack shall compute the centre of a ball of declared diameter touching both flanks of each space and shall report radial runout as the range of the ball-centre radii.
- **REQ-IND.GEAR.SIZE-003** When dimension over balls or pins is requested, the pack shall compute it from the ball contacts for the declared ball diameter, and for an odd number of teeth shall apply the declared odd-tooth method.
- **REQ-IND.GEAR.SIZE-004** When tooth thickness is requested at a declared diameter, the pack shall report chordal and arc tooth thickness per tooth.
- **REQ-IND.GEAR.SIZE-005** If any tooth space lacks points on either flank, then the pack shall report runout and dimension over balls as `NOT_DECIDABLE` and name the missing spaces.

## Interfaces

- **Offers:** `pack.evaluate_gear_size@1` (R1).
- **Consumes:** `IND.GEAR.DEF` nominal flank API; KRN numeric facade.

## Decomposition hints

BALL (contact solver and runout), OVER (over balls or pins), THK (tooth thickness).

## Open questions

- Whether runout is also needed from physical ball probing (direct reading) in the first release [unverified].

## Children

_None yet: status `open`._
