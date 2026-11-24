---
id: HAL.IDL.BASE.DESC
title: Device@1 package operations, DeviceInstance, calibration-state references and Device-core@1
level: 4
parent: HAL.IDL.BASE
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
built_in: [fid-hal-idl]
satisfies: [REQ-HAL.IDL.BASE-001, REQ-HAL.IDL.BASE-004, REQ-HAL.IDL.BASE-006]
covers: []
depends_on: [HAL.IDL.BASE.ERR, HAL.IDL.BASE.HEALTH, HAL.IDL.BASE.CFG, HAL.IDL.GEN.FMT]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# HAL.IDL.BASE.DESC: Device@1 package operations, DeviceInstance, calibration-state references and Device-core@1

## Purpose

The `Device@1` capability package itself: its seven operations with typed requests, responses and error cases; the `DeviceInstance` descriptor; read-only calibration-state references; and the `Device-core@1` conformance profile (the list of tests, not their execution). Every other package and every driver builds on it.

## Scope

**In:**
- Operations: `describe`, `open_session`, `close_session`, `get_health`, `get_configuration`, `set_configuration`, `get_calibration_state`, each with request, response, error cases and side-effect class
- `DeviceInstance { make, model, serial, firmware }` as `UntrustedText`, plus serving driver identity
- `CalibrationState { dcc_digests, qualification_ref, compensation_id, last_interim_check, valid_until }`, read-only
- `Device-core@1` conformance profile: a numbered list of test items
- The package's `capability-package/v1` description (via `HAL.IDL.GEN.FMT`)

**Out:**
- Health states, error model and configuration model (owned by sibling leaves)
- Version negotiation (owned by `HAL.IDL.NEG`)
- Running the profile (owned by `HAL.SIM.CONF`)

## Requirements

- **REQ-HAL.IDL.BASE.DESC-001** The `Device@1` package shall define exactly the seven operations in Scope, each with a typed request, typed response, a list of allowed `ErrorCode`s and a side-effect class (`describe`, `get_health`, `get_configuration` and `get_calibration_state` R0; the others D1).
- **REQ-HAL.IDL.BASE.DESC-002** `DeviceInstance` string fields shall be `UntrustedText`, and the package shall contain no field that carries operator guidance as free text.
- **REQ-HAL.IDL.BASE.DESC-003** The package shall define no operation that writes calibration, qualification or compensation data; `get_calibration_state` shall return references and validity dates only.
- **REQ-HAL.IDL.BASE.DESC-004** The `Device-core@1` profile shall list at least: describe consistency, session open and close, health reporting within the transition table, error-code closure, configuration round-trip and digest stability, and calibration-state read-only behaviour.
- **REQ-HAL.IDL.BASE.DESC-005** The package description shall validate against `capability-package/v1` and its digest shall be identical on every supported platform.

## Acceptance criteria

- **AC-1** Given the package, when exported as `capability-package/v1`, then it validates and lists seven operations with the classes in REQ-HAL.IDL.BASE.DESC-001.
- **AC-2** Given the export digested on Windows, Linux and macOS, then the three digests equal `fixtures/hal/idl/base/desc/device_v1.sha256`.
- **AC-3** Given `fixtures/hal/idl/base/desc/instance.json` with a 5 000-byte serial, then the serial is truncated per the `UntrustedText` rule.
- **AC-4** Given the package's operation list, then no operation's request type contains a calibration, qualification or compensation write field (schema walk test).
- **AC-5** Given the `Device-core@1` profile, then it holds at least the six items of REQ-HAL.IDL.BASE.DESC-004, each with a stable item ID.

## Design

- **Registry commands:** none; library. Runtime callers are `HAL.ACQ.SESS.*`.
- **Data structures:** request and response types per operation; `DeviceInstance`; `CalibrationState`; `ConformanceProfile { id: "Device-core@1", items: Vec<ProfileItem { id, title, checks }> }`.
- **Algorithm:** none beyond type definitions and the package description builder.
- **Errors:** per operation, drawn from `ErrorCode`.
- **Determinism:** D0.
- **Performance budget:** trivial.
- **Crate:** `fid-hal-idl` (module `base::device`), layer L0.

## Test fixtures

Project-authored fixtures under `fixtures/hal/idl/base/desc/` (CC-BY-4.0); golden package digest.

## Build prompt

> In crate `fid-hal-idl` (L0), module `base::device`, define the `Device@1` capability package: operations `describe`, `open_session`, `close_session`, `get_health`, `get_configuration`, `set_configuration`, `get_calibration_state` with typed requests, responses, allowed `ErrorCode`s and classes (R0 for describe and the three gets, D1 for the rest); `DeviceInstance { make, model, serial, firmware: UntrustedText, driver_id }`; read-only `CalibrationState { dcc_digests, qualification_ref, compensation_id, last_interim_check, valid_until }` with no write operation anywhere; the `Device-core@1` conformance profile with stable item IDs (describe consistency, session open/close, health within the transition table, error-code closure, configuration round-trip and digest stability, calibration read-only). Implement the `CapabilityPackage` trait from `HAL.IDL.GEN.FMT` so the package exports as `capability-package/v1`. No panic, unwrap, expect or unsafe (CONSTITUTION §8). Tests: AC-1..AC-5.

## Open questions

- None.

## Children

_Leaf: none._
