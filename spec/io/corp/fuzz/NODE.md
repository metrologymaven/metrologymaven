---
id: IO.CORP.FUZZ
title: Fuzz targets for every reader and CI fuzz budgets
level: 3
parent: IO.CORP
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
built_in: [fid-io-corp]
built_pr: 471
satisfies: [REQ-IO.CORP-004]
covers: []
depends_on: [IO.CORP.MANIF, IO.REG.DETECT, IO.REG.LIMITS]
side_effect: R1
determinism: N
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# IO.CORP.FUZZ: Fuzz targets and CI fuzz budgets

## Purpose

Ensures every registered first-party reader has a coverage-guided fuzz target seeded from its corpus, and runs them in CI with fixed budgets, failing on any panic, hang or limit breach. A reader without a target fails CI.

## Scope

**In:**
- A generic fuzz harness that drives any registered reader through the `IO.REG` harness with small limits
- Per-reader targets generated from the registry (one target per reader ID)
- Seed corpora from the manifests (accepted and hostile entries)
- CI jobs: per-merge budget and weekly budget, crash minimisation and artefact upload
- A coverage check: every reader ID in the registry has a target

**Out:**
- Hostile corpus generation (`IO.CORP.HOSTILE`), fidelity scoring (`IO.CORP.SCORE`)

## Requirements

- **REQ-IO.CORP.FUZZ-001** Every registered first-party reader shall have a fuzz target, and CI shall fail with `fuzz_target_missing` naming any reader without one.
- **REQ-IO.CORP.FUZZ-002** CI shall run each target for at least 10 CPU-minutes per merge to main and 8 CPU-hours per week [unverified: budgets to tune].
- **REQ-IO.CORP.FUZZ-003** If a target panics, aborts, exceeds 10 s on one input or exceeds the harness memory cap, then the CI job shall fail and upload the minimised input.
- **REQ-IO.CORP.FUZZ-004** Each target shall run the reader under `IO.REG.LIMITS` with limits small enough (e.g. 16 MiB bytes, depth 64) that a bound violation is a finding rather than a slow run.

## Acceptance criteria

- **AC-1** Given a test registry with readers `mesh.stl` and `test.panic_on_0xff` (a planted reader that panics on byte 0xFF), when the fuzz job runs 60 s per target, then it fails, names `test.panic_on_0xff` and uploads an input containing 0xFF.
- **AC-2** (error path) Given a registry with a reader that has no target, then the coverage check fails `fuzz_target_missing`.
- **AC-3** Given the STL reader and its manifest seeds, when fuzzed for 10 CPU-minutes, then no failure is reported (regression baseline).
- **AC-4** Given a planted reader that loops forever on an empty input, then the job fails with a timeout finding within 10 s per input.

## Design

- **Registry commands:** none (CI tooling).
- **Data structures:** `FuzzTarget { reader_id, seeds_dir }`, generated `fuzz/fuzz_targets/<reader_id>.rs` from a template.
- **Algorithm:** libFuzzer-style coverage-guided fuzzing (cargo-fuzz) [unverified: tool choice against allowlist]; per-target memory cap 2 GiB; timeout 10 s; minimise crashes before upload.
- **Errors:** `fuzz_target_missing`; findings reported as CI failures.
- **Determinism:** N (fuzzing is observational); findings are replayed deterministically from the saved input.
- **Performance budget:** REQ-002.
- **Crate:** `fid-io-corp` (fuzz workspace member `fid-io-fuzz`).

## Test fixtures

Planted test readers registered only in the test build; STL manifest seeds from `IO.CORP.MANIF`.

## Build prompt

> In crate `fid-io-corp`, build the fuzz layer: generate one cargo-fuzz target per registered first-party reader ID (from `fid import list-formats --json`) driving the reader through the `fid-io-reg` harness with small limits (16 MiB bytes, depth 64); seed from the format's manifest entries; add a coverage check failing `fuzz_target_missing`; add CI jobs with 10 CPU-minutes per target per merge and 8 CPU-hours per target weekly, timeout 10 s per input, 2 GiB memory cap, failing on panic/abort/timeout/OOM and uploading minimised inputs. Tests AC-1..AC-4 using planted test-only readers. CONSTITUTION §8 applies to non-test code.

## Open questions

- CI cost of weekly budgets across ~20 readers needs ECO.CI sizing.

## Children

_Leaf: none._
