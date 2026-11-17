---
id: HAL.DRV.ENV
title: Environment-sensor driver (Modbus and serial temperature probes)
level: 3
parent: HAL.DRV
kind: feature
status: leaf
phase: P2
size: M
owners: []
claimed_by:
satisfies: [REQ-HAL.DRV-001, REQ-HAL.DRV-003, REQ-HAL.DRV-008]
covers: [PCD.MCH.03, DEV.ENV.01]
depends_on: [HAL.IDL.SENS, HAL.HOST.TIER1, HAL.SIM.GAUENV, HAL.SIM.CONF]
side_effect: D1
determinism: N
cli: [fid device read-environment]
mcp: [device_read_environment]
journal: [device.read_environment@1]
updated: 2026-11-16
---

# HAL.DRV.ENV: Environment-sensor driver (Modbus and serial temperature probes)

## Purpose

A Tier-1 `Environment` driver for temperature (and optionally humidity and pressure) sensors reachable over open protocols: Modbus RTU, Modbus TCP and line-based serial. A register map per sensor model is data, not code. These readings feed part and machine temperature compensation, which `CMM` and `UNC` own; this leaf supplies stamped observations in kelvin with uncertainty.

## Scope

**In:**
- Modbus RTU and TCP polling, and line-based serial (ASCII) sensors
- Sensor descriptor (data): address, register or line pattern, numeric type, scale, offset, unit, stated standard uncertainty, channel names
- Conversion to kelvin through the declared encoding path (raw values plus descriptor; the runtime converts)
- `device.read_environment@1` (D1): read once or subscribe at a rate ≥ 1 Hz
- Catalog entry for DEV.ENV.01

**Out:**
- Temperature compensation of results (owned by `CMM` and `UNC`)
- Tracker weather stations (owned by `HAL.DRV.TRK`)
- Building-management integrations (not planned)

## Requirements

- **REQ-HAL.DRV.ENV-001** The driver shall report temperature in kelvin with the sensor's stated standard uncertainty and the sampling time of each reading.
- **REQ-HAL.DRV.ENV-002** While subscribed, the driver shall deliver at least one reading per second per configured channel, and any missed poll shall be recorded as a gap.
- **REQ-HAL.DRV.ENV-003** The driver shall send raw register or line values with a declared encoding, so conversion happens in the runtime and not in the driver.
- **REQ-HAL.DRV.ENV-004** If a sensor descriptor is malformed or names a unit outside the SI set, then session open shall fail with `CONFIG_INVALID`.
- **REQ-HAL.DRV.ENV-005** If a Modbus exception or serial parse error occurs, then the driver shall return `TRANSIENT` for that poll, retry with the same idempotency key, and mark the session `INTERRUPTED` after 5 consecutive failures.
- **REQ-HAL.DRV.ENV-006** The driver shall pass `Environment-core@1` against the environment simulator, and `device.read_environment@1` shall be exposed on MCP only as a request.

## Acceptance criteria

- **AC-1** Given the Modbus TCP test server `fixtures/hal/drv/env/modbus_server` with map `pt100_4ch.json` (4 channels, int16, scale 0.01 °C as K offset 273.15), when `fid device read-environment --subscribe --rate 2 --json` runs for 60 s, then ≥ 480 readings arrive, each in K with uncertainty and sampling time, and values match the server's script within the scale step.
- **AC-2** Given the serial line fixture `fixtures/hal/drv/env/serial_ascii.log` replayed on a virtual serial port, when read once, then the reading equals 293.15 K ± 0.005 K.
- **AC-3** (error path) Given descriptor `bad_unit.json` (unit `degF`), when the session opens, then it fails with `CONFIG_INVALID` and journals only the failure.
- **AC-4** (error path) Given the Modbus server configured to return exceptions for 6 polls, when subscribed, then 5 `TRANSIENT` retries occur and the session becomes `INTERRUPTED`.
- **AC-5** Given the driver and the environment simulator, when `Environment-core@1` runs, then all tests pass.
- **AC-6** (replay) Given the journal of AC-1 and its transcript, when replayed through transcript playback in exact mode, then the outcome is `IDENTICAL`.

## Design

- **Registry command:** `device.read_environment@1`, class D1 (request-only on MCP); params `device: Handle`, `channels: [String]`, `mode: once|subscribe`, `rate_hz: f64 (≥ 1)`; result: batch references.
- **Data structures:** `EnvSensorDescriptor { transport: ModbusRtu|ModbusTcp|SerialAscii, address, channels: [ChannelMap { register_or_pattern, numeric, scale, offset, unit, u_std }] }`.
- **Algorithm:** fixed-rate polling on a monotonic timer; raw values plus encoding descriptor into the batch sink; retry with idempotency key.
- **Errors:** `CONFIG_INVALID`, `TRANSIENT`, `COMM_LOST`.
- **Determinism:** N (device I/O); replay through transcripts is exact.
- **Performance budget:** 32 channels at 10 Hz with < 1 % CPU on reference hardware [unverified until measured].
- **Crate:** `fid-drv-env`.

## Test fixtures

Project-written Modbus TCP test server and serial log under `fixtures/hal/drv/env/` (Apache-2.0), with invented sensor maps. No vendor documentation is copied.

## Build prompt

> Create crate `fid-drv-env`, a Tier-1 driver implementing `Environment@1` over Modbus RTU, Modbus TCP and line-based serial ASCII, configured by a data descriptor per sensor (transport, address, channels with register or line pattern, numeric type, scale, offset, SI unit, stated standard uncertainty). Send raw values with declared encodings (the runtime converts to K); never convert in the driver. Register command `device.read_environment@1` (D1, request-only on MCP; CLI `fid device read-environment`, MCP `device_read_environment`) with `mode once|subscribe` and `rate_hz ≥ 1`. Errors: malformed descriptor or non-SI unit → `CONFIG_INVALID` at open; poll failure → `TRANSIENT` with same idempotency key; 5 consecutive failures → session `INTERRUPTED`; missed polls recorded as gaps. Use the Tier-1 API from `fid-hal-driver`. Modbus library must be on the licence allowlist. No panic, unwrap, expect or unsafe; parsers bounded (CONSTITUTION §8). Tests: AC-1..AC-6 with fixtures under `fixtures/hal/drv/env/`, plus `Environment-core@1` from the conformance crate.

## Open questions

- Which sensor models to publish maps for first depends on user demand; maps are data and can be contributed without code changes.

## Children

_Leaf: none._
