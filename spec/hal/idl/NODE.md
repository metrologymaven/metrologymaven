---
id: HAL.IDL
title: Device interface definitions and capability contracts
level: 2
parent: HAL
kind: subsystem
status: open
phase: P0
size: L
owners: []
claimed_by:
satisfies: [REQ-HAL-001]
covers: []
depends_on: [CORE]
cli: [fid device *-capability, fid device *-capabilities]
mcp: [device_*_capability, device_*_capabilities]
journal: [device.negotiate_*]
updated: 2026-11-16
---

# HAL.IDL: Device interface definitions and capability contracts

## Purpose

The versioned contract every device, driver, simulator and playback source implements: the mandatory `Device` base, the eleven capability contracts, the batch and provenance-stamp schemas, and the negotiation rules that pick a common version at session open. It is a separate unit because it is the one artefact every other HAL child and every third-party driver author builds against, and it must be versioned and published independently of any implementation.

## Scope

**In:**
- Capability packages: control-plane interface, data-plane batch schemas, semantics notes in our own words, and a conformance profile per capability
- The `Device` base: describe, session, health, configuration, calibration-state references
- Capability contracts: TactileProbe, CncMotion (I++-aligned semantics), PortablePose, LineScan, AreaScan, HeightMap, Profile, Gauge, Environment, Rotary, Camera
- Semver rules per capability, and capability negotiation (highest mutually supported minor version and feature flags)
- Units and encodings: SI quantities on the control plane, declared exact encodings on the data plane
- Frames (sensor → mount → device → machine → part) and timestamp fields (`t_dev`, `t_host`, time-source enum)
- The error model (codes, severity, retryable flag, untrusted device message, operator hint ID) and the health state machine
- Schemas of `AcquisitionBatch`, the provenance stamp (including `trust` and `simulated`), `ExternalClaim` for device-computed quantities, and the `instrument-profile/v1` descriptor
- Generation of host stubs and schemas for the registry, MCP and docs from the packages

**Out** (and which node owns it instead):
- Writing stamps and running sessions (owned by `HAL.ACQ`)
- Driver tiers, host processes and templates built from the stubs (owned by `HAL.HOST`)
- The I++ wire protocol client (owned by `HAL.IPP`)
- Conformance test execution and simulators (owned by `HAL.SIM`)
- Uncertainty models that consume sensor descriptors (owned by `UNC`)

## Requirements

- **REQ-HAL.IDL-001** The HAL shall define the `Device` base and each of the eleven capabilities as a capability package with a semantic version, a control-plane interface, data-plane batch schemas and a conformance profile.
- **REQ-HAL.IDL-002** When a session opens, the negotiation rules shall select, per capability, the highest minor version and the feature-flag set supported by both runtime and driver, and shall fail the open with `UNSUPPORTED` if no common major version exists.
- **REQ-HAL.IDL-003** Every control-plane quantity in a capability package shall be typed as a value plus an SI unit (m, rad, s, K, Pa), and every data-plane payload shall declare its exact native encoding (scale, offset, numeric type) so conversion to f64 metres is lossless and reproducible.
- **REQ-HAL.IDL-004** The provenance-stamp schema shall include batch identity and sequence range, gaps, payload digest, capability and features, driver identity and digest, device identity, configuration digest, calibration references, sensor-model reference, time source with sync uncertainty, warm-up state, transcript-segment digest, `trust` (`attested | declared | unknown`) and a boolean `simulated` field.
- **REQ-HAL.IDL-005** The error model shall define a closed set of error codes, each with a documented runtime behaviour, and shall carry every device-supplied message as untrusted text that is never interpreted as instructions (CONSTITUTION C6).
- **REQ-HAL.IDL-006** The `CncMotion` contract shall map one-to-one onto I++ DME session, tool, motion, measurement, scan and event semantics, with each mapping cited by I++ clause ID only [unverified until spec cards exist].
- **REQ-HAL.IDL-007** A breaking change to any capability shall increment its major version, and the previous major version shall remain negotiable for at least one release.

## Interfaces

- **Offers:** capability packages (`Device@1`, `LineScan@1`, ...); generated host stubs and JSON Schemas; `AcquisitionBatch`, provenance-stamp, `ExternalClaim` and `instrument-profile/v1` schemas; `device.describe_capability@1` (R0) and `device.list_capabilities@1` (R0).
- **Consumes:** CORE registry manifest conventions (schema generation hooks).

## Decomposition hints

BASE (`Device` base, health, errors), CAPS (the eleven capability contracts, possibly one child per group), STAMP (batch and provenance-stamp schemas; freeze v0 in P0), NEG (negotiation and versioning rules), GEN (stub and schema generation), PROFILE (`instrument-profile/v1` descriptor).

## Open questions

- Concept doc 08 proposes proto3 plus Arrow schemas plus generated WIT as the package format (H-D2). That is an implementation choice to settle at a leaf; the spec here only requires versioned, generated contracts.
- The stamp schema must be frozen in P0, before most capabilities exist; the remaining capabilities follow their consumer phases (P2 to P4).

## Children

_None yet: status `open`._
