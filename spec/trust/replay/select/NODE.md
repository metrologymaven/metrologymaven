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
depends_on: [TRUST.PKG.READER]
uses_artifacts: [KRN.MAN.MANI.SCHEMA, KRN.MAN.KCI]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-21
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
- **AC-H1** Hypothesis: the selector achieves 100 % agreement with the hand-labelled replayability and `NOT_REPLAYABLE` reason of every result in the frozen replay conformance corpus `fixtures/replay/corpus/<kernel-version>/` (TRUST.REPLAY.CORPUS) for the first released kernel version, with false-replayable rate 0, measured by comparing selector output to each package's `expect.json`; falsified if any result is misclassified or a result is marked replayable whose closure lacks an input.
- **AC-H2** Hypothesis: the selector marks at least 95 % [unverified] of points-tier results replayable on the frozen replay conformance corpus `fixtures/replay/corpus/<kernel-version>/` (TRUST.REPLAY.CORPUS) for the first released kernel version, excluding cases built to be `NOT_REPLAYABLE`, measured by counting replayable results over eligible results; falsified if the share is below the threshold.

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
- Build status (2026-11-21, module `fid-vreplay::select`): not marked `built_in`. Open: AC-1 and AC-2 name `fixtures/pkg/golden/points.zip` and `results.zip`, which `TRUST.PKG.FIXTURE` has not produced yet; the module passes both against stand-in packages `fixtures/replay/select/points.zip` and `results.zip` from the independent reference `fixtures/replay/select/make_cases.py`, and the tests must be re-pointed once the golden packages exist. AC-H1 and AC-H2 wait for `TRUST.REPLAY.CORPUS`. AC-3, AC-4 and AC-5 are covered.
- Package content model assumed by the build, since `TRUST.PKG.CONTENT.SELECT` and `.JSEG` are unbuilt: `results/records.json` is a JSON array of ResultRecords carrying `id`, `evaluation_basis`, `algorithm` (`<id>@<semver>`) and `journal_entry`; each line of `journal/segment.jsonl` is `{"header": <entry.schema.json header>, "payload": <EntryPayload JSON>}` with the payload optional; kernel-call parameters are the payload's `args`, checked against `args_commit`. If those leaves choose otherwise, this module follows them.
- Rules the build fixed where the Design is silent: a kernel call is a `command` entry naming exactly one algorithm; the producer of an input is the latest earlier `command` entry that outputs its digest; a kernel-produced input is always recomputed, a non-kernel producer's output is substituted when that digest is packaged under `points/`, and a missing one is `POINTS_MISSING` (no producer, or a producer recorded as class N) or `NON_KERNEL_STEP` (any other non-kernel producer); a record whose entry is absent, not a command, or names another algorithm is `ENTRY_MISSING`, a non-kernel root is `NON_KERNEL_STEP`; a kernel call recorded under another kernel manifest is `KERNEL_UNAVAILABLE`. When several reasons apply, the primary one is the first in the REQ-001 list (then `PLAN_TOO_LARGE`), with the smallest detail string. A record basis `RESULTS_ONLY`/`DIRECT_READING` comes first, then a `results` evidence tier (`BASIS_RESULTS_ONLY` for every result).
- Trust boundary (as for TRUST.REPLAY.RUN): replayability never rests on a determinism or side-effect class the package declares; the recorded class only picks between two `NOT_REPLAYABLE` labels. RUN must take determinism and quantity kind from the algorithm catalogue by the replayable result's `algorithm_id`, which SELECT takes from the journal entry and checks against the kernel manifest.

## Children

_Leaf: none._
