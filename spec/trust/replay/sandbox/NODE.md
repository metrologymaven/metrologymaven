---
id: TRUST.REPLAY.SANDBOX
title: WASM kernel sandbox with digest check and limits
level: 3
parent: TRUST.REPLAY
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-TRUST.REPLAY-007, REQ-TRUST.REPLAY-009]
covers: []
depends_on: []
uses_artifacts: [KRN.MAN.WASM, KRN.MAN.KCI, KRN.MAN.MANI.SCHEMA]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# TRUST.REPLAY.SANDBOX: WASM kernel sandbox with digest check and limits

## Purpose

Runs the WASM reference kernel inside the verifier with nothing but the kernel call interface: no filesystem, network, clock or environment, a memory cap and an execution budget. It refuses to load any artifact whose SHA-256 differs from the kernel manifest. It is the only place the verifier executes code that came from outside its own binary.

## Scope

**In:**
- Artifact hash check against the kernel manifest's artifact entry
- Import-section check: only the allowlisted imports from KRN.MAN, all provided as deterministic host stubs
- An interpreting WASM runtime with fuel metering (deterministic budget) and a host wall-clock watchdog
- Memory cap and per-call limits
- Calling the kernel entry point (REQ-KRN.MAN-001) with algorithm ID, params, seed and inputs, returning the result envelope bytes

**Out:**
- Which calls to make (`TRUST.REPLAY.SELECT`)
- Where artifacts come from (`TRUST.REPLAY.KART`)
- Building the artifact (`KRN.MAN`)

## Requirements

- **REQ-TRUST.REPLAY.SANDBOX-001** The sandbox shall compute the SHA-256 of the artifact bytes and shall refuse to instantiate it with `sandbox.artifact_mismatch` if it differs from the kernel manifest entry for target wasm32.
- **REQ-TRUST.REPLAY.SANDBOX-002** If the module imports anything outside the allowlist published in the kernel manifest, then the sandbox shall refuse it with `sandbox.forbidden_import`.
- **REQ-TRUST.REPLAY.SANDBOX-003** The sandbox shall provide no WASI or host function that reads files, sockets, clocks, randomness or environment variables.
- **REQ-TRUST.REPLAY.SANDBOX-004** The sandbox shall cap linear memory at a caller-set limit, default 4 GiB, and return `sandbox.memory_limit` when a call exceeds it [unverified default].
- **REQ-TRUST.REPLAY.SANDBOX-005** The sandbox shall enforce a fuel budget per call and a caller-set wall-clock budget per package, returning `sandbox.budget_exhausted` without partial results when either runs out.
- **REQ-TRUST.REPLAY.SANDBOX-006** A trap inside the module shall be returned as `sandbox.trap` with the trap kind, and shall never abort the verifier process.
- **REQ-TRUST.REPLAY.SANDBOX-007** Running the same call twice in the sandbox shall return byte-identical result envelopes.

## Acceptance criteria

- **AC-1** Given test kernel `fixtures/replay/sandbox/kernel-test.wasm` and its manifest, when `fit.circle.lsq@1` is called on `fixtures/replay/sandbox/circle-100.bin`, then the result envelope digest equals the pinned value from the KRN.MAN validation run.
- **AC-2** (error path) Given the same artifact with one byte changed, then `sandbox.artifact_mismatch` and no instantiation.
- **AC-3** (error path) Given `fixtures/replay/sandbox/imports-fd.wasm` importing a WASI file function, then `sandbox.forbidden_import`.
- **AC-4** (error path) Given `fixtures/replay/sandbox/loop.wasm` (infinite loop) and `grow.wasm` (unbounded memory growth), then `sandbox.budget_exhausted` and `sandbox.memory_limit` respectively, within 10 s, with the process still alive.
- **AC-5** Given AC-1 run twice and on Windows, Linux and macOS CI, then all result digests are equal.
- **AC-H1** Hypothesis: the sandboxed WASM kernel achieves byte-identical result envelopes on 100 % of kernel calls generated from the frozen PTB TraCIM and NIST reference point sets at the versions pinned by KRN.MAN.VAL, across the six CI host targets (Windows, Linux, macOS × x86-64, arm64), measured by running every KRN.MAN.VAL call through the sandbox on each target and comparing envelope digests with the pinned digests of the KRN.MAN.VAL report for the same kernel version; falsified if any envelope digest differs on any target.
- **AC-H2** Hypothesis: the sandbox achieves 0 host-resource accesses (file, socket, clock, randomness, environment) and 0 verifier-process aborts on the hostile-module set `fixtures/replay/sandbox/*.wasm` (version pinned with the corpus), with every budget or memory exhaustion reported within 10 s, measured by running each module under a syscall audit on the six CI host targets (Windows, Linux, macOS × x86-64, arm64); falsified if any access is observed, the process dies, or a limit error arrives late.

## Design

- **Registry command(s):** none.
- **Data structures:** `Sandbox { engine, limits: Limits }`; `Limits { memory_bytes, fuel_per_call, wall_clock }`; `CallRequest`, `CallResult(Vec<u8>)`.
- **Algorithm:** hash → parse module → check imports → instantiate with stub imports → copy inputs into guest memory through the kernel's allocator export → call entry → copy result out. Use an interpreter (for example `wasmi`) rather than a JIT so behaviour is identical across hosts and builds for wasm32 itself (verifier-in-browser) [unverified: runtime choice confirmed at build time against KRN.MAN's pinned runtime configuration].
- **Errors:** `sandbox.artifact_mismatch`, `sandbox.forbidden_import`, `sandbox.memory_limit`, `sandbox.budget_exhausted`, `sandbox.trap`.
- **Determinism:** D0 (interpreted, fuel-metered; the wall-clock watchdog only aborts, never changes results).
- **Performance budget:** a 100,000-point least-squares cylinder fit in under 5 s interpreted [unverified until measured].
- **Crate:** `fid-vreplay` (module `sandbox`).

## Test fixtures

- `fixtures/replay/sandbox/*.wasm`: tiny hand-written test modules (WAT sources committed) plus a test kernel build from KRN.MAN; synthetic inputs generated with a fixed seed.

## Build prompt

> In crate `fid-vreplay`, add module `sandbox`: run the WASM reference kernel with an interpreting, fuel-metered runtime (pure Rust, builds for wasm32 too). Before instantiation, check the artifact's SHA-256 against the kernel manifest (`sandbox.artifact_mismatch`) and the import section against the manifest's allowlist (`sandbox.forbidden_import`); provide no host function for files, sockets, clocks, randomness or environment. Limits: memory cap (default 4 GiB), fuel per call, wall-clock per package (`sandbox.memory_limit`, `sandbox.budget_exhausted`); traps become `sandbox.trap` and never abort the process. Call the kernel's single entry point (algorithm ID, params, seed, inputs with digests) and return the result envelope bytes. Tests: the five acceptance criteria with WAT test modules and a KRN.MAN test kernel. Constraints: CONSTITUTION §8 (no panics, no unsafe in this crate), C2/C1 (values only from the reference kernel), D0.

## Open questions

- The runtime and its version must match KRN.MAN's "pinned runtime configuration"; if KRN pins a different engine, follow KRN.

## Children

_Leaf: none._
