---
id: HAL.IDL.BASE.ERR
title: Device error model, untrusted device text and operator hint IDs
level: 4
parent: HAL.IDL.BASE
kind: task
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-HAL.IDL.BASE-003, REQ-HAL.IDL.BASE-004]
covers: []
depends_on: [HAL.IDL.STAMP]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# HAL.IDL.BASE.ERR: Device error model, untrusted device text and operator hint IDs

## Purpose

The closed error model shared by every capability package: eleven error codes, each with a severity, a retryable flag and one normative runtime behaviour class, plus the `DeviceError` record that carries a device's own code and message as untrusted text and points to operator guidance only by ID. One definition keeps every driver, simulator and runtime component in agreement about what an error means.

## Scope

**In:**
- `ErrorCode` closed enum: `TRANSIENT`, `COMM_LOST`, `CONFIG_INVALID`, `OUT_OF_RANGE`, `NOT_QUALIFIED`, `CALIBRATION_EXPIRED`, `LIMIT`, `SAFETY_STOP_OBSERVED`, `VENDOR_LICENSE`, `UNSUPPORTED`, `INTERNAL`
- Per code: `severity()`, `retryable()`, `behaviour()` (`Retry | Interrupt | StopStreams | Fail | Fault`) and a semantics note in the project's own words
- `DeviceError { code, device_code, message: UntrustedText, hint: Option<OperatorHintId> }`
- The `OperatorHintId` ID space (`hal.<code>.<nnn>`); the catalogue text is not here
- Re-export of `UntrustedText` (defined with the stamp types of `HAL.IDL.STAMP`) as the type for every device-supplied string

**Out:**
- Applying behaviours at runtime (owned by `HAL.ACQ.SESS.HEALTH`)
- The hint catalogue text (owned by `VIZ` and docs)
- Health states (owned by `HAL.IDL.BASE.HEALTH`)

## Requirements

- **REQ-HAL.IDL.BASE.ERR-001** The `ErrorCode` enum shall contain exactly the eleven codes listed in Scope, and deserialising any other code shall fail with `error_code_unknown`.
- **REQ-HAL.IDL.BASE.ERR-002** Each code shall have a fixed severity (`info | warning | error | critical`), retryable flag and behaviour, given by the table in Design and covered by an exhaustive test.
- **REQ-HAL.IDL.BASE.ERR-003** `DeviceError.message` and every other device-supplied string shall be `UntrustedText` limited to 4 096 bytes, and longer strings shall be truncated at a UTF-8 boundary with a `truncated` marker rather than rejected.
- **REQ-HAL.IDL.BASE.ERR-004** Operator guidance shall be referenced only by an `OperatorHintId` matching `hal\.[a-z_]+\.[0-9]{3}`, and a `DeviceError` shall carry no free-text guidance field.
- **REQ-HAL.IDL.BASE.ERR-005** A `DeviceError` shall round-trip through canonical JSON with byte-identical output.

## Acceptance criteria

- **AC-1** Given `fixtures/hal/idl/base/err/all_codes.json`, when parsed, then all eleven codes parse and round-trip byte-identically.
- **AC-2** (error path) Given code `"OVERHEAT"`, then parsing fails with `error_code_unknown`.
- **AC-3** Given the behaviour table test, then for every variant `behaviour()`, `severity()` and `retryable()` equal the Design table (exhaustive match; adding a variant without a row fails the build).
- **AC-4** Given a 10 000-byte device message, then it is stored as 4 096 bytes or fewer, ending on a UTF-8 boundary, with `truncated: true`.
- **AC-5** (error path) Given hint ID `"see manual page 4"`, then parsing fails with `hint_id_invalid`.

## Design

- **Registry commands:** none; library types.
- **Table:**

| Code | Severity | Retryable | Behaviour |
|---|---|---|---|
| TRANSIENT | warning | yes | Retry |
| COMM_LOST | error | no | Interrupt |
| CONFIG_INVALID | error | no | Fail |
| OUT_OF_RANGE | error | no | Fail |
| NOT_QUALIFIED | error | no | Fail |
| CALIBRATION_EXPIRED | warning | no | Fail |
| LIMIT | error | no | StopStreams |
| SAFETY_STOP_OBSERVED | critical | no | StopStreams |
| VENDOR_LICENSE | error | no | Fail |
| UNSUPPORTED | error | no | Fail |
| INTERNAL | critical | no | Fault |

- **Data structures:** as in Scope; `UntrustedText { bytes: String, truncated: bool }` re-exported.
- **Errors:** `error_code_unknown`, `hint_id_invalid`.
- **Determinism:** D0.
- **Performance budget:** trivial.
- **Crate:** `fid-hal-idl` (module `base::error`), layer L0.

## Test fixtures

Project-authored JSON under `fixtures/hal/idl/base/err/` (CC-BY-4.0).

## Build prompt

> In crate `fid-hal-idl` (layer L0, no CORE dependency), module `base::error`, define the closed `ErrorCode` enum (TRANSIENT, COMM_LOST, CONFIG_INVALID, OUT_OF_RANGE, NOT_QUALIFIED, CALIBRATION_EXPIRED, LIMIT, SAFETY_STOP_OBSERVED, VENDOR_LICENSE, UNSUPPORTED, INTERNAL) with `severity()`, `retryable()` and `behaviour()` per the table in the spec (exhaustive match), `DeviceError { code, device_code: Option<i64>, message: UntrustedText, hint: Option<OperatorHintId> }`, and `OperatorHintId` validated against `hal\.[a-z_]+\.[0-9]{3}`. Re-export `UntrustedText` from the stamp module; truncate device strings above 4 096 bytes at a UTF-8 boundary with `truncated: true`. Unknown codes fail `error_code_unknown`; bad hint IDs `hint_id_invalid`. Serde with canonical JSON round-trip. No panic, unwrap, expect or unsafe (CONSTITUTION §8). Tests: AC-1..AC-5.

## Open questions

- `HAL.IDL.PROFILE` rejects over-long `UntrustedText` (`untrusted_text_too_long`) while device messages here are truncated; the difference is deliberate (profiles are authored files, device messages arrive live), but `HAL.IDL.STAMP` should state which rule applies to stamp identity strings.

## Children

_Leaf: none._
