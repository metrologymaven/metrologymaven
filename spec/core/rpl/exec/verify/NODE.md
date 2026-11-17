---
id: CORE.RPL.EXEC.VERIFY
title: Verifier worker sandbox, kernel lookup and memo bypass
level: 4
parent: CORE.RPL.EXEC
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
satisfies: [REQ-CORE.RPL.EXEC-004, REQ-CORE.RPL.EXEC-005]
covers: []
depends_on: [CORE.RPL.EXEC.LOOP, CORE.STO.CACHE]
uses_artifacts: [KRN.MAN.WASM]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# CORE.RPL.EXEC.VERIFY: Verifier worker sandbox, kernel lookup and memo bypass

## Purpose

Runs a verifier replay in a separate worker process that can read the project but cannot change it, cannot reach the network, never reads the memo cache, and computes every value with a kernel artifact chosen by digest (the recorded one or a named newer one). It implements D-020 option C locally: bundled artifacts for the current major, an artifact carried in the project or package, or an artifact in the user's local kernel cache; the source is reported. Cross-version diff reuses this worker once per kernel.

## Scope

**In:** worker process launch and IPC, read-only project open, cache handle in disabled mode, bus refusal of X1/D1/M1 execution inside the worker, OS-level sandbox where available, `KernelSource` with the three local sources and SHA-256 check, reporting the artifact source.
**Out:** filling the local kernel cache by download (an opt-in X1 through `egress.send@1`, owned by `PLAT` and `ECO.REL`); the verifier binary `fid-verify` and its own WASM sandbox (`TRUST.REPLAY.SANDBOX`); classification and report (`CORE.RPL.CLASS`, `CORE.RPL.EXEC.REPORT`).

## Requirements

- **REQ-CORE.RPL.EXEC.VERIFY-001** When a verifier replay starts, the engine shall run the span in a worker process that opens the project database and blob store read-only.
- **REQ-CORE.RPL.EXEC.VERIFY-002** The worker shall hold its cache handle in disabled mode, so every memo lookup misses and every step is recomputed.
- **REQ-CORE.RPL.EXEC.VERIFY-003** The worker's bus shall refuse to execute any X1, D1 or M1 handler; such steps shall be served from recorded observations or reported `NOT_REPLAYABLE`.
- **REQ-CORE.RPL.EXEC.VERIFY-004** Where the platform offers it, the worker shall run with outbound network denied by the operating system (network namespace on Linux; equivalent mechanisms on Windows and macOS [unverified which]), and the report shall state whether OS-level denial was active.
- **REQ-CORE.RPL.EXEC.VERIFY-005** The `KernelSource` shall look a kernel up by manifest digest in this order: artifacts bundled with the application, artifacts carried in the project, then the user's local kernel cache, and shall load an artifact only if its SHA-256 equals the digest in the kernel manifest.
- **REQ-CORE.RPL.EXEC.VERIFY-006** The worker shall report, per kernel used, the kernel manifest digest and the source it came from (`bundled`, `project`, `local_cache`).
- **REQ-CORE.RPL.EXEC.VERIFY-007** If the worker exits abnormally or exceeds its memory or time budget, then the engine shall report the affected steps `NOT_REPLAYABLE(resource)` and leave the project unchanged.

## Acceptance criteria

- **AC-1** Given fixture project `fixtures/core/rpl/exec/verify/session_50` with a warm memo cache, when a verifier replay runs, then the cache reports zero hits and the handler call counter equals the number of W-class steps.
- **AC-2** Given the same project, when the worker tries to open the database for writing (fault-injected), then the open fails and the project's file hashes are unchanged after the run.
- **AC-3** Given a span containing an X1 fixture command, when replayed, then the X1 handler is not called and the step is served from its recorded observation.
- **AC-4** On Linux CI inside a network namespace probe, when the worker attempts a connection (fault-injected), then the attempt fails and the report says OS-level denial was active.
- **AC-5** (error path) Given a local-cache artifact whose bytes do not match the manifest digest, then it is not loaded and the kernel is reported missing.
- **AC-6** (error path) Given a worker killed mid-run, then the remaining steps are `NOT_REPLAYABLE(resource)`.

## Design

- **Registry command(s):** none; library plus the worker entry point `fid replay-worker` (internal, not a registry command, not on any surface).
- **Data structures:** `WorkerSpec { project: ReadOnlyHandle, span, kernel_digest, budgets }`; `KernelSource` (also used by SUBST pre-flight); `ArtifactOrigin { Bundled, Project, LocalCache }`; IPC messages in canonical JSON over stdin/stdout, length-prefixed, size-capped.
- **Algorithm:** spawn worker with a minimal environment; worker opens storage read-only (SQLite read-only and immutable flags [unverified exact flags]), builds a bus with an execution policy that refuses X1/D1/M1, runs LOOP, streams `StepComparison`s back.
- **Errors:** `kernel_unavailable`, `worker_failed`, `budget_exceeded`.
- **Determinism:** D0 for orchestration; step results per their class.
- **Performance budget:** worker start < 500 ms [unverified until measured].
- **Crate:** `fid-replay` (module `verify`) and a thin worker binary target in `fid` (L1 core binary).

## Test fixtures

`fixtures/core/rpl/exec/verify/` synthetic projects; a tampered kernel artifact fixture; the bundled reference kernel from `KRN.MAN.WASM` CI output. CC-BY-4.0.

## Build prompt

> In crate `fid-replay`, add module `verify`: run a span in a child process (`fid replay-worker`) that opens the project read-only, uses `fid-store` cache in disabled mode, and builds a bus whose policy refuses X1, D1 and M1 handlers (serve recorded observations instead). Apply OS network denial where available and report whether it was active. Implement `KernelSource` per D-020 option C, locally only: bundled, then project-carried, then local kernel cache; load only if SHA-256 matches the kernel manifest; report the origin. Abnormal exit or budget overrun gives `NOT_REPLAYABLE(resource)`. IPC is size-capped canonical JSON. Tests AC-1..AC-6 with fixtures in `fixtures/core/rpl/exec/verify/`. Constraints: CONSTITUTION C7 (no network), REQ "verifier replay never uses the memo cache", §8 (no panic, no `unsafe`), never download anything.

## Open questions

- Exact OS mechanisms for network denial on Windows and macOS are [unverified]; AC-4 runs on Linux only until `PLAT` names them.
- The kernel artifact layout inside a project (where a carried artifact lives) must match `CORE.STO.LAYOUT`'s fixed paths; this leaf assumes `kernels/<digest>.wasm` and LAYOUT reserves it.

## Children

_Leaf: none._
