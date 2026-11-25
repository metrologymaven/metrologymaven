---
id: HAL.IDL.SENS.GAUENV
title: Gauge@1 and Environment@1 packages
level: 4
parent: HAL.IDL.SENS
kind: task
status: leaf
phase: P2
size: M
owners: []
claimed_by:
built_in: [fid-hal-idl]
built_pr: 618
satisfies: [REQ-HAL.IDL.SENS-001, REQ-HAL.IDL.SENS-004, REQ-HAL.IDL.SENS-005]
covers: []
depends_on: [HAL.IDL.SENS.STREAM, HAL.IDL.BASE, HAL.IDL.UNITS, HAL.IDL.GEN.FMT]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# HAL.IDL.SENS.GAUENV: Gauge@1 and Environment@1 packages

## Purpose

The two simplest sensor packages, and the first live devices on the acquisition path: `Gauge@1` for digital hand gauges and indicators (read one value, stream, zero) and `Environment@1` for temperature, humidity and pressure sensors. Their payloads are what drivers (`HAL.DRV.GAU`, `HAL.DRV.ENV`) and simulators (`HAL.SIM.GAUENV`) produce.

## Scope

**In:**
- `Gauge@1`: `read` (D1), `zero` (D1), streaming through the template; `GaugeReading { value, resolution, gauge_identity, calibration_ref, t_dev, t_host }`
- `Environment@1`: `subscribe` through the template; `EnvReading { temperature, temperature_u, humidity, pressure, sampling_time, sensor_identity, t_dev, t_host }`
- Conformance profiles `Gauge-core@1` and `Environment-core@1`

**Out:**
- Gauge register and binding readings to a register entry (owned by `UNC.INST.LIB`; binding at runtime by `HAL.DRV.GAU`)
- Manual typed readings (owned by `RPT.BALLOON.ENTRY`)
- Thermal compensation (owned by `KRN` and `UNC`)

## Requirements

- **REQ-HAL.IDL.SENS.GAUENV-001** A `GaugeReading` shall carry the value as an SI `Quantity`, the resolution as a `Quantity` of the same unit, the gauge identity as `UntrustedText`, an optional calibration reference, an optional `t_dev` and a required `t_host`.
- **REQ-HAL.IDL.SENS.GAUENV-002** If a gauge reading's value is not an integer multiple of its resolution within 1e-9 relative, then validation shall fail with `reading_not_quantised`.
- **REQ-HAL.IDL.SENS.GAUENV-003** An `EnvReading` shall carry temperature in K with its stated standard uncertainty in K, a sampling time in s, and optional humidity (dimensionless fraction in [0, 1]) and pressure (Pa).
- **REQ-HAL.IDL.SENS.GAUENV-004** `read` and `zero` shall be class D1; `Gauge@1` shall define no operation that changes calibration.
- **REQ-HAL.IDL.SENS.GAUENV-005** `Gauge-core@1` shall check quantisation, unit consistency, zero behaviour and timestamp monotonicity; `Environment-core@1` shall check units, uncertainty presence and sampling-time consistency.

## Acceptance criteria

- **AC-1** Given `fixtures/hal/idl/sens/gauenv/caliper_read.json` (value 0.01234 m, resolution 1e-5 m), when validated, then it passes and round-trips identically.
- **AC-2** (error path) Given value 0.012345 m with resolution 1e-5 m, then validation fails with `reading_not_quantised`.
- **AC-3** (error path) Given humidity 1.3, then validation fails with `value_out_of_range`.
- **AC-4** Given `fixtures/hal/idl/sens/gauenv/room_env.json` (293.15 K, u 0.05 K), then it passes.
- **AC-5** Given both package exports, then they validate as `capability-package/v1`, include the streaming operations and list their profiles.

## Design

- **Registry commands:** none; library packages. `device.read_gauge@1` is defined by `HAL.DRV.GAU`.
- **Data structures:** `GaugeReading`, `EnvReading`, `GaugeInfo { kind, range, resolution }` in describe.
- **Algorithm:** quantisation check `|value/resolution - round(value/resolution)| <= 1e-9 * max(1, |value/resolution|)` (division and rounding without FMA).
- **Errors:** `reading_not_quantised`, `value_out_of_range`, `unit_unsupported`.
- **Determinism:** D0.
- **Performance budget:** trivial.
- **Crate:** `fid-hal-idl` (modules `caps::gauge`, `caps::environment`), layer L0.

## Test fixtures

Project-authored JSON under `fixtures/hal/idl/sens/gauenv/` (CC-BY-4.0).

## Build prompt

> In crate `fid-hal-idl` (L0), modules `caps::gauge` and `caps::environment`, define `Gauge@1` (`read` D1, `zero` D1, streaming via `caps::stream`; `GaugeReading { value: Quantity, resolution: Quantity (same unit), gauge_identity: UntrustedText, calibration_ref: Option<Digest>, t_dev: Option<i64>, t_host: i64 }`; validation `reading_not_quantised` when value is not a multiple of resolution within 1e-9 relative; no calibration-writing operation) and `Environment@1` (subscribe via the streaming template; `EnvReading { temperature: K, temperature_u: K, humidity: Option<fraction 0..1>, pressure: Option<Pa>, sampling_time: s, sensor_identity: UntrustedText, t_dev, t_host }`). Define profiles `Gauge-core@1` and `Environment-core@1`. Implement `CapabilityPackage` and add both to the inventory. No panic, unwrap, expect or unsafe (CONSTITUTION §8). Tests: AC-1..AC-5.

## Open questions

- Linking a reading to a register entry in `UNC.INST.LIB` happens at runtime; the package carries only the device-declared identity, because L0 types may not depend on UNC's register.
- Inventory: `GaugePackage` and `EnvironmentPackage` implement `CapabilityPackage`; adding them to `inventory::packages()` waits for `HAL.IDL.GEN.EXPORT`, which owns the inventory and is not built yet.
- Sampling time is read as the time over which the sensor sampled one reading. `Environment-core@1` (EN-03) checks it is positive, the same in every reading of a stream, and equal to the optional core setting `sampling_time` when the device has one. It does not check it against the spacing of the timestamps, because averaging windows may overlap.

## Children

_Leaf: none._
