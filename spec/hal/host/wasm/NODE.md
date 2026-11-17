---
id: HAL.HOST.WASM
title: Tier-3 WASM component sandbox for readers and sensor models
level: 3
parent: HAL.HOST
kind: module
status: open
phase: P3
size: L
owners: []
claimed_by:
satisfies: [REQ-HAL.HOST-003, REQ-HAL.HOST-004]
covers: []
depends_on: [HAL.IDL.GEN, HAL.HOST.PKG, HAL.ACQ.FILEDEV]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# HAL.HOST.WASM: Tier-3 WASM component sandbox for readers and sensor models

## Purpose

Runs third-party and vendor-supplied format readers and sensor error models as sandboxed WebAssembly components with no device I/O. Their output is a pure function of input bytes (D0), which makes them safe to load from outside the project and replayable. It is separate because the sandbox is a distinct isolation mechanism with its own limits and threat model.

## Scope

**In:**
- The component worlds: `reader` (sniff, open, metadata, next-batch) and `sensor-model` (pure evaluation)
- Sandbox: no filesystem, network, clock or random-source imports; read-only byte-stream input; structured log output
- Fuel and memory limits; deterministic execution settings
- Load-time signature and digest check against the allowlist
- Reader output handed to the file pseudo-device path with the component digest in provenance

**Out** (and which node owns it instead):
- First-party format readers (owned by `IO`)
- Clean-room provenance records for independently implemented proprietary readers (owned by `IO` and `ECO`)
- Uncertainty evaluation that calls sensor models (owned by `UNC`)

## Requirements

- **REQ-HAL.HOST.WASM-001** A Tier-3 component shall be instantiated with no filesystem, network, clock or random-source capability, and a component that imports any other host function shall be refused at load with `component_import_denied`.
- **REQ-HAL.HOST.WASM-002** Every call into a component shall run within a fuel limit and a memory limit set per package, and exhausting either shall return `component_limit_exceeded` without affecting the engine.
- **REQ-HAL.HOST.WASM-003** Given the same component digest and input bytes, a component's output bytes shall be identical on every supported platform (D0).
- **REQ-HAL.HOST.WASM-004** Before loading a component, the runtime shall verify the package signature and component digest against the allowlist and refuse with `driver_not_allowlisted` on mismatch.
- **REQ-HAL.HOST.WASM-005** Observations produced by a reader component shall carry the component digest in their provenance stamp and trust no higher than `declared`.

## Interfaces

- **Offers:** `reader@1` and `sensor-model@1` worlds; component runner used by `IO` import and `UNC` evaluation.
- **Consumes:** `HAL.IDL.GEN` (WIT output); `HAL.HOST.PKG` allowlist; `HAL.ACQ.FILEDEV` stamping path.

## Decomposition hints

WORLD (WIT worlds and versioning), RUN (sandbox runner, limits, determinism settings), LOAD (allowlist check), READER (adapter into the file pseudo-device), MODEL (sensor-model adapter for UNC).

## Open questions

- Floating-point NaN canonicalization and SIMD in the WASM runtime must be configured for D0; the runtime choice (concept doc 08 names wasmtime) is a leaf decision [unverified].

## Children

_None yet: status `open`._
