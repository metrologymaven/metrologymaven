---
id: IND.GEAR.FLANK
title: Profile and helix deviations
level: 3
parent: IND.GEAR
kind: module
status: open
phase: P3
size: L
owners: []
claimed_by:
satisfies: [REQ-IND.GEAR-004, REQ-IND.GEAR-005, REQ-IND.GEAR-007]
covers: [PCD.IND.02]
depends_on: [IND.GEAR.DEF]
cli: [fid pack evaluate-gear-profile, fid pack evaluate-gear-helix]
mcp: [pack_evaluate_gear_profile, pack_evaluate_gear_helix]
journal: [pack.evaluate_gear_profile@1, pack.evaluate_gear_helix@1]
updated: 2026-11-16
---

# IND.GEAR.FLANK: Profile and helix deviations

## Purpose

Evaluates measured profile traces (along the involute) and helix traces (along the face width) of each flank against the nominal from `IND.GEAR.DEF`, giving total, form and slope deviation per flank over the declared evaluation range. Profile and helix share the same structure (deviation trace, mean line, three derived values), so one module owns both.

## Scope

**In:**
- Deviation trace: signed normal deviation of measured points from the nominal flank, as a function of roll length (profile) or face-width position (helix)
- Mean line over the evaluation range and the total, form and slope deviations
- Optional modifications declared in the gear definition (for example tip relief, crowning) as part of the nominal [unverified scope for first release]
- `pack.evaluate_gear_profile@1` and `pack.evaluate_gear_helix@1` (R1)

**Out** (and which node owns it instead):
- Nominal flank generation (owned by `IND.GEAR.DEF`)
- Pitch and runout (owned by `IND.GEAR.PITCH` and `IND.GEAR.SIZE`)
- Grade classification (owned by `IND.GEAR.GRADE`)

## Requirements

- **REQ-IND.GEAR.FLANK-001** When given a measured profile trace, the pack shall compute total, form and slope profile deviation per flank over the declared evaluation range.
- **REQ-IND.GEAR.FLANK-002** When given a measured helix trace, the pack shall compute total, form and slope helix deviation per flank over the declared evaluation range.
- **REQ-IND.GEAR.FLANK-003** The mean line used for form and slope deviation shall be a least-squares line through the deviation trace over the evaluation range, and the method ID shall be recorded in each ResultRecord.
- **REQ-IND.GEAR.FLANK-004** If the measured points do not span the declared evaluation range within the declared coverage tolerance, then the pack shall report the affected deviations as `NOT_DECIDABLE` and shall not extrapolate.
- **REQ-IND.GEAR.FLANK-005** Every deviation shall be emitted as a ResultRecord through `IND.CONTRACT` with evaluation basis `POINTS` and UEC-0 unless an uncertainty evaluation is attached.

## Interfaces

- **Offers:** the two commands above; `FlankDeviation { total, form, slope, trace }` type.
- **Consumes:** `IND.GEAR.DEF` nominal flank API; CMM scanning results or SCAN point data.

## Decomposition hints

TRACE (deviation trace and coverage check), EVAL (mean line and derived values). Each could be a leaf.

## Open questions

- Exact definitions of form and slope deviation follow ISO 1328-1 (edition per the manifest) [unverified]; the maintainer must confirm them from the standard without copying its text.

## Children

_None yet: status `open`._
