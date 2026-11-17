---
id: UNC.MODEL.SUBST
title: ISO 15530-3 substitution evaluation
level: 3
parent: UNC.MODEL
kind: module
status: open
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-UNC.MODEL-006]
covers: []
depends_on: [UNC.MODEL.GRAPH, UNC.UEC.CLASS]
cli: [fid unc run-substitution]
mcp: [unc_run_substitution]
journal: [unc.run_substitution@1]
updated: 2026-11-16
---

# UNC.MODEL.SUBST: ISO 15530-3 substitution evaluation

## Purpose

The calibrated-workpiece (substitution) method for task-specific CMM uncertainty: repeated measurements of a calibrated workpiece, the workpiece's calibration value and uncertainty, and the production measurements are combined into a task-specific U whose components are reported separately. It needs no machine control, so it can ship in P1a as a stretch goal. It stays open because the component breakdown must be checked against the purchased standard.

## Scope

**In:**
- Input structure: calibrated workpiece reference (value, U, k, DCC or certificate reference), repeated runs on the calibrated workpiece, procedure profile (minimum run count, minimum repetitions), production runs
- Component evaluation (calibration, procedure repeatability, systematic error treatment, workpiece and material variation) and combined U
- `unc.run_substitution@1` (R1 scratch result; committing the statement goes through `UNC.PROP.SELECT`'s commit command)
- Validation record that `UNC.UEC.CLASS` needs for UEC-3

**Out:**
- ISO/TS 15530-4 simulation (owned by `UNC.INST.IEMPS` for inputs and `UNC.PROP.MCM` for evaluation)
- Calibration certificate parsing (owned by `IO`)

## Requirements

- **REQ-UNC.MODEL.SUBST-001** When `unc.run_substitution@1` is run, the system shall require the calibrated value with its uncertainty and coverage factor, at least the minimum number of runs set by the procedure profile, and the production measurement set.
- **REQ-UNC.MODEL.SUBST-002** If fewer runs than the profile minimum are supplied, then the command shall fail with `insufficient_runs` and report the required count.
- **REQ-UNC.MODEL.SUBST-003** The system shall report each uncertainty component separately with its value and source, and the combined expanded U with k [unverified: component naming per ISO 15530-3:2011].
- **REQ-UNC.MODEL.SUBST-004** The system shall report the systematic deviation between the mean of the repeated runs and the calibrated value, and state whether it was corrected or included in U.
- **REQ-UNC.MODEL.SUBST-005** If the production measurement conditions (feature, strategy digest, machine) differ from those of the calibrated-workpiece runs, then the system shall refuse UEC-3 and return `conditions_mismatch`.

## Interfaces

- **Offers:** `SubstitutionInput`, `SubstitutionResult` with component breakdown and a validation record reference for UEC-3; command `unc.run_substitution@1`.
- **Consumes:** model graph (UNC.MODEL.GRAPH); class assignment (UNC.UEC.CLASS); measured results from GDT and KRN through the bus.

## Decomposition hints

INPUT (structure and procedure profiles), EVAL (component formulas and combination), CMD (command and validation record).

## Open questions

- Component names, minimum run counts and the bias treatment must be checked against the purchased text of ISO 15530-3:2011 and any newer edition [unverified]. A GD&T or uncertainty domain expert should review before this node becomes a leaf.

## Children

_None yet: status `open`._
