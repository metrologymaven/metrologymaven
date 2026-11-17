---
id: TRUST.REPLAY
title: Verifier replay with the reference kernel
level: 2
parent: TRUST
kind: subsystem
status: open
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-TRUST-003]
covers: []
depends_on: [TRUST.PKG, KRN, UNC, CORE]
cli: [fid verify replay]
mcp: [verify_replay]
journal: []
updated: 2026-11-16
---

# TRUST.REPLAY: Verifier replay with the reference kernel

## Purpose

When a Result Package includes input points, re-run the recorded computations through the WASM build of the reference kernel inside the verifier and classify the outcome. This is the "verifier" replay mode of CONSTITUTION §4, restricted to what a package contains. It is separate from CORE's general replay engine because it must run standalone, sandboxed and without the project, the GUI or any device.

## Scope

**In:**
- Selecting replayable entries: commands whose inputs are all present in the package
- Loading the pinned WASM reference kernel matching the package's kernel manifest
- Running it in a sandbox with no I/O
- Comparing recomputed values, uncertainty statuses and decisions with the packaged ResultRecords
- Outcome classification per result and per package: `IDENTICAL | EQUIVALENT | DIVERGENT | NOT_REPLAYABLE`
- Equivalence tolerances per determinism class
- Reasons for `NOT_REPLAYABLE` (missing points, unknown algorithm version, missing kernel artifact, `RESULTS_ONLY` or `DIRECT_READING` basis)

**Out** (and which node owns it instead):
- The kernel and its WASM build (`KRN`)
- Uncertainty and decision-rule algorithms (`UNC`)
- Replay in the full app (exact, re-targeted, dry-run, cross-version) (`CORE`)
- Package contents (`TRUST.PKG`)
- Report formatting (`TRUST.VERIFY`)

## Requirements

- **REQ-TRUST.REPLAY-001** When a package includes the input points for a result, the replayer shall recompute that result with the WASM reference kernel whose version matches the kernel manifest in the package.
- **REQ-TRUST.REPLAY-002** The replayer shall classify a result `IDENTICAL` when every recomputed value is bit-equal to the packaged value.
- **REQ-TRUST.REPLAY-003** The replayer shall classify a result `EQUIVALENT` only when values differ within the tolerance published for the algorithm's determinism class and the conformance decision is unchanged.
- **REQ-TRUST.REPLAY-004** If a recomputed conformance decision differs from the packaged decision, then the replayer shall classify the result `DIVERGENT` regardless of numeric tolerance.
- **REQ-TRUST.REPLAY-005** If a result's inputs, algorithm version or kernel artifact are unavailable, or its basis is `RESULTS_ONLY` or `DIRECT_READING`, then the replayer shall classify it `NOT_REPLAYABLE` and name the reason.
- **REQ-TRUST.REPLAY-006** The package-level outcome shall be the worst result-level outcome in the order `DIVERGENT` > `NOT_REPLAYABLE` > `EQUIVALENT` > `IDENTICAL`.
- **REQ-TRUST.REPLAY-007** The WASM kernel shall run with no filesystem, network, clock or environment access, and with a memory cap (initial target 4 GiB) and a wall-clock budget set by the caller [unverified].
- **REQ-TRUST.REPLAY-008** A replay of a D0 algorithm shall give `IDENTICAL` on Windows, Linux and macOS x86-64 and arm64 for every package in the replay conformance corpus.
- **REQ-TRUST.REPLAY-009** The replayer shall verify the WASM kernel artifact's SHA-256 against the kernel manifest before executing it, and shall refuse to run a mismatched artifact.

## Interfaces

- **Offers:** `verify.replay` (R1) returning per-result and package outcomes with reasons; a library used by `TRUST.VERIFY`.
- **Consumes:** WASM reference kernel and kernel manifest from `KRN`; decision-rule and uncertainty algorithm IDs from `UNC`; replay outcome definitions from `CORE`; package reader from `TRUST.PKG`.

## Decomposition hints

SELECT (replayable set), SANDBOX (WASM runtime + limits), COMPARE (classification + tolerances), CORPUS (replay conformance packages).

## Open questions

- How the verifier obtains kernel artifacts for older versions offline: bundled, fetched once on opt-in, or included in the package. Bundling keeps C7; including in the package grows packages. Needs a ruling with KRN.
- Tolerances for `EQUIVALENT` per determinism class should come from KRN's published tolerance table; this node references it but does not own it.

## Children

_None yet: status `open`._
