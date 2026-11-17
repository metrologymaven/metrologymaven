---
id: UNC.INST
title: Instrument uncertainty models
level: 2
parent: UNC
kind: subsystem
status: split
phase: P1a
size: XL
owners: []
claimed_by:
satisfies: [REQ-UNC-003]
covers: [SA.UNC.01]
depends_on: [UNC.MODEL]
cli: [fid unc *-instrument-model, fid unc *-profile]
mcp: [unc_*_instrument_model, unc_*_profile]
journal: [unc.*_instrument_model]
updated: 2026-11-16
---

# UNC.INST: Instrument uncertainty models

## Purpose

Provides **versioned, parameterised instrument uncertainty models** for CMMs, laser trackers, articulated arms and scanners, as data rather than code: specification-derived models built from published MPE terms (ISO 10360 series), and full Instrument Error Model Parameter Sets (IEMPS) where makers or users supply them. It also ships an instrument-profile library so a first result need not be UEC-0, and it exports per-observation weights for networks and transforms. It is separate because instrument models are a distinct data product with their own provenance and update cadence.

## Scope

**In:**
- Instrument model schema: parameters, units, applicable range, source, version, digest
- Specification-derived models from MPE terms (CMM per ISO 10360-2, arms per ISO 10360-12, scanners per ISO 10360-8, trackers per ISO 10360-10, multi-sensor per ISO 10360-13 [unverified: part-to-device mapping]) with an explicit term-to-contributor mapping
- IEMPS ingestion for virtual-CMM simulation (ISO/TS 15530-4 inputs)
- Tracker, arm and scanner per-observation models (range, angle, probing noise)
- Instrument-profile library with cited sources
- Per-observation covariance and weight export for networks and transforms
- Binding a model to a device instance or a project

**Out** (and which node owns it instead):
- Device drivers, capabilities and descriptor transport (owned by `HAL`)
- Non-instrument contributors such as thermal and sampling (owned by `UNC.MODEL`)
- Network adjustment that uses the weights (owned by `LVM`)
- Transform uncertainty (owned by `UNC.XFM`)

## Requirements

- **REQ-UNC.INST-001** The system shall store every instrument uncertainty model as versioned data with named parameters, units, valid operating range, source citation and a content digest.
- **REQ-UNC.INST-002** The system shall provide specification-derived models for at least one CMM, laser tracker, articulated arm and scanner class each, built from published MPE terms.
- **REQ-UNC.INST-003** Every specification-derived model shall state which MPE term maps to which contributor, and results using it shall be classed UEC-1.
- **REQ-UNC.INST-004** If a measurement lies outside a model's valid operating range, then the system shall flag the result and shall not silently extrapolate the model.
- **REQ-UNC.INST-005** When an observation from a tracker, arm or scanner is evaluated, the system shall produce its per-observation covariance matrix in SI units from the bound model.
- **REQ-UNC.INST-006** The system shall export per-observation weights derived from the model covariance in a form consumed by `LVM` adjustment and `UNC.XFM`.
- **REQ-UNC.INST-007** When an IEMPS is imported, the system shall verify that it references a passing ISO 10360-2 acceptance or reverification record before it can be bound for simulation use.
- **REQ-UNC.INST-008** The instrument-profile library shall ship with at least 10 profiles at first release, each with a public source citation [unverified].

## Interfaces

- **Offers:** `InstrumentModel` type; covariance and weight export; commands such as `unc.import_instrument_model@1` (W2), `unc.bind_instrument_model@1` (W2), `unc.list_profile@1` (R0), `unc.show_instrument_model@1` (R0).
- **Consumes:** device identity and provenance stamps from HAL; model graph contract from `UNC.MODEL`.

## Decomposition hints

SCHEMA (model data format), MPE (spec-derived models per ISO 10360 part), PORT (tracker, arm and scanner observation models), IEMPS (virtual-CMM parameter sets), LIB (profile library), WEIGHT (covariance and weight export).

## Open questions

- Ownership of the device descriptor format is with HAL; this node only maps parameters to contributors.
- Licensing or interfacing PTB's VCMM for ISO/TS 15530-4 is open [unverified].

## Split rationale

Split by **kind of instrument evidence**, around one shared data format: the format with import, binding and range check (SCHEMA); MPE-derived models from datasheets (MPE); per-observation covariance models for portables and scanners (PORT); maker parameter sets for virtual-CMM simulation (IEMPS); the curated profile library (LIB); and the weight transform consumed by networks and alignment (WEIGHT). Each evidence kind has its own provenance, review needs and phase: MPE and LIB in P1a, PORT and WEIGHT in P1b for scan and transform work, IEMPS in P2 with ISO/TS 15530-4.

Alternatives considered:
- **One child per device class** (CMM, tracker, arm, scanner). Rejected: the format, range check and weighting would be repeated in each, and MPE derivation spans classes.
- **Range check as its own child.** Rejected: it is a property of the format (declared range axes), so it sits with SCHEMA.
- **WEIGHT inside LVM.** Rejected: `UNC.XFM` also needs it, and the parent requires UNC to export weights.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| UNC.INST.SCHEMA | Instrument model data format, import, binding and range check (leaf) | 001 004 | SA.UNC.01 |
| UNC.INST.MPE | Specification-derived models from ISO 10360 MPE terms | 002 003 | SA.UNC.01 |
| UNC.INST.PORT | Per-observation models for trackers, arms and scanners | 002 005 | SA.UNC.01 |
| UNC.INST.IEMPS | Instrument Error Model Parameter Sets for virtual-CMM simulation | 007 | SA.UNC.01 |
| UNC.INST.LIB | Instrument-profile library | 002 008 | SA.UNC.01 |
| UNC.INST.WEIGHT | Covariance and weight export for networks and transforms (leaf) | 006 | none |
