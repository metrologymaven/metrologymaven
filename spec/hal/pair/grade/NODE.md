---
id: HAL.PAIR.GRADE
title: Trust-grade rules and firmware-drift signal
level: 3
parent: HAL.PAIR
kind: task
status: leaf
phase: P2
size: M
owners: []
claimed_by:
satisfies: [REQ-HAL.PAIR-005]
covers: []
depends_on: [TRUST.SIGN]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# HAL.PAIR.GRADE: Trust-grade rules and firmware-drift signal

## Purpose

The pure function that grades a source's provenance as `attested`, `declared` or `unknown`, and the firmware-drift signal that tells the Motion Executive a CMM controller is outside its tested firmware range. Every stamp's `trust` value comes from here, and UNC uses it to cap uncertainty claims.

## Scope

**In:**
- `grade(inputs) -> Grade { trust, reasons, firmware_drift }`
- Inputs: driver signature status, conformance-report validity and coverage (driver digest and capability profile), firmware-in-tested-range, pairing state for remote hosts, source kind, identity assertion
- Firmware-drift signal with a motion-block flag for `CncMotion` sources

**Out:**
- Verifying signatures and reports (owned by `TRUST`; this function receives verified statuses)
- Writing the grade into stamps (owned by `HAL.ACQ.STAMP`)
- Blocking motion (owned by `CMM` execution; it consumes the signal)
- Producing conformance reports (owned by `HAL.SIM.CONF`)

## Requirements

- **REQ-HAL.PAIR.GRADE-001** The function shall return `attested` only when the driver signature is valid, a valid conformance report covers the driver digest and the capability profile in use, the firmware is within the tested range, and, for a remote host, the channel is paired.
- **REQ-HAL.PAIR.GRADE-002** Otherwise, the function shall return `declared` when the source asserts a device identity, and `unknown` when it does not.
- **REQ-HAL.PAIR.GRADE-003** Sources of kind `file` or `entered` shall never grade above `declared`.
- **REQ-HAL.PAIR.GRADE-004** Every downgrade from `attested` shall list each failed condition in `reasons`, using a closed reason set.
- **REQ-HAL.PAIR.GRADE-005** When a source implements `CncMotion` and its firmware is outside the tested range, the function shall set `firmware_drift.motion_block = true`.
- **REQ-HAL.PAIR.GRADE-006** Given identical inputs, the function shall return identical canonical output.

## Acceptance criteria

- **AC-1** Given `fixtures/hal/pair/grade/all_good.json`, when `grade` runs, then trust is `attested` with empty reasons.
- **AC-2** Given `fixtures/hal/pair/grade/fw_out_of_range_cmm.json`, when `grade` runs, then trust is `declared`, reasons contain `FIRMWARE_OUT_OF_RANGE` and `motion_block` is true.
- **AC-3** Given `fixtures/hal/pair/grade/no_report.json`, when `grade` runs, then trust is `declared` with reason `CONFORMANCE_REPORT_MISSING`.
- **AC-4** Given `fixtures/hal/pair/grade/file_signed.json` (file source with every condition true), when `grade` runs, then trust is `declared` with reason `SOURCE_KIND_CAPPED`.
- **AC-5** Given `fixtures/hal/pair/grade/anonymous.json` (no identity, unsigned), when `grade` runs, then trust is `unknown`.
- **AC-6** Given `fixtures/hal/pair/grade/remote_unpaired.json`, when `grade` runs, then trust is `declared` with reason `CHANNEL_UNPAIRED`.
- **AC-7** Given the table of all 2^5 boolean input combinations in `fixtures/hal/pair/grade/truth_table.json`, when `grade` runs on each, then every output equals the expected row.

## Design

- **Registry commands:** none; called by `HAL.ACQ.STAMP` and by the Motion Executive's pre-run checks.
- **Data structures:** `GradeInputs { signature: Status, report: ReportStatus, firmware_in_range: Option<bool>, remote: bool, paired: bool, source_kind, identity_asserted: bool, capabilities }`, `Grade { trust, reasons: Vec<Reason>, firmware_drift: Option<Drift> }`, closed `Reason` enum.
- **Algorithm:** evaluate conditions in a fixed order; collect reasons sorted.
- **Errors:** none; malformed inputs are a type error at the call site.
- **Determinism:** D0.
- **Performance budget:** trivial.
- **Crate:** `fid-hal-pair` (module `grade`).

## Test fixtures

Project-authored JSON inputs and an exhaustive truth table under `fixtures/hal/pair/grade/` (CC-BY-4.0).

## Build prompt

> In crate `fid-hal-pair`, module `grade`, implement `grade(inp: &GradeInputs) -> Grade`. `attested` only if signature valid AND a valid conformance report covers the driver digest and capability profile AND firmware in tested range AND (not remote OR paired); otherwise `declared` if identity asserted, else `unknown`. Sources of kind `file` or `entered` cap at `declared` (`SOURCE_KIND_CAPPED`). Each failed condition adds a reason from a closed enum (`SIGNATURE_INVALID`, `CONFORMANCE_REPORT_MISSING`, `CONFORMANCE_REPORT_INVALID`, `FIRMWARE_OUT_OF_RANGE`, `CHANNEL_UNPAIRED`, `SOURCE_KIND_CAPPED`), sorted. If the source implements `CncMotion` and firmware is out of range, set `firmware_drift.motion_block = true`. Statuses arrive already verified by TRUST; do no crypto here. No panic, unwrap, expect or unsafe (CONSTITUTION §8). Tests: AC-1..AC-7 with fixtures under `fixtures/hal/pair/grade/`.

## Open questions

- Firmware ranges are compared as strings from untrusted device text; a normalized version grammar per driver is needed and is supplied by the driver manifest (`HAL.HOST.PKG`).

## Children

_Leaf: none._
