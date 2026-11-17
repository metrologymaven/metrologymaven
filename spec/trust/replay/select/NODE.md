---
id: TRUST.REPLAY.SELECT
title: Replayable-set selection and NOT_REPLAYABLE reasons
level: 3
parent: TRUST.REPLAY
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-TRUST.REPLAY-005]
covers: []
depends_on: [TRUST.PKG.READER, KRN.MAN]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# TRUST.REPLAY.SELECT: Replayable-set selection and NOT_REPLAYABLE reasons

## Purpose

Decides, for each result in a package, whether the verifier can recompute it, and builds the replay plan for those it can: the ordered list of kernel calls (algorithm ID, parameters, seed, input digests) reconstructed from the packaged journal entries. Results that cannot be replayed get `NOT_REPLAYABLE` with a precise, enumerated reason, so the report never hides why a check was skipped.

## Scope

**In:**
- Mapping each ResultRecord to the journal entries that produced it (by entry hash and input/output digests)
- Dependency closure: earlier kernel calls whose outputs feed the result (e.g. a constructed feature)
- Availability checks: point sets present, algorithm ID known to the target kernel manifest, kernel artifact obtainable (asked of `TRUST.REPLAY.KART`)
- Reason codes for `NOT_REPLAYABLE`
- The ordered replay plan

**Out:**
- Obtaining the artifact (`TRUST.REPLAY.KART`)
- Executing calls (`TRUST.REPLAY.SANDBOX`)
- Classification (`TRUST.REPLAY.COMPARE`)

## Requirements

- **REQ-TRUST.REPLAY.SELECT-001** For each ResultRecord, the selector shall report `NOT_REPLAYABLE` with exactly one primary reason from: `BASIS_RESULTS_ONLY`, `BASIS_DIRECT_READING`, `POINTS_MISSING`, `ENTRY_MISSING`, `ALGORITHM_UNKNOWN`, `KERNEL_UNAVAILABLE`, `NON_KERNEL_STEP`.
- **REQ-TRUST.REPLAY.SELECT-002** The selector shall mark a result replayable only when every kernel call in its dependency closure has its inputs present in the package or produced by an earlier call in the plan.
- **REQ-TRUST.REPLAY.SELECT-003** When a closure contains a class-N step (instrument reading, human text, external file) whose recorded output is packaged, the selector shall substitute the recorded output and not mark the result `NOT_REPLAYABLE`.
- **REQ-TRUST.REPLAY.SELECT-004** The replay plan shall list kernel calls in journal order with algorithm ID, parameters, seed and input digests, and each call shall appear once even when several results depend on it.
- **REQ-TRUST.REPLAY.SELECT-005** If the closure of a result exceeds 100,000 kernel calls, then the selector shall mark it `NOT_REPLAYABLE` with reason `PLAN_TOO_LARGE` [unverified limit].

## Acceptance criteria

- **AC-1** Given `fixtures/pkg/golden/points.zip` (three hole results), when the selector runs, then all three are replayable and the plan contains the expected kernel calls listed in `fixtures/replay/select/points.plan.json`.
- **AC-2** Given `fixtures/pkg/golden/results.zip`, then every result is `NOT_REPLAYABLE` with `BASIS_RESULTS_ONLY`.
- **AC-3** (error path) Given `fixtures/replay/select/missing-points.zip` (one point set removed and manifest re-signed with test keys), then that result is `POINTS_MISSING` and the others are replayable.
- **AC-4** Given `fixtures/replay/select/constructed.zip` (a distance between two fitted circles), then the plan contains both fits before the distance, each once.
- **AC-5** Given `fixtures/replay/select/unknown-algo.zip`, then `ALGORITHM_UNKNOWN`.

## Design

- **Registry command(s):** none; used by `verify.replay@1`.
- **Data structures:** `ReplayPlan { calls: Vec<KernelCall>, results: Vec<(RecordId, Vec<CallIndex>)> }`; `KernelCall { entry_index, algorithm_id, params, seed: Option<u64>, inputs: Vec<Digest> }`; `NotReplayable { record_id, reason, detail }`.
- **Algorithm:** index entries by output digest (sorted vector); for each record walk backwards through input digests to producing entries; topological order by entry index; check availability.
- **Errors:** passthrough `pkg.*`.
- **Determinism:** D0.
- **Performance budget:** plan for 10,000 results in under 1 s [unverified until measured].
- **Crate:** `fid-vreplay` (module `select`).

## Test fixtures

- Golden packages from `TRUST.PKG.FIXTURE` and `fixtures/replay/select/*` generated from them with documented edits.

## Build prompt

> Create crate `fid-vreplay` with module `select`. From a package (read via `fid-pkg::reader`), map each ResultRecord to the journal entries that produced it via entry hash and input/output digests, compute the dependency closure of kernel calls, substitute packaged recorded outputs for class-N steps, and produce an ordered, deduplicated `ReplayPlan` (algorithm ID, params, seed, input digests). Unreplayable results get one primary reason: `BASIS_RESULTS_ONLY`, `BASIS_DIRECT_READING`, `POINTS_MISSING`, `ENTRY_MISSING`, `ALGORITHM_UNKNOWN`, `KERNEL_UNAVAILABLE`, `NON_KERNEL_STEP`, `PLAN_TOO_LARGE` (over 100,000 calls). Ask an injected `ArtifactSource` trait (implemented by the KART module) whether the kernel artifact is available. Tests: the five acceptance criteria. Constraints: CONSTITUTION §8, D0, no std HashMap.

## Open questions

- `KERNEL_UNAVAILABLE` semantics depend on D-020 (artifact sourcing); the trait boundary keeps this leaf buildable.

## Children

_Leaf: none._
