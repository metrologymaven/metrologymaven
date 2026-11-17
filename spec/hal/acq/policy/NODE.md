---
id: HAL.ACQ.POLICY
title: Calibration-state policy and ExternalClaim classification
level: 3
parent: HAL.ACQ
kind: task
status: leaf
phase: P2
size: M
owners: []
claimed_by:
satisfies: [REQ-HAL.ACQ-007, REQ-HAL.ACQ-008]
covers: []
depends_on: [HAL.IDL.STAMP, HAL.IDL.BASE]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# HAL.ACQ.POLICY: Calibration-state policy and ExternalClaim classification

## Purpose

Two pure decisions made on every acquisition: whether a device's calibration and qualification state lets acquisition proceed (flag or block, by tenant policy), and whether a value a driver reports is a calibrated observation or a device-computed `ExternalClaim`. Keeping both as pure functions makes them replayable and testable without devices.

## Scope

**In:**
- `check_calibration(state, policy, now) -> Allow | Flag(reason) | Block(reason)`
- `classify(report, capability) -> Observation | ExternalClaim`
- The tenant policy input type (`flag` default, `block` option) per calibration item

**Out:**
- Motion blocking on expired calibration (owned by `CMM` execution)
- Storing tenant policy and profiles (owned by `TRUST.PROFILE`)
- Decision rules returning `NOT_DECIDABLE` (owned by `UNC.DEC`)
- Writing the result into the stamp (owned by `HAL.ACQ.STAMP`)

## Requirements

- **REQ-HAL.ACQ.POLICY-001** When a device's calibration or qualification reference is expired at acquisition time and the policy is `flag`, the check shall return `Flag` with reason `CALIBRATION_EXPIRED` or `NOT_QUALIFIED`.
- **REQ-HAL.ACQ.POLICY-002** When the same state occurs and the policy is `block`, the check shall return `Block` with the same reason, and `device.start_acquisition@1` shall fail with that code.
- **REQ-HAL.ACQ.POLICY-003** If calibration state is unknown (no reference supplied), then the check shall return `Flag` with reason `CALIBRATION_UNKNOWN` under either policy.
- **REQ-HAL.ACQ.POLICY-004** The classifier shall label as `ExternalClaim` every reported quantity that the capability package does not define as a raw observation payload field (for example an on-board diameter or a device-fitted plane).
- **REQ-HAL.ACQ.POLICY-005** An `ExternalClaim` shall record the device as algorithm source and the device's method text as `UntrustedText`, and shall never be emitted as a value of record.
- **REQ-HAL.ACQ.POLICY-006** Given identical inputs including the evaluation time, both functions shall return identical outputs.

## Acceptance criteria

- **AC-1** Given `fixtures/hal/acq/policy/cal_expired.json` (DCC valid until 2027-01-01, now 2027-02-01) and policy `flag`, when the check runs, then it returns `Flag(CALIBRATION_EXPIRED)`.
- **AC-2** Given the same state and policy `block`, when the check runs, then it returns `Block(CALIBRATION_EXPIRED)`.
- **AC-3** Given `fixtures/hal/acq/policy/cal_none.json`, when the check runs under `block`, then it returns `Flag(CALIBRATION_UNKNOWN)`.
- **AC-4** Given `fixtures/hal/acq/policy/gauge_report.json` (raw `Gauge` reading plus a device-computed mean), when classified, then the reading is an `Observation` and the mean is an `ExternalClaim` with device as source.
- **AC-5** Given `fixtures/hal/acq/policy/arm_circle.json` (device-fitted circle diameter), when classified, then it is an `ExternalClaim` and its method text is `UntrustedText`.
- **AC-6** Given each AC fixture evaluated twice, then outputs are byte-identical canonical JSON.

## Design

- **Registry commands:** none; called by `device.start_acquisition@1` and the stamping path.
- **Data structures:** `CalibrationState` (from `HAL.IDL.BASE`), `CalPolicy { per_item: BTreeMap<CalItem, PolicyMode> }`, `CalVerdict`, `ReportedQuantity`, `Classification`.
- **Algorithm:** compare validity end against `now` (explicit input, never the system clock); classification by lookup of the capability's raw payload field list.
- **Errors:** none beyond the verdicts; malformed inputs fail `policy_input_invalid`.
- **Determinism:** D0; time is an input.
- **Performance budget:** trivial.
- **Crate:** `fid-hal-runtime` (module `policy`).

## Test fixtures

Project-authored JSON under `fixtures/hal/acq/policy/` (CC-BY-4.0).

## Build prompt

> In crate `fid-hal-runtime`, module `policy`, implement `check_calibration(state: &CalibrationState, policy: &CalPolicy, now_utc_ns: i64) -> CalVerdict` returning `Allow`, `Flag(reason)` or `Block(reason)`: expired calibration or qualification → `Flag` under policy `flag`, `Block` under `block`; missing references → `Flag(CALIBRATION_UNKNOWN)` always. Implement `classify(q: &ReportedQuantity, cap: &CapabilitySchema) -> Classification`: anything that is not a raw payload field of the capability is an `ExternalClaim` (device as algorithm source, method as `UntrustedText`), never a value of record. Time is an explicit input; never read the clock. No panic, unwrap, expect or unsafe; BTreeMap only (CONSTITUTION §8). Tests: AC-1..AC-6 with fixtures under `fixtures/hal/acq/policy/`.

## Open questions

- Grace periods before expiry (warn N days ahead) are not specified; add them only if `TRUST.PROFILE` asks for them.

## Children

_Leaf: none._
