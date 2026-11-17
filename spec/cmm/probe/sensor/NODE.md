---
id: CMM.PROBE.SENSOR
title: Scanning-probe and non-contact sensor configuration
level: 3
parent: CMM.PROBE
kind: feature
status: leaf
phase: P2
size: M
owners: []
claimed_by:
satisfies: [REQ-CMM.PROBE-005]
covers: [PCD.PRB.05, PCD.PRB.06]
depends_on: []
side_effect: W2
determinism: D0
cli: [fid cmm define-sensor, fid cmm show-sensor]
mcp: [cmm_define_sensor, cmm_show_sensor]
journal: [cmm.define_sensor@1]
updated: 2026-11-16
---

# CMM.PROBE.SENSOR: Scanning-probe and non-contact sensor configuration

## Purpose

Defines the sensor attached to a probe configuration: a touch-trigger or analog scanning tactile probe (deflection limits), a laser line sensor (standoff range, line width, depth of field, point spacing, exposure) or a vision sensor (field of view, working distance, pixel size, illumination). All values are bounded and validated at entry, and the sensor digest feeds the probe configuration digest. Strategies use the sensor kind to decide which scan types a tip may run.

## Scope

**In:**
- `SensorConfig` entity (ID, version, digest) with kinds `TOUCH_TRIGGER`, `ANALOG_SCANNING`, `LASER_LINE`, `VISION`
- Parameter bounds and validation per kind
- `cmm.define_sensor@1` (W2) and `cmm.show_sensor@1` (R0)

**Out** (and which node owns it instead):
- Probe build-up and digest composition (owned by `CMM.PROBE.MODEL`, which references the sensor digest)
- Scan strategies and their sensor checks (owned by `CMM.STRAT`)
- Sensor drivers and acquisition (owned by `HAL`)
- Sensor uncertainty models (owned by `UNC`)

## Requirements

- **REQ-CMM.PROBE.SENSOR-001** The system shall model a sensor as one of `TOUCH_TRIGGER`, `ANALOG_SCANNING`, `LASER_LINE` or `VISION`, each with a stable ID, a version and a digest over RFC 8785 canonical JSON of all parameters except the display name.
- **REQ-CMM.PROBE.SENSOR-002** For an `ANALOG_SCANNING` sensor, the system shall require minimum, nominal and maximum deflection in metres with 0 < minimum ≤ nominal ≤ maximum ≤ 0.01 m, and shall reject other values with `SENSOR_PARAM_INVALID`.
- **REQ-CMM.PROBE.SENSOR-003** For a `LASER_LINE` sensor, the system shall require standoff minimum and maximum (0 < min < max ≤ 1 m), line width, depth of field and point spacing (each finite and positive) and exposure time in (0, 1] s, and shall reject other values with `SENSOR_PARAM_INVALID`.
- **REQ-CMM.PROBE.SENSOR-004** For a `VISION` sensor, the system shall require field-of-view width and height, working distance and pixel size (each finite and positive) and an illumination channel list of at most 16 entries, and shall reject other values with `SENSOR_PARAM_INVALID`.
- **REQ-CMM.PROBE.SENSOR-005** The system shall report for every sensor whether it supports continuous contact scanning (`ANALOG_SCANNING` only) as a derived, read-only property.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/cmm/probe/sensor_analog.json`, when `fid cmm define-sensor --file sensor_analog.json --json` runs, then the sensor is created with `supports_contact_scan: true` and the fixture's `expected_digest`.
- **AC-2** Given fixture `fixtures/cmm/probe/sensor_laser_line.json`, when defined, then it is created with `supports_contact_scan: false`.
- **AC-3** (error path) Given fixture `fixtures/cmm/probe/sensor_bad_standoff.json` (min standoff greater than max), when defined, then it fails with `SENSOR_PARAM_INVALID` naming `standoff_min_m` and journals nothing.
- **AC-4** (error path) Given an analog sensor with nominal deflection above maximum, when defined, then it fails with `SENSOR_PARAM_INVALID`.
- **AC-5** (replay) Given the journal from AC-1 and AC-2, when replayed in exact mode, then the outcome is `IDENTICAL`.

## Design

- **Registry commands:** `cmm.define_sensor@1` (W2; params `SensorConfigInput`, `edit_of: Option<SensorId>`; result `{sensor_id, version, digest}`); `cmm.show_sensor@1` (R0).
- **Data structures:** `SensorConfig { id, version, name: UntrustedText, kind: SensorKind, digest }`; `SensorKind = TouchTrigger { trigger_force_n: Option<f64> } | AnalogScanning { deflection_min_m, deflection_nom_m, deflection_max_m } | LaserLine { standoff_min_m, standoff_max_m, line_width_m, depth_of_field_m, point_spacing_m, exposure_s } | Vision { fov_w_m, fov_h_m, working_distance_m, pixel_size_m, illumination: Vec<IlluminationChannel> }`.
- **Algorithm:** per-kind bound checks; digest via JCS.
- **Errors:** `SENSOR_PARAM_INVALID { field }`, `SENSOR_NOT_FOUND`.
- **Determinism:** D0.
- **Crate:** `fid-cmm-probe`.

## Test fixtures

Synthetic sensor definitions in `fixtures/cmm/probe/` (`sensor_analog.json`, `sensor_laser_line.json`, `sensor_bad_standoff.json`), hand-written with generic values that do not reproduce any vendor data sheet, CC-BY-4.0.

## Build prompt

> In the Rust crate `fid-cmm-probe`, implement sensor configuration with commands `cmm.define_sensor@1` (W2, journaled) and `cmm.show_sensor@1` (R0); CLI `fid cmm define-sensor`, `fid cmm show-sensor` with `--json`; MCP `cmm_define_sensor`, `cmm_show_sensor` (generated).
> Kinds: TOUCH_TRIGGER; ANALOG_SCANNING (0 < min ≤ nominal ≤ max ≤ 0.01 m deflection); LASER_LINE (0 < standoff min < max ≤ 1 m, positive line width, depth of field and spacing, exposure in (0, 1] s); VISION (positive FOV, working distance and pixel size, ≤ 16 illumination channels). Invalid values fail with `SENSOR_PARAM_INVALID {field}` and journal nothing. Derived property `supports_contact_scan` is true only for ANALOG_SCANNING. Digest = SHA-256 over RFC 8785 JCS of all fields but the name.
> Acceptance tests: the five criteria in spec node `CMM.PROBE.SENSOR`, fixtures in `fixtures/cmm/probe/`.
> Constraints: no panic, no `unsafe`, no `unwrap`/`expect`; NaN is an error; determinism D0.

## Open questions

- The bounds (0.01 m deflection, 1 m standoff) are sanity limits, not vendor figures [unverified]; a domain expert may tighten them.

## Children

_Leaf: none._
