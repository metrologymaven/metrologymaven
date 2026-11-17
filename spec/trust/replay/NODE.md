---
id: TRUST.REPLAY
title: Verifier replay with the reference kernel
level: 2
parent: TRUST
kind: subsystem
status: split
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-TRUST-003]
covers: []
depends_on: [TRUST.PKG.READER]
uses_artifacts: [KRN.MAN.WASM, KRN.MAN.KCI, KRN.MAN.MANI.SCHEMA, UNC.RDL.REP, CORE.RPL.CLASS]
cli: [fid verify replay]
mcp: [verify_replay]
journal: []
updated: 2026-11-17
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

## Split rationale

The split follows **the replay pipeline**: decide what can be replayed (SELECT), find the kernel (KART), execute it safely (SANDBOX), judge the outcome (COMPARE), and wire them into one command (RUN); the cross-platform evidence set is its own task (CORPUS). The sandbox is isolated because it is the only place the verifier runs external code; artifact sourcing is isolated because it is blocked on a maintainer choice (D-020) and must not hold up the rest.

Alternatives considered:
- **Reuse CORE.RPL's engine directly.** Rejected for the verifier: it must run standalone without the project store or GUI crates (REQ-TRUST.VERIFY-009). CORE.RPL remains the engine for in-app modes; both must agree on outcome definitions (CONSTITUTION §4).
- **Fold KART into SANDBOX.** Rejected: hashing and running an artifact is fixed; where it comes from is policy.

## Open questions

- How the verifier obtains kernel artifacts for older versions offline: opened as D-020; `TRUST.REPLAY.KART` is blocked, the rest builds against the `ArtifactSource` trait.
- Tolerances for `EQUIVALENT` come from the REP-1 replay equivalence-tolerance table owned by `UNC.RDL` (accepted level-2 review); `TRUST.REPLAY.COMPARE` consumes it and records its digest. UNC.RDL has not yet published the table schema.
- CORE owns the replay engine for all in-app modes; this subsystem is the standalone verifier-mode replayer. Boundary to be confirmed with CORE.RPL.
- Boundary with CORE.RPL (2026-11-17): `CORE.RPL.EXEC` (incl. `CORE.RPL.EXEC.VERIFY`) is the product replay engine for every in-app mode, running against a project store. `TRUST.REPLAY.RUN` is the standalone verifier replayer that sees only a Result Package. They share no code; they share the outcome vocabulary and decision-invariance rule of `CORE.RPL.CLASS`, the REP-1 table of `UNC.RDL.REP` and the WASM artifact of `KRN.MAN.WASM`, all as `uses_artifacts`. A parity test on the replay conformance corpus (`TRUST.REPLAY.CORPUS`) is the agreement check.
- Dependencies narrowed (2026-11-17, D-034): kernel manifest, call encoding and WASM artifact (`KRN.MAN.*`), REP-1 (`UNC.RDL.REP`) and outcome definitions (`CORE.RPL.CLASS`) are artifacts; `fid-vreplay` links no KRN, UNC or CORE crate, matching the `TRUST.VERIFY.REPRO` allowlist.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| TRUST.REPLAY.SELECT | Replayable-set selection and NOT_REPLAYABLE reasons | 005 | none |
| TRUST.REPLAY.SANDBOX | WASM kernel sandbox with digest check and limits | 007 009 | none |
| TRUST.REPLAY.COMPARE | Replay outcome classification and aggregation | 002 003 004 006 | none |
| TRUST.REPLAY.KART | Kernel artifact sourcing for verifier replay (decision D-020) | 001 | none |
| TRUST.REPLAY.RUN | Verifier replay command | 001 | none |
| TRUST.REPLAY.CORPUS | Replay conformance corpus and cross-platform gate | 008 | none |
