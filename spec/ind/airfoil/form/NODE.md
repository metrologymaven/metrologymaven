---
id: IND.AIRFOIL.FORM
title: Zone-wise airfoil profile form
level: 3
parent: IND.AIRFOIL
kind: module
status: open
phase: P3
size: L
owners: []
claimed_by:
satisfies: [REQ-IND.AIRFOIL-001, REQ-IND.AIRFOIL-006]
covers: [GOM.SPE.05, PCD.IND.01, PW.IND.01]
depends_on: [IND.AIRFOIL.SECTION, GDT, ALN]
cli: [fid pack evaluate-airfoil-form]
mcp: [pack_evaluate_airfoil_form]
journal: [pack.evaluate_airfoil_form@1]
updated: 2026-11-16
---

# IND.AIRFOIL.FORM: Zone-wise airfoil profile form

## Purpose

Evaluates the deviation of the measured section from nominal per zone (pressure side, suction side, leading edge, trailing edge), after an optional in-plane best fit whose translation and rotation allowances are declared per evaluation. This is how blade profile tolerances, often different per zone, are checked.

## Scope

**In:**
- Per-zone normal deviation (minimum, maximum, form = max minus min) of measured against nominal
- In-plane best fit with declared allowances: none, translation only, translation and rotation, each optionally bounded
- Recording the allowance and the fitted transform in each ResultRecord
- Delegating to GDT profile-of-a-line evaluation when the tolerance is a profile callout
- `pack.evaluate_airfoil_form@1` (R1)

**Out** (and which node owns it instead):
- Generic profile-of-a-line tolerance semantics (owned by `GDT`)
- The best-fit solver (owned by `ALN`)
- Zone labelling (owned by `IND.AIRFOIL.SECTION`)

## Requirements

- **REQ-IND.AIRFOIL.FORM-001** The form evaluation command shall be registered only through the airfoil pack manifest.
- **REQ-IND.AIRFOIL.FORM-002** When form is evaluated, the pack shall report per zone the minimum and maximum signed normal deviation and the form value in metres.
- **REQ-IND.AIRFOIL.FORM-003** When a best fit is declared, the pack shall restrict it to the section plane with exactly the declared translation and rotation allowances and bounds, and shall record the allowance and the fitted transform in each ResultRecord.
- **REQ-IND.AIRFOIL.FORM-004** If a declared bound on translation or rotation is reached by the best fit, then the pack shall flag the result as bound-limited.
- **REQ-IND.AIRFOIL.FORM-005** When a zone carries a profile-of-a-line callout, the pack shall obtain the evaluation from GDT and shall not re-implement the tolerance zone semantics.
- **REQ-IND.AIRFOIL.FORM-006** Every zone form result compared with a tolerance shall be emitted as a ResultRecord through `IND.CONTRACT` with UEC-0 unless an uncertainty evaluation is attached.

## Interfaces

- **Offers:** `pack.evaluate_airfoil_form@1` (R1).
- **Consumes:** `IND.AIRFOIL.SECTION` zones; ALN constrained 2D best fit; GDT profile-of-a-line evaluation.

## Decomposition hints

DEV (per-zone deviations), FIT (constrained in-plane best fit wrapper).

## Open questions

- Whether some customers fit on one zone and evaluate another (for example fit on the pressure side only) [unverified]; if so the fit zone should be a declared parameter.

## Children

_None yet: status `open`._
