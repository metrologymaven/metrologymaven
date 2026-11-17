---
id: UNC.MODEL.SUBST.INPUT
title: Substitution inputs, procedure profiles and condition checks
level: 4
parent: UNC.MODEL.SUBST
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-UNC.MODEL.SUBST-001, REQ-UNC.MODEL.SUBST-002, REQ-UNC.MODEL.SUBST-005]
covers: []
depends_on: [UNC.MODEL.GRAPH.SCHEMA, UNC.UEC.STMT]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# UNC.MODEL.SUBST.INPUT: Substitution inputs, procedure profiles and condition checks

## Purpose

The input structure of the ISO 15530-3 calibrated-workpiece method and the checks that run before any evaluation: the calibrated reference (value, U, k, certificate or DCC digest), the repeated runs on the calibrated workpiece, the production measurements, the measurement conditions each was taken under, and a procedure profile that sets the minimum run and repetition counts. It refuses inputs that are too few or taken under different conditions.

## Scope

**In:** `SubstitutionInput`, `CalibratedReference`, `RunSet`, `Conditions { characteristic_kind, feature_ref, strategy_digest, machine_id, probe_config_digest, program_digest }`, `ProcedureProfile { id, min_runs, min_repetitions_per_run }` (project-defined defaults, overridable per site); `check(input, profile) -> Result<CheckedInput, SubstError>`.
**Out:** component formulas (owned by `UNC.MODEL.SUBST.EVAL`); the command (owned by `UNC.MODEL.SUBST.CMD`); certificate parsing (owned by `IO`).

## Requirements

- **REQ-UNC.MODEL.SUBST.INPUT-001** The check shall require the calibrated value with unit, its expanded uncertainty with coverage factor, a source digest of kind `certificate | dcc`, the repeated runs and the production measurement set, failing with `input_missing{field}` otherwise.
- **REQ-UNC.MODEL.SUBST.INPUT-002** If the number of runs or repetitions per run is below the procedure profile minimum, then the check shall fail with `insufficient_runs` reporting the required and supplied counts.
- **REQ-UNC.MODEL.SUBST.INPUT-003** If any field of `Conditions` differs between the calibrated-workpiece runs and the production measurements, then the check shall return `conditions_mismatch` naming the fields, and the input shall not be eligible for UEC-3.
- **REQ-UNC.MODEL.SUBST.INPUT-004** All values shall be in SI units, and if any value is non-finite or a unit is inconsistent with the characteristic kind, then the check shall fail with `invalid_value`.

## Acceptance criteria

- **AC-1** Given `fixtures/unc/model/subst/ring_gauge_ok.json` (calibrated ring 50.0012 mm, U 0.6 µm, k 2, 20 runs of 3 repetitions, profile `default` [unverified: default minimums]), then `check` returns `CheckedInput`.
- **AC-2** (error path) Given 5 runs where the profile requires 20, then `insufficient_runs{required: 20, supplied: 5}`.
- **AC-3** (error path) Given production measurements with a different strategy digest, then `conditions_mismatch{strategy_digest}`.
- **AC-4** (error path) Given a missing coverage factor, then `input_missing{k}`.

## Design

- **Registry command(s):** none.
- **Data structures:** as in Scope.
- **Errors:** `input_missing{field}`, `insufficient_runs{required, supplied}`, `conditions_mismatch{fields}`, `invalid_value`.
- **Determinism:** D0.
- **Performance budget:** trivial.
- **Crate:** `fid-unc` (module `subst`).

## Test fixtures

`fixtures/unc/model/subst/`: invented ring-gauge and bore datasets (CC-BY-4.0).

## Build prompt

> In crate `fid-unc`, module `subst`, define `SubstitutionInput` (calibrated reference value, U, k, source digest of kind certificate or dcc; runs on the calibrated workpiece; production measurements; `Conditions {characteristic_kind, feature_ref, strategy_digest, machine_id, probe_config_digest, program_digest}` per set) and `ProcedureProfile {id, min_runs, min_repetitions_per_run}`. Implement `check(input, profile)`: `input_missing{field}`, `insufficient_runs{required, supplied}`, `conditions_mismatch{fields}` (not eligible for UEC-3), `invalid_value` (non-finite or wrong unit). SI units only. Tests AC-1..AC-4 with `fixtures/unc/model/subst/`. No panic or `unwrap`; BTreeMap only.

## Open questions

- Minimum run and repetition counts of the default profile must be checked against ISO 15530-3:2011 [unverified]; sites may override them.

## Children

_Leaf: none._
