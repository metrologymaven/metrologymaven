---
id: IND.GEAR.GRADE
title: Gear accuracy grade classification
level: 3
parent: IND.GEAR
kind: module
status: open
phase: P3
size: L
owners: []
claimed_by:
satisfies: [REQ-IND.GEAR-008]
covers: [PCD.IND.02]
depends_on: [IND.GEAR.FLANK, IND.GEAR.PITCH, IND.GEAR.SIZE]
cli: [fid pack classify-gear-grade]
mcp: [pack_classify_gear_grade]
journal: [pack.classify_gear_grade@1]
updated: 2026-11-16
---

# IND.GEAR.GRADE: Gear accuracy grade classification

## Purpose

Classifies each gear deviation into the accuracy grade of the declared gear accuracy standard, using the standard's tolerance formulas as implemented by the pack maintainer, and reports the achieved grade as a derived value with the standard designator and edition in its spec context.

## Scope

**In:**
- Tolerance values per grade as functions of gear size parameters, implemented from the declared standard by the maintainer
- Achieved grade per characteristic and overall grade (worst characteristic)
- `pack.classify_gear_grade@1` (R1)

**Out** (and which node owns it instead):
- The deviations being classified (owned by `IND.GEAR.FLANK`, `IND.GEAR.PITCH`, `IND.GEAR.SIZE`)
- Declaring which standards the pack implements (owned by `IND.GEAR.DATA`)
- Decision rules with uncertainty (owned by `UNC`)

## Requirements

- **REQ-IND.GEAR.GRADE-001** When the declared standard defines accuracy grades, the pack shall report the achieved grade per characteristic as a derived value whose spec context carries the standard designator and edition.
- **REQ-IND.GEAR.GRADE-002** The pack shall report the overall grade of a gear as the worst achieved grade over the evaluated characteristics, naming the characteristic that sets it.
- **REQ-IND.GEAR.GRADE-003** If a characteristic is `NOT_DECIDABLE`, then the pack shall exclude it from the overall grade and shall state the exclusion.
- **REQ-IND.GEAR.GRADE-004** If the gear's size parameters lie outside the validity range of the declared standard, then the pack shall refuse the classification with a typed error.
- **REQ-IND.GEAR.GRADE-005** The tolerance formulas shall be implemented in the pack's own words and code, and no standards text or tables shall be stored or served (CONSTITUTION §5).

## Interfaces

- **Offers:** `pack.classify_gear_grade@1` (R1).
- **Consumes:** gear deviations from FLANK, PITCH and SIZE; standard declarations from `IND.GEAR.DATA`.

## Decomposition hints

Split by standard once more than one is supported (for example ISO and AGMA).

## Open questions

- Whether AGMA standards must be offered alongside ISO for the ASME-dialect user base needs a domain expert.
- Reproducing grade tolerance tables numerically may itself raise copyright questions; the maintainer should confirm that tolerances computed from formulas are acceptable [unverified].

## Children

_None yet: status `open`._
