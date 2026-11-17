---
id: TRUST.REPLAY.CORPUS
title: Replay conformance corpus and cross-platform gate
level: 3
parent: TRUST.REPLAY
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-TRUST.REPLAY-008]
covers: []
depends_on: [TRUST.REPLAY.RUN]
uses_artifacts: [TRUST.PKG.FIXTURE, KRN.MAN.WASM]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# TRUST.REPLAY.CORPUS: Replay conformance corpus and cross-platform gate

## Purpose

A versioned set of Result Packages with points, produced by real kernel releases, together with expected replay outcomes; and a CI gate that replays every package on Windows, Linux and macOS on x86-64 and arm64. It proves that D0 algorithms replay `IDENTICAL` everywhere, and gives third-party verifiers a public test set.

## Scope

**In:**
- Corpus layout: packages plus `expect.json` (package and per-result outcomes)
- Coverage rule: at least one package per D0 algorithm in the kernel manifest, plus planned `EQUIVALENT`, `DIVERGENT` and `NOT_REPLAYABLE` cases
- Generator that builds packages from synthetic seeded inputs through the real kernel and exporter
- CI matrix job on six platform targets

**Out:**
- Format conformance packages (`TRUST.PKG.FIXTURE`)
- Kernel validation datasets (`UNC.RDL`)

## Requirements

- **REQ-TRUST.REPLAY.CORPUS-001** The corpus shall contain at least one points-tier package for every D0 algorithm in the current kernel manifest, and CI shall fail when an algorithm lacks one.
- **REQ-TRUST.REPLAY.CORPUS-002** The corpus shall include at least one deliberately `DIVERGENT` package (changed decision), one `EQUIVALENT` package (D3 algorithm within tolerance) and one package per `NOT_REPLAYABLE` reason.
- **REQ-TRUST.REPLAY.CORPUS-003** CI shall replay every corpus package on Windows, Linux and macOS for x86-64 and arm64 and fail if any outcome differs from `expect.json` or any D0 result is not `IDENTICAL`.
- **REQ-TRUST.REPLAY.CORPUS-004** The generator shall be deterministic for a seed, and CI shall fail if regenerated packages differ from committed digests for the same kernel version.

## Acceptance criteria

- **AC-1** Given the corpus for the current kernel, when `cargo xtask replay-corpus --check` runs on each of the six CI targets, then every package matches its `expect.json`.
- **AC-2** (error path) Given a kernel manifest with a new D0 algorithm and no corpus package, then the coverage check fails naming the algorithm.
- **AC-3** (error path) Given a corpus package whose expected outcome is edited to `IDENTICAL` for the planned `DIVERGENT` case, then the gate fails.
- **AC-4** Given the generator run twice with seed `0xC0FFEE`, then package digests are identical.

## Design

- **Registry command(s):** none.
- **Data structures:** `fixtures/replay/corpus/<kernel-version>/<case>/{package.fidpkg, expect.json}`.
- **Algorithm:** seeded synthetic geometry → kernel fit via the CLI in a scratch project → `sign.export_package@1` with points tier → test-key signature → expected outcome from a reference replay, reviewed by a maintainer on first commit.
- **Errors:** gate reports the case and platform.
- **Determinism:** D0 for D0 cases; D2 seeds for generation.
- **Performance budget:** whole corpus under 10 min per platform [unverified until measured].
- **Crate:** `fid-vreplay` (xtask `replay-corpus`); CI matrix config.

## Test fixtures

- Synthetic, seeded geometry only (CC-BY-4.0); no third-party data.

## Build prompt

> Add xtask `replay-corpus` to `fid-vreplay`. Generate, from fixed seeds, points-tier Result Packages covering every D0 algorithm in the current kernel manifest, plus planned `EQUIVALENT` (D3 within tolerance), `DIVERGENT` (changed decision) and one per `NOT_REPLAYABLE` reason; store under `fixtures/replay/corpus/<kernel-version>/<case>/` with `expect.json`. `--check` replays each with `verify.replay@1` and compares to `expect.json`, fails on any D0 result that is not `IDENTICAL`, fails when a D0 algorithm has no package, and fails when regenerated packages differ from committed digests. Add a CI matrix for Windows, Linux and macOS on x86-64 and arm64. Tests: the four acceptance criteria. Constraints: CONSTITUTION §4, §8; test keys only; no third-party data.

## Open questions

- Corpus size will grow with each kernel version; older versions' corpora may move to a separate repository (ECO) once D-020 settles artifact archiving.

## Children

_Leaf: none._
