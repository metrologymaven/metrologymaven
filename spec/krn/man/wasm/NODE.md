---
id: KRN.MAN.WASM
title: WASM reference artifact and import check
level: 3
parent: KRN.MAN
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
satisfies: [REQ-KRN.MAN-005]
covers: []
depends_on: [KRN.MAN.KCI]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# KRN.MAN.WASM: WASM reference artifact and import check

## Purpose

The one byte-identical kernel binary that runs on every platform and computes values of record in P0 and P1a. It is a WebAssembly command module that reads one canonical `KernelCall` from standard input, calls `evaluate`, and writes the `KernelResult` to standard output. The I/O shim is a separate crate outside the kernel graph, so the kernel stays free of I/O and `unsafe`. CI checks that the module imports nothing beyond an allowlisted minimal WASI set and runs it under a pinned runtime configuration.

## Scope

**In:** the shim crate `fid-kernel-wasm` (binary, target `wasm32-wasip1`); the import allowlist file; the import-section checker (a CI tool); the pinned wasmtime configuration (NaN canonicalisation on, relaxed SIMD off, threads off, fixed stack and memory limits) recorded as data; a smoke test running sample calls.
**Out:** the assembly of the algorithm table (`fid-kernel` crate, filled by generated registration code); reproducible-build settings (`KRN.MAN.REPRO`); cross-host comparisons (`KRN.MAN.XART`); verifier replay using the artifact (`TRUST`).

## Requirements

- **REQ-KRN.MAN.WASM-001** The build shall produce a `wasm32-wasip1` command module from `fid-kernel-wasm` whose `main` reads one canonical call from stdin, calls `fid_kci::evaluate` with the assembled table, writes the canonical result or error to stdout, and exits 0 on success and 1 on a kernel error.
- **REQ-KRN.MAN.WASM-002** The shim crate shall contain no `unsafe` code and no logic other than reading, calling and writing.
- **REQ-KRN.MAN.WASM-003** The import checker shall list every import of the module and fail CI if any import is not in the allowlist file (initially `fd_read`, `fd_write`, `proc_exit` and the minimum others the Rust standard library requires for a command, each justified in the file).
- **REQ-KRN.MAN.WASM-004** The runtime configuration (wasmtime version, NaN canonicalisation, relaxed SIMD, threads, stack and memory limits) shall be stored as a data file whose SHA-256 is the `replay_runtime.config_digest`.
- **REQ-KRN.MAN.WASM-005** If the module exhausts the configured stack or memory, then the runner shall report `NOT_REPLAYABLE(resource)` rather than a result.

## Acceptance criteria

- **AC-1** Given the built module, when the import checker runs, then it lists only allowlisted imports and exits 0; given a test module that imports `random_get`, then it exits non-zero naming the import.
- **AC-2** Given fixture `fixtures/man/kci-calls-v1` (from `KRN.MAN.KCI`), when each call is piped through the module under the pinned wasmtime configuration on Linux x86-64, then every stdout equals the golden result bytes.
- **AC-3** Given a call to a test algorithm that recurses past the configured stack limit, then the runner reports `NOT_REPLAYABLE(resource)`.
- **AC-4** Given the shim source, when scanned, then it contains no `unsafe` and no call into the kernel other than `evaluate`.

## Design

- **Registry command(s):** none.
- **Data structures:** `wasm-imports.allow` (one import per line with justification); `wasm-runtime.toml` (pinned runtime settings).
- **Algorithm:** read all of stdin with a fixed maximum size (for example 1 GiB [unverified]), parse the canonical call, evaluate, write. The import checker parses the module's import section with an allowlisted pure-Rust wasm parser in a CI tool crate.
- **Errors:** shim exit codes 0, 1 (kernel error, result on stdout), 2 (malformed or oversized input); runner-level `NOT_REPLAYABLE(resource)`.
- **Determinism:** D0.
- **Performance budget:** start-up < 50 ms per call under wasmtime with a cached compiled module [unverified until measured].
- **Crate:** `fid-kernel-wasm` (shim) and the CI tool `fid-wasm-check`.

## Test fixtures

`fixtures/man/kci-calls-v1` (shared with `KRN.MAN.KCI`), CC-BY-4.0; a test-only module importing `random_get`.

## Build prompt

> Create binary crate `fid-kernel-wasm` targeting `wasm32-wasip1`: `main` reads stdin (bounded size), parses one canonical `KernelCall`, calls `fid_kci::evaluate` with the algorithm table and card registry from the `fid-kernel` assembly crate, writes the canonical `KernelResult` or error to stdout, and exits 0, 1 (kernel error) or 2 (bad input). No `unsafe`, no other logic. Create CI tool crate `fid-wasm-check` that lists a module's imports and fails on any not in `wasm-imports.allow` (each allowed import justified in the file). Store the pinned wasmtime settings (version, NaN canonicalisation on, relaxed SIMD off, threads off, stack and memory limits) in `wasm-runtime.toml`; the runner used by CI applies them and maps stack or memory exhaustion to `NOT_REPLAYABLE(resource)`. Tests: import check pass and a failing test module, `fixtures/man/kci-calls-v1` golden outputs through the module, stack-exhaustion mapping, shim scan for `unsafe`. Constitution: kernel crates have no I/O and no `unsafe`; the shim is outside the kernel graph but still has no `unsafe`; deny `unwrap`/`expect`/`panic`. Acceptance: AC-1..AC-4 of spec node KRN.MAN.WASM.

## Open questions

- The concept doc's manifest example uses `wasm32-wasip2` with an empty import list. This leaf uses a `wasip1` command with an allowlisted minimal WASI import set so no `unsafe` export glue is needed; revisit if the steering group wants a strictly empty import section (that would need a named `unsafe` exception for an export shim).

## Children

_Leaf: none._
