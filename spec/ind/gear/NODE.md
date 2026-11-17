---
id: IND.GEAR
title: Gear pack
level: 2
parent: IND
kind: subsystem
status: split
phase: P3
size: L
owners: []
claimed_by:
satisfies: [REQ-IND-001, REQ-IND-002, REQ-IND-003, REQ-IND-004]
covers: [PCD.IND.02]
depends_on: [IND.CONTRACT, KRN, GDT, CMM]
cli: [fid pack *-gear*]
mcp: [pack_*_gear*]
journal: [pack.*_gear*]
updated: 2026-11-16
---

# IND.GEAR: Gear pack

## Purpose

Analytical inspection of cylindrical involute gears measured on a CMM or from dense scans: profile, helix and pitch deviations, runout and size over balls or pins, evaluated against a declared gear accuracy standard. Gear metrology has its own vocabulary and evaluation ranges, so it ships as a pack owned by gear experts.

## Scope

**In:**
- Gear definition element: module, number of teeth, pressure angle, helix angle, profile shift, face width, tip and root diameters, evaluation ranges
- Nominal involute and helix generation for each flank
- Profile deviations (total, form, slope) per flank
- Helix deviations (total, form, slope) per flank
- Single and cumulative pitch deviations and radial runout
- Dimension over balls or pins and tooth thickness
- Accuracy-grade classification against the declared standard
- Gear report template (per-flank traces and grade table)

**Out** (and which node owns it instead):
- Probing paths and execution on the CMM (owned by `CMM`)
- Generic fits and curve evaluation (owned by `KRN`)
- Generic runout tolerances on shafts (owned by `GDT`)
- Bevel, worm and non-involute gears (not covered; see Open questions)
- Pack manifest, verification harness and provenance (owned by `IND.CONTRACT`)

## Requirements

- **REQ-IND.GEAR-001** The gear pack shall register its commands, element types, checks and report templates only through the `IND.CONTRACT` manifest.
- **REQ-IND.GEAR-002** The gear pack manifest shall declare each gear accuracy standard it implements by designator and edition, starting with ISO 1328-1:2013 [unverified], and shall name a maintainer.
- **REQ-IND.GEAR-003** When given a gear definition, the gear pack shall generate nominal involute profiles and helices for every flank in SI metres, consistent with the geometry definitions of ISO 21771 (edition to be confirmed) [unverified].
- **REQ-IND.GEAR-004** When given measured profile traces, the gear pack shall compute total, form and slope profile deviation per flank over the declared evaluation range.
- **REQ-IND.GEAR-005** When given measured helix traces, the gear pack shall compute total, form and slope helix deviation per flank over the declared evaluation range.
- **REQ-IND.GEAR-006** When given one measurement point per flank at the reference diameter, the gear pack shall compute single pitch deviation, cumulative pitch deviation and total cumulative pitch deviation for left and right flanks separately.
- **REQ-IND.GEAR-007** If the evaluation range is not covered by measured points, or fewer teeth are measured than the declared pitch evaluation requires, then the gear pack shall report `NOT_DECIDABLE` for the affected characteristics rather than extrapolating.
- **REQ-IND.GEAR-008** When the declared standard defines accuracy grades, the gear pack shall report the achieved grade per characteristic as a derived value with the standard designator and edition in its spec context.
- **REQ-IND.GEAR-009** The gear pack shall ship reference datasets including at least one synthetic gear with known injected deviations, and on it every computed deviation shall match the injected value within 1e-8 m [unverified].

## Interfaces

- **Offers:** element type `CylindricalGear`; commands such as `pack.evaluate_gear@1` (R1), `pack.create_gear_check@1` (W2); gear report template.
- **Consumes:** CMM scanning results or SCAN point data; KRN fits; GDT datum axis; IND.CONTRACT manifest and provenance.

## Decomposition hints

DEF (gear definition and nominals), PROF (profile), HELIX (helix), PITCH (pitch and runout), SIZE (over balls, tooth thickness), GRADE (accuracy grade), DATA, TPL.

## Split rationale

The split follows **gear metrology's own families of deviations**: the definition and nominal flanks (DEF), flank-trace deviations for profile and helix (FLANK), pitch (PITCH), ball-or-pin based values: runout, size over balls and tooth thickness (SIZE), and grading against a standard (GRADE), plus the pack's declarations and datasets (DATA) and template (TPL). Profile and helix share one child because they use the same trace, mean-line and total/form/slope structure; runout moved from pitch to SIZE because it uses ball contacts in the tooth spaces, not single flank points.

Alternatives considered:
- **Separate PROF and HELIX children.** Rejected: the algorithms are the same over different parameters; splitting them would duplicate the mean-line method.
- **Grade inside each deviation child.** Rejected: grading depends on the declared standard and needs one owner for standards interpretation.

## Open questions

- Bevel, worm and spline gears are not in PCD.IND.02's row text and are left out [unverified]. If incumbents cover them, a maintainer should add rows to the seeds and a child here.
- Whether AGMA standards must be offered alongside ISO for the ASME-dialect user base needs a domain expert.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| IND.GEAR.DEF | Gear definition and nominal flanks | 001 003 | PCD.IND.02 |
| IND.GEAR.FLANK | Profile and helix deviations | 004 005 007 | PCD.IND.02 |
| IND.GEAR.PITCH | Gear pitch deviations (leaf) | 006 007 | PCD.IND.02 |
| IND.GEAR.SIZE | Runout, size over balls and tooth thickness | 001 007 | PCD.IND.02 |
| IND.GEAR.GRADE | Gear accuracy grade classification | 008 | PCD.IND.02 |
| IND.GEAR.DATA | Gear standards declarations and reference datasets | 002 009 | none |
| IND.GEAR.TPL | Gear report template | 001 | none |
