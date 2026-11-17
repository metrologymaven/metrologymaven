---
id: KRN.MAN.XART
title: Cross-host and cross-artifact bit-identity gate
level: 3
parent: KRN.MAN
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
satisfies: [REQ-KRN.MAN-006, REQ-KRN.MAN-007, REQ-KRN.MAN-010]
covers: []
depends_on: [KRN.MAN.WASM, KRN.MAN.KCI]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# KRN.MAN.XART: Cross-host and cross-artifact bit-identity gate

## Purpose

Measures and enforces "same input, same bits". It runs the frozen reference call set through the WASM artifact on every supported host and through each native performance artifact, compares output digests, publishes the comparison, fails CI when the WASM artifact differs across hosts, and fails a patch release whose outputs differ from the previous release.

## Scope

**In:** a CI tool `fid-xart` that runs a call set through an artifact and writes a digest table; the host matrix (Windows, Linux, macOS on x86-64 and ARM64 where runners exist); WASM-versus-host gate; native-versus-WASM measurement (reported, not gating, while `native_gate = measured-only`); patch-release gate against the previous release's digest table; a machine-readable report.
**Out:** the reference call set content (`UNC.RDL`, consumed at test time only, no build dependency); pass or fail against reference values (`KRN.MAN.VAL`); changing `native_gate` (a steering-group phase decision).

## Requirements

- **REQ-KRN.MAN.XART-001** For every release candidate, the tool shall run the frozen reference call set through the WASM artifact on each host in the matrix and fail CI if any output digest differs between hosts.
- **REQ-KRN.MAN.XART-002** For every release candidate, the tool shall run the same call set through each native artifact and publish, per artifact and per algorithm, the count of calls whose output digest equals the WASM digest.
- **REQ-KRN.MAN.XART-003** When the release is a patch release, the tool shall compare the WASM digest table with the previous release's table and fail if any digest differs.
- **REQ-KRN.MAN.XART-004** The report shall be generated as canonical JSON, include the call-set digest and every artifact hash, and never be hand-edited (CI regenerates and compares).
- **REQ-KRN.MAN.XART-005** If a host in the matrix is unavailable, then the report shall list it as `not_run` and the gate shall fail unless the host is marked optional in the matrix file.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/man/xart-calls-v1` (a synthetic call set over the test algorithms of `KRN.MAN.KCI`, standing in for the frozen reference set until `UNC.RDL` provides it), when the tool runs on the matrix, then the report lists identical WASM digests on every host and exits 0.
- **AC-2** Given a test native artifact built with a deliberately different summation order for one test algorithm, then the report shows the mismatch count for that algorithm and the job still exits 0 (measured only).
- **AC-3** (error path) Given a previous-release digest table that differs in one entry and a release marked patch, then the job exits non-zero naming the call and algorithm.
- **AC-4** Given the same inputs twice, then the report bytes are identical.

## Design

- **Registry command(s):** none.
- **Data structures:** `xart-matrix.toml` (hosts, runner labels, optional flags); `DigestTable { call_set_digest, artifact_sha256, rows: Vec<(call_index, algorithm, output_digest)> }`; `XartReport`.
- **Algorithm:** run, tabulate, compare; canonical JSON via `fid-kci`.
- **Errors:** CI failure with the first differing call and algorithm.
- **Determinism:** D0 for the report given its inputs.
- **Performance budget:** full matrix < 30 minutes [unverified until measured].
- **Crate:** `fid-xart` (CI tool; outside the kernel graph).

## Test fixtures

`fixtures/man/xart-calls-v1`: synthetic calls, CC-BY-4.0. The frozen reference call set from `UNC.RDL` replaces it in release CI.

## Build prompt

> Create CI tool crate `fid-xart` (outside the kernel graph). It runs a canonical call set through an artifact (the `fid-kernel-wasm` module under the pinned wasmtime configuration, or a native `fid-kernel` build), writes a `DigestTable` (call-set digest, artifact SHA-256, per-call algorithm and output digest) as canonical JSON (use `fid-kci`), and compares tables: fail if WASM digests differ across hosts in `xart-matrix.toml`; report (without failing) native-vs-WASM match counts per artifact and algorithm; on patch releases, fail if any WASM digest differs from the previous release's table; list unavailable hosts as `not_run` and fail unless marked optional. Add the CI workflow for Windows, Linux and macOS on x86-64 and ARM64 runners. Tests: `fixtures/man/xart-calls-v1` identical across hosts, a deliberately divergent native test artifact (reported, not failing), patch-release failure, report byte stability. Deny `unwrap`/`expect`/`panic`. Acceptance: AC-1..AC-4 of spec node KRN.MAN.XART.

## Open questions

- Availability of hosted ARM64 runners for Windows [unverified]; mark that host optional until one exists.

## Children

_Leaf: none._
