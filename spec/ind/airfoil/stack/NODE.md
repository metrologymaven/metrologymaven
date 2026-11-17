---
id: IND.AIRFOIL.STACK
title: Blade section stacking
level: 3
parent: IND.AIRFOIL
kind: module
status: open
phase: P3
size: L
owners: []
claimed_by:
satisfies: [REQ-IND.AIRFOIL-005, REQ-IND.AIRFOIL-006]
covers: [GOM.SPE.05, PCD.IND.01, PW.IND.01]
depends_on: [IND.AIRFOIL.PARAMS]
cli: [fid pack evaluate-airfoil-stack]
mcp: [pack_evaluate_airfoil_stack]
journal: [pack.evaluate_airfoil_stack@1]
updated: 2026-11-16
---

# IND.AIRFOIL.STACK: Blade section stacking

## Purpose

Evaluates several sections of one blade together against a declared stacking axis: per-section centroid offset (bow and lean components) and twist relative to nominal, ordered by span position.

## Scope

**In:**
- Stack definition: ordered sections, stacking axis, bow and lean reference directions
- Per-section centroid offset split into declared bow and lean directions
- Per-section twist (measured minus nominal stagger) and twist relative to a declared reference section
- Validity checks on the stack
- `pack.evaluate_airfoil_stack@1` (R1)

**Out** (and which node owns it instead):
- Per-section centroid and stagger (owned by `IND.AIRFOIL.PARAMS`)
- Blade-root datum alignment (owned by `ALN`)

## Requirements

- **REQ-IND.AIRFOIL.STACK-001** When a stack is evaluated, the pack shall report for each section its span position, centroid offset from the stacking axis in the declared bow and lean directions, and twist relative to nominal.
- **REQ-IND.AIRFOIL.STACK-002** The pack shall order sections by their span position along the stacking axis, independent of input order.
- **REQ-IND.AIRFOIL.STACK-003** If fewer than two sections are given, or any section plane deviates from perpendicular to the stacking axis by more than the declared angle tolerance, then the pack shall refuse the evaluation with a typed error naming the section.
- **REQ-IND.AIRFOIL.STACK-004** Every stack value compared with a tolerance shall be emitted as a ResultRecord through `IND.CONTRACT` with UEC-0 unless an uncertainty evaluation is attached.

## Interfaces

- **Offers:** `pack.evaluate_airfoil_stack@1` (R1).
- **Consumes:** `IND.AIRFOIL.PARAMS` centroid and stagger; ALN datum frames for the stacking axis.

## Decomposition hints

Likely a single leaf once PARAMS fixes its output type.

## Open questions

- Whether bow and lean are defined in the engine frame or relative to the chord direction varies [unverified]; make it a declared option.

## Children

_None yet: status `open`._
