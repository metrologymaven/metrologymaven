---
id: IND.GEAR.DEF
title: Gear definition and nominal flanks
level: 3
parent: IND.GEAR
kind: module
status: open
phase: P3
size: L
owners: []
claimed_by:
satisfies: [REQ-IND.GEAR-001, REQ-IND.GEAR-003]
covers: [PCD.IND.02]
depends_on: [IND.CONTRACT, KRN]
cli: [fid pack create-gear]
mcp: [pack_create_gear]
journal: [pack.create_gear@1]
updated: 2026-11-16
---

# IND.GEAR.DEF: Gear definition and nominal flanks

## Purpose

The `CylindricalGear` element and its nominal geometry: the gear parameters a user enters from the drawing, their validation, and the nominal involute profile and helix of every flank in SI metres, which every deviation evaluation compares against.

## Scope

**In:**
- Element type `CylindricalGear`: normal module, number of teeth, normal pressure angle, helix angle and hand, profile shift coefficient, face width, tip and root diameters, profile and helix evaluation ranges, gear axis frame
- Parameter validation and derived quantities (reference, base and measurement diameters)
- Nominal involute profile per flank (transverse plane) and nominal helix per flank
- `pack.create_gear@1` (W2; agents write to an agent branch)

**Out** (and which node owns it instead):
- Deviation evaluations (owned by `IND.GEAR.FLANK`, `IND.GEAR.PITCH`, `IND.GEAR.SIZE`)
- Gear axis datum construction (owned by `GDT` and `ALN`)
- Bevel, worm and non-involute gears (not covered; see parent Open questions)

## Requirements

- **REQ-IND.GEAR.DEF-001** The `CylindricalGear` element type and its create command shall be registered only through the gear pack manifest.
- **REQ-IND.GEAR.DEF-002** When a gear is created, the pack shall validate that the number of teeth is an integer of at least 3, that module, face width and diameters are positive and finite, and that the pressure angle lies strictly between 0 and 45 degrees, and shall refuse invalid input with a typed error naming the parameter.
- **REQ-IND.GEAR.DEF-003** When given a gear definition, the pack shall generate the nominal involute profile of every left and right flank in the transverse plane and the nominal helix of every flank, in SI metres, consistent with the geometry definitions of ISO 21771 (edition to be confirmed) [unverified].
- **REQ-IND.GEAR.DEF-004** The nominal involute evaluated at any roll angle in the evaluation range shall agree with the closed-form involute of the base circle within 1e-12 m [unverified].
- **REQ-IND.GEAR.DEF-005** If the declared evaluation range extends beyond the tip diameter or below the base diameter, then the pack shall refuse the definition with a typed error.

## Interfaces

- **Offers:** element type `CylindricalGear`; `pack.create_gear@1` (W2); nominal flank API (point and normal at a roll angle or face-width position) for FLANK, PITCH and SIZE.
- **Consumes:** KRN numeric facade; CORE document model; `IND.CONTRACT` manifest.

## Decomposition hints

ELEM (element and validation), NOM (nominal involute and helix generator). NOM is a good leaf candidate.

## Open questions

- Internal gears (ring gears) need sign changes in the nominal; whether the first release includes them needs the pack maintainer.

## Children

_None yet: status `open`._
