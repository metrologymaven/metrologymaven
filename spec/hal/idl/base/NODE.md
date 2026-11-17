---
id: HAL.IDL.BASE
title: Device base package, health state machine and error model
level: 3
parent: HAL.IDL
kind: module
status: split
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-HAL.IDL-001, REQ-HAL.IDL-005]
covers: []
depends_on: [HAL.IDL.UNITS, HAL.IDL.STAMP, HAL.IDL.GEN.FMT, CORE.JRN.CANON]
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# HAL.IDL.BASE: Device base package, health state machine and error model

## Purpose

The `Device@1` capability package that every device, driver, simulator, playback source and file pseudo-device implements: describe, session, health, configuration and calibration-state references, plus the shared health state machine and the closed error model that all other capability packages reuse. It is separate because every capability package imports it, and its error and health semantics must be identical everywhere.

## Scope

**In:**
- `Device@1` control-plane operations: describe, open and close session, health, read and set configuration, read calibration-state references
- The `DeviceInstance` descriptor (make, model, serial, firmware as untrusted text; serving driver identity)
- The health state machine and its allowed transitions; the warm-up state reported into stamps
- The error model: closed code set, severity, retryable flag, device code, untrusted device message, operator hint ID
- Calibration-state references (DCC digests, qualification record, compensation identity, latest interim check): read-only
- Typed configuration with an acquisition-relevant subset that enters the configuration digest
- The `Device-core@1` conformance profile (test list; execution is owned by `HAL.SIM.CONF`)

**Out** (and which node owns it instead):
- Quantity, encoding, frame and timestamp types (owned by `HAL.IDL.UNITS`)
- Stamp and batch schemas (owned by `HAL.IDL.STAMP`)
- Version negotiation (owned by `HAL.IDL.NEG`)
- Device-class capabilities (owned by `HAL.IDL.CNC` and `HAL.IDL.SENS`)
- Running sessions and supervising health at runtime (owned by `HAL.ACQ`)
- The operator-hint catalogue text (owned by `VIZ` and docs; this node only defines the ID space)

## Requirements

- **REQ-HAL.IDL.BASE-001** The `Device@1` package shall define the operations describe, open-session, close-session, get-health, get-configuration, set-configuration and get-calibration-state, each with typed request, response and error cases.
- **REQ-HAL.IDL.BASE-002** The health state machine shall define the states `OFFLINE`, `CONNECTING`, `INITIALIZING`, `WARMING_UP`, `READY`, `BUSY`, `FAULT`, `SAFETY_STOPPED`, `MAINTENANCE` and `INTERRUPTED` and a closed transition table; a transition outside the table reported by a driver shall be a `Device-core@1` conformance failure.
- **REQ-HAL.IDL.BASE-003** The error model shall define exactly the codes `TRANSIENT`, `COMM_LOST`, `CONFIG_INVALID`, `OUT_OF_RANGE`, `NOT_QUALIFIED`, `CALIBRATION_EXPIRED`, `LIMIT`, `SAFETY_STOP_OBSERVED`, `VENDOR_LICENSE`, `UNSUPPORTED` and `INTERNAL`, each with a documented runtime behaviour, a severity and a retryable flag.
- **REQ-HAL.IDL.BASE-004** Every device-supplied string (make, model, serial, firmware, device message) shall be typed `UntrustedText`, and operator guidance shall be referenced only by an `operator_hint_id` into a project-owned catalogue.
- **REQ-HAL.IDL.BASE-005** No operation in any capability package shall clear `SAFETY_STOPPED` or acknowledge `SAFETY_STOP_OBSERVED`; reset happens only at the machine.
- **REQ-HAL.IDL.BASE-006** The `Device@1` package shall expose calibration state as references only and shall define no operation that writes calibration, qualification or compensation data.
- **REQ-HAL.IDL.BASE-007** Configuration shall be typed, and the package shall mark which fields are acquisition-relevant so that only those enter the configuration digest; vendor extensions shall be namespaced, declared in describe and always digested.

## Interfaces

- **Offers:** `Device@1` package; `HealthState`, `DeviceError`, `ErrorCode`, `UntrustedText`, `DeviceInstance`, `CalibrationStateRef`, `ConfigDigest` types; `Device-core@1` conformance profile.
- **Consumes:** `HAL.IDL.UNITS` quantity types; `HAL.IDL.STAMP` digest conventions.

## Decomposition hints

HEALTH (state machine and transition table), ERR (error codes and runtime behaviour table), CFG (configuration typing, acquisition-relevant subset, vendor extensions, digest), DESC (describe and `DeviceInstance`), PROF (`Device-core@1` test list). The first three may each be a leaf.

## Split rationale

The split follows **the four things every package imports from the base**, each testable alone: the error model with untrusted device text (ERR), the health state machine (HEALTH), the configuration model and digest (CFG), and the `Device@1` package that ties them together with its conformance profile (DESC). The `PROF` hint is folded into DESC, because a profile is a list of test items owned by the package it tests.

All four live in the L0 crate `fid-hal-idl` and depend only on L0 nodes (`HAL.IDL.*` and the canonical-JSON library `CORE.JRN.CANON`), as CONSTITUTION §2.1 requires for HAL.IDL types.

Alternatives considered:
- **A single leaf.** Rejected: four independent rule sets with separate fixtures exceed one M-sized PR.
- **ERR inside HEALTH.** Rejected: error behaviours are reused by every package operation, not only by health.

## Open questions

- Resolved here: `MAINTENANCE` is enterable by the device (any state except `BUSY` and `SAFETY_STOPPED`) and by the runtime only from `OFFLINE`; see `HAL.IDL.BASE.HEALTH`.
- The package file format is chosen in `HAL.IDL.GEN.FMT` (a project-owned `capability-package/v1` description exported from Rust types) rather than proto3 plus Arrow (concept doc 08 H-D2).

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| HAL.IDL.BASE.ERR | Device error model, untrusted device text and operator hint IDs (leaf) | 003 004 | none |
| HAL.IDL.BASE.HEALTH | Health state machine and transition table (leaf) | 002 005 | none |
| HAL.IDL.BASE.CFG | Typed configuration, acquisition-relevant subset and configuration digest (leaf) | 007 | none |
| HAL.IDL.BASE.DESC | Device@1 package operations, DeviceInstance, calibration-state references and Device-core@1 (leaf) | 001 004 006 | none |
