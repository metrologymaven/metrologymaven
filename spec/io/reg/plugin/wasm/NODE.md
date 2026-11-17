---
id: IO.REG.PLUGIN.WASM
title: Sandboxed WASM component host for plugins
level: 4
parent: IO.REG.PLUGIN
kind: task
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-IO.REG.PLUGIN-002, REQ-IO.REG.PLUGIN-003]
covers: [GOM.IMP.03]
depends_on: [IO.REG.PLUGIN.ABI, IO.REG.LIMITS]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# IO.REG.PLUGIN.WASM: Sandboxed WASM component host for plugins

## Purpose

Runs format plugins compiled to WebAssembly components inside a sandbox that grants no file system, network, clock or environment access, with a memory cap and a deterministic instruction budget. A trap, a memory overrun or a budget exhaustion fails one import cleanly. This is the default host for third-party readers, including vendor-supplied readers for incumbent project and mesh formats (GOM.IMP.03).

## Scope

**In:**
- Loading a component from an installed package (bytes and digest handed over by the plugin registry)
- Instantiation with only the contract imports; no WASI file, socket, clock or environment imports
- Memory cap (per-instance linear memory limit) and fuel budget derived from the descriptor's requested caps, never above host maxima
- Mapping traps to `reader_crashed`, fuel or memory exhaustion to `limit_exceeded`
- Fresh instance per invocation (no state carried between imports)

**Out:**
- Native plugins (`IO.REG.PLUGIN.NATIVE`)
- Output validation (`IO.REG.PLUGIN.VALID`)
- Install and enable (ECO, A0)

## Requirements

- **REQ-IO.REG.PLUGIN.WASM-001** The host shall instantiate a plugin with only the contract's imports, and loading shall fail with `plugin_import_forbidden` if the component imports anything else.
- **REQ-IO.REG.PLUGIN.WASM-002** If a plugin traps, then the import shall fail with `reader_crashed` and the document shall be unchanged.
- **REQ-IO.REG.PLUGIN.WASM-003** If a plugin exceeds its memory cap or its fuel budget, then the import shall fail with `limit_exceeded` naming `plugin_memory` or `plugin_fuel`.
- **REQ-IO.REG.PLUGIN.WASM-004** Each invocation shall use a new instance, so no data from one import is visible to the next.
- **REQ-IO.REG.PLUGIN.WASM-005** Given the same component, input bytes and options, the host shall return the same records, since the sandbox exposes no clock, randomness or environment.

## Acceptance criteria

- **AC-1** Given `fixtures/io/reg/plugin/wasm/points_ok.wasm` (a sample reader) and `pts.txt`, when imported through `import.file@1`, then the import succeeds and provenance names the plugin.
- **AC-2** (error path) Given `trap.wasm` that executes `unreachable`, then `reader_crashed`, document unchanged.
- **AC-3** (error path) Given `alloc_bomb.wasm` growing memory past the cap, then `limit_exceeded` (`plugin_memory`).
- **AC-4** (error path) Given `spin.wasm` with an infinite loop, then `limit_exceeded` (`plugin_fuel`) within 2 s wall time.
- **AC-5** (error path) Given `wants_fs.wasm` importing a WASI file function, then loading fails `plugin_import_forbidden`.
- **AC-6** Given AC-1 run twice, then both record streams are byte-identical.

## Design

- **Registry commands:** none (host library used through the import and export skeletons).
- **Data structures:** `WasmHost { engine, max_memory, max_fuel }`; `LoadedPlugin { descriptor, component_digest }`.
- **Algorithm:** a WASM runtime with fuel metering and memory limits [unverified: runtime crate choice, its licence on the D-001 allowlist, and its use of `unsafe` behind a named exception per CONSTITUTION §8]; check component imports against the contract list; instantiate per call; stream bytes through the contract's bounded stream.
- **Errors:** `plugin_import_forbidden`, `reader_crashed`, `limit_exceeded`, `plugin_abi_mismatch`.
- **Determinism:** D0 for records (fuel-based limits are deterministic; no wall-clock limit decides outcomes except as a safety net reported as `N`).
- **Performance budget:** sample point reader within 3× native speed [unverified until measured].
- **Crate:** `fid-io-plugin` (module `wasm`).

## Test fixtures

Sample and hostile components built from source in `fixtures/io/reg/plugin/wasm/src/` (MIT OR Apache-2.0), compiled in CI.

## Build prompt

> In crate `fid-io-plugin`, add module `wasm`: a host that loads a WASM component, refuses any import outside the contract (`plugin_import_forbidden`), instantiates a fresh instance per call with a linear-memory cap and fuel budget from the descriptor (capped by host maxima), streams source bytes through the contract, and maps traps to `reader_crashed` and memory or fuel exhaustion to `limit_exceeded` (`plugin_memory`/`plugin_fuel`). No WASI file, socket, clock or environment. Record any `unsafe` in the runtime as a named exception. CONSTITUTION §8: no panics, no unwrap. Tests AC-1..AC-6 with sample and hostile components built in CI.

## Open questions

- First-party readers for undocumented vendor formats (GOM.IMP.03) are not allowed under the clean-room rule; only vendor-supplied or publicly documented plugins run here [unverified per vendor] (carried from IO.REG.PLUGIN).

## Children

_Leaf: none._
