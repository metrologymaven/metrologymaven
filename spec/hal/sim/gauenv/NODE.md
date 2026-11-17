---
id: HAL.SIM.GAUENV
title: Gauge and environment-sensor simulators
level: 3
parent: HAL.SIM
kind: task
status: leaf
phase: P2
size: M
owners: []
claimed_by:
satisfies: [REQ-HAL.SIM-001, REQ-HAL.SIM-005, REQ-HAL.SIM-008]
covers: []
depends_on: [HAL.IDL.SENS, HAL.SIM.IPPSIM, HAL.SIM.CONF]
side_effect: I1
determinism: D2
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# HAL.SIM.GAUENV: Gauge and environment-sensor simulators

## Purpose

Synthetic `Gauge` and `Environment` sources. Gauges are the cheapest devices that exercise the full acquisition path (session, health, batches, stamp, journal), so these simulators are the first end-to-end test target of the runtime and of the first real gauge and temperature drivers.

## Scope

**In:**
- Gauge simulator: scripted or random true values, resolution quantization, seeded Gaussian noise, zero operation, single read and streaming
- Environment simulator: temperature (K) with a linear-plus-sinusoidal drift model and seeded noise, optional humidity and pressure, 1 Hz default sampling
- Failure modes: battery-low event, disconnect, out-of-range reading
- Registration with the simulator launcher (`device.start_sim@1 --kind gauge|environment`) and declaration as simulated

**Out:**
- The launcher commands (owned by `HAL.SIM.IPPSIM`)
- Gauge and environment drivers for real hardware (owned by `HAL.DRV.GAU` and `HAL.DRV.ENV`)
- Conformance suites (owned by `HAL.SIM.CONF`)

## Requirements

- **REQ-HAL.SIM.GAUENV-001** The gauge simulator shall implement `Gauge@1`, quantize each reading to its configured resolution, and pass `Gauge-core@1`.
- **REQ-HAL.SIM.GAUENV-002** The environment simulator shall implement `Environment@1`, report temperature in kelvin with a stated standard uncertainty and sampling time, and pass `Environment-core@1`.
- **REQ-HAL.SIM.GAUENV-003** Given the same seed and script, both simulators shall produce bit-identical reading sequences on the same platform (D2).
- **REQ-HAL.SIM.GAUENV-004** Both simulators shall declare themselves simulated, so every resulting stamp carries `simulated = true`.
- **REQ-HAL.SIM.GAUENV-005** The failure modes battery-low, disconnect and out-of-range shall be schedulable by reading count and shall surface as the matching `Device@1` events and error codes.

## Acceptance criteria

- **AC-1** Given `fixtures/hal/sim/gauenv/caliper.json` (true values 10.000–10.010 mm, resolution 0.01 mm, σ 0.002 mm, seed 42), when 1 000 readings are taken through a runtime session, then every value is a multiple of 1e-5 m, the sequence digest equals `caliper.sha256`, and every stamp has `simulated: true`.
- **AC-2** Given `fixtures/hal/sim/gauenv/room.json` (293.15 K, drift 0.5 K/h, seed 7), when 3 600 samples at 1 Hz are read, then the mean slope is 0.5 ± 0.05 K/h and each reading carries uncertainty and sampling time.
- **AC-3** Given each simulator, when `HAL.SIM.CONF` runs `Gauge-core@1` and `Environment-core@1`, then all tests pass.
- **AC-4** Given `caliper.json` with `disconnect_after: 100`, when readings are taken, then reading 101 fails with `COMM_LOST` and the session becomes `INTERRUPTED`.
- **AC-5** (replay) Given the journal of AC-1, when replayed against the simulator with the same seed on the same platform, then the outcome is `IDENTICAL`.

## Design

- **Registry commands:** none of its own; started with `device.start_sim@1` (I1) owned by `HAL.SIM.IPPSIM`.
- **Data structures:** `GaugeSimConfig { script | distribution, resolution, sigma, seed, failures }`, `EnvSimConfig { t0_k, drift_k_per_s, amplitude_k, period_s, sigma_k, rate_hz, seed }`.
- **Algorithm:** counter-based RNG from `fid-num` keyed by (seed, reading index); quantization by round-half-to-even to resolution; drift model evaluated at simulated time.
- **Errors:** `COMM_LOST`, `OUT_OF_RANGE`, battery-low as a device event.
- **Determinism:** D2 (seeded; same platform bit-identical).
- **Performance budget:** ≥ 10 000 readings per second per instance [unverified until measured].
- **Crate:** `fid-hal-sim` (modules `gauge`, `environment`).

## Test fixtures

Scenario JSON files under `fixtures/hal/sim/gauenv/` (CC-BY-4.0) with golden sequence digests.

## Build prompt

> In crate `fid-hal-sim`, implement `gauge` and `environment` simulators as device sources registered with the simulator launcher (`device.start_sim@1 --kind gauge|environment`, class I1), each declaring itself simulated. Gauge: implements `Gauge@1` (read, stream, zero), values from a script or distribution, Gaussian noise from the `fid-num` counter-based RNG keyed by (seed, index), quantized to resolution with round-half-to-even. Environment: implements `Environment@1`, temperature in K = t0 + drift·t + amplitude·sin(2πt/period) + noise, with stated standard uncertainty and sampling time, optional humidity and pressure. Schedulable failures by reading count: battery-low event, disconnect (`COMM_LOST`), out-of-range (`OUT_OF_RANGE`). No OS randomness or wall clock in value generation. No panic, unwrap, expect or unsafe (CONSTITUTION §8). Tests: AC-1..AC-5 with fixtures under `fixtures/hal/sim/gauenv/`, running the `Gauge-core@1` and `Environment-core@1` suites from the conformance crate.

## Open questions

- None.

## Children

_Leaf: none._
