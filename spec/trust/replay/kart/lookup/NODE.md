---
id: TRUST.REPLAY.KART.LOOKUP
title: Offline kernel artifact lookup chain
level: 4
parent: TRUST.REPLAY.KART
kind: task
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-TRUST.REPLAY.KART-001, REQ-TRUST.REPLAY.KART-003]
covers: []
depends_on: [TRUST.PKG.READER]
uses_artifacts: [KRN.MAN.WASM, KRN.MAN.MANI.SCHEMA, KRN.MAN.REPRO]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# TRUST.REPLAY.KART.LOOKUP: Offline kernel artifact lookup chain

## Purpose

The verifier's `ArtifactSource`: given the kernel manifest from a package, it finds the WASM reference artifact whose SHA-256 the manifest lists for target `wasm32-wasip1`, looking first in the package, then in the set bundled with the verifier, then in the local kernel cache, all without network access (D-020 option C). It returns the bytes and their origin, or `KERNEL_UNAVAILABLE` with the kernel version and how to obtain the artifact.

## Scope

**In:**
- `ArtifactSource` implementation and the fixed lookup order
- In-package member `kernel/<sha256>.wasm`, bundled directory, local cache directory
- Origin reporting and the unavailable hint
- Cache directory location and the `--kernel-cache` override

**Out:**
- Filling the cache from the network (`TRUST.REPLAY.KART.FETCH`)
- Executing the artifact and its final hash check (`TRUST.REPLAY.SANDBOX`)
- Producing artifacts and the rebuild recipe (`KRN.MAN.WASM`, `KRN.MAN.REPRO`)

## Requirements

- **REQ-TRUST.REPLAY.KART.LOOKUP-001** Given a kernel manifest, the source shall look for the `wasm32-wasip1` artifact digest it lists in this order: a package member `kernel/<sha256>.wasm` listed in the package manifest, the verifier's bundled directory, then the local cache directory, and shall return the first candidate whose SHA-256 equals the digest with origin `package`, `bundled` or `cache`.
- **REQ-TRUST.REPLAY.KART.LOOKUP-002** If a candidate's SHA-256 differs from the listed digest, then the source shall skip it, record a `candidate_mismatch` note with its origin, and continue with the next source.
- **REQ-TRUST.REPLAY.KART.LOOKUP-003** If no source holds a matching artifact, or the manifest lists no `wasm32-wasip1` artifact, then the source shall return `KERNEL_UNAVAILABLE` naming the kernel semver and artifact digest, with a hint naming `fid verify fetch-kernel` and the reproducible-rebuild recipe.
- **REQ-TRUST.REPLAY.KART.LOOKUP-004** The source shall open no network connection and write nothing outside the cache directory; it shall only read.
- **REQ-TRUST.REPLAY.KART.LOOKUP-005** The local cache directory shall default to the OS per-user cache location under `fid/kernels/` and shall be overridable per run with `--kernel-cache <dir>`; cache files shall be named `<sha256>.wasm`.

## Acceptance criteria

- **AC-1** Given golden package `fixtures/pkg/golden/points-with-kernel.fidpkg` (contains `kernel/<sha>.wasm`), when `get` runs with an empty cache, then origin is `package`.
- **AC-2** Given `fixtures/pkg/golden/points.fidpkg` (no kernel member) and the bundled set containing the digest, then origin is `bundled`; with only the cache holding it, origin is `cache`.
- **AC-3** Given a cache file with the right name but one flipped byte and no other source, then the result is `KERNEL_UNAVAILABLE` with a `candidate_mismatch` note for `cache`.
- **AC-4** (error path) Given a manifest for kernel `9.9.9` held nowhere, then `KERNEL_UNAVAILABLE` names `9.9.9`, the digest and both hints; and under a socket-denying sandbox AC-1..AC-4 pass unchanged.
- **AC-H1** Hypothesis: the offline lookup chain returns a digest-matching artifact for 100 % of kernel versions referenced by the frozen replay conformance corpus `fixtures/replay/corpus/<kernel-version>/` (TRUST.REPLAY.CORPUS) for the first released kernel version that are in the bundled set or carried in-package, with 0 network connections, measured by running `get` for each referenced digest under a socket-denying sandbox; falsified if any such version is `KERNEL_UNAVAILABLE` or any connection is attempted.
- **AC-H2** Hypothesis: the lookup chain accepts a mismatched artifact at rate 0 over a fault-injection set of at least 1,000 [unverified] corrupted candidates (flipped bytes, truncation, wrong-name files) spread across package, bundled and cache sources, measured by a seeded injection run counting accepted candidates whose SHA-256 differs from the manifest; falsified if any is accepted.

## Design

- **Registry command(s):** none; used by `verify.replay@1` (`TRUST.REPLAY.RUN`) and `TRUST.REPLAY.SELECT`.
- **Data structures:** `Origin = Package | Bundled | Cache`; `Unavailable { semver, digest, notes: Vec<Note>, hints }`; `ArtifactSource::get(&KernelManifest) -> Result<(Bytes, Origin), Unavailable>`.
- **Algorithm:** pick the `wasm32-wasip1` entry of the manifest's artifact list; probe sources in order; stream-hash each candidate; first match wins.
- **Errors:** `KERNEL_UNAVAILABLE` (as a NOT_REPLAYABLE reason).
- **Determinism:** D0 (same files give same answer).
- **Performance budget:** hashing a 20 MB artifact < 100 ms [unverified until measured].
- **Crate:** `fid-vreplay`, module `kart`.

## Test fixtures

Golden packages from `TRUST.PKG.FIXTURE` plus a variant with an embedded kernel member, generated by the fixture generator; a tiny stub WASM module stands in for the kernel where only hashing is tested.

## Build prompt

> In crate `fid-vreplay`, add module `kart` implementing `ArtifactSource::get(&KernelManifest) -> Result<(Bytes, Origin), Unavailable>`. Find the manifest's `wasm32-wasip1` artifact digest (parse the kernel manifest by its published schema; do not link `fid-kci`). Probe in order: (1) package member `kernel/<sha256>.wasm` via `fid-pkg::reader` (must be listed in the package manifest), (2) the bundled directory shipped next to `fid-verify` (`kernels/<sha256>.wasm`), (3) the local cache (`<os-cache>/fid/kernels/<sha256>.wasm`, override `--kernel-cache`). Stream-hash each candidate; skip mismatches with a `candidate_mismatch` note; return the first match with its origin. If none, return `KERNEL_UNAVAILABLE` naming semver and digest, with hints to run `fid verify fetch-kernel <digest>` or rebuild per the published reproducible-build recipe. No network, no writes. Tests: AC-1..AC-4. Constraints: D-020 option C, CONSTITUTION §8, REQ-TRUST.VERIFY.CLI-001 (no network-capable dependency).

## Open questions

- The bundled set holds the current major's artifacts only (D-020); the release process (ECO.REL) must place them in `kernels/`.

## Children

_Leaf: none._
