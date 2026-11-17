---
id: CORE.RPL.XVER
title: Cross-version diff
level: 3
parent: CORE.RPL
kind: feature
status: leaf
phase: P0
size: M
owners: []
claimed_by:
satisfies: [REQ-CORE.RPL-012]
covers: []
depends_on: [CORE.RPL.EXEC.VERIFY, CORE.RPL.EXEC.LOOP, CORE.RPL.CLASS]
uses_artifacts: [UNC.RDL.REP, KRN.MAN.WASM]
side_effect: R1
determinism: D0
cli: [fid journal replay-cross-version]
mcp: [journal_replay_cross_version]
journal: [journal.replay_cross_version@1]
updated: 2026-11-17
---

# CORE.RPL.XVER: Cross-version diff

## Purpose

Answers "what would change if we upgraded the kernel?". XVER replays the same archived inputs under two kernel versions and reports, per characteristic, the value under each kernel, the signed delta, and any decision or status change. It feeds release regression, re-attestation and the validity review of earlier results. It reuses the verifier worker of `CORE.RPL.EXEC.VERIFY` once per kernel, so it adds pairing and reporting, not a second sandbox.

## Scope

**In:** running one span under kernel A and kernel B (one verifier worker each), pairing results by stable characteristic ID, value delta in the characteristic's unit, decision and status change flags, per-characteristic outcome from `CORE.RPL.CLASS`, summary counts, `cross-version-report.schema.json` v1 and the command `journal.replay_cross_version@1` (R1).
**Out:** obtaining old kernel artifacts (D-020; local sources in `CORE.RPL.EXEC.VERIFY`, downloads in `PLAT`/`ECO.REL`); release regression gates that consume the report (`ECO.CI`); re-attestation workflow (`TRUST`).

## Requirements

- **REQ-CORE.RPL.XVER-001** When `journal.replay_cross_version@1` runs, the engine shall execute the same span with the same recorded inputs once under kernel A and once under kernel B, each in its own verifier worker.
- **REQ-CORE.RPL.XVER-002** The report shall list, for each characteristic present in either run, its stable ID, the value under each kernel with unit, the signed delta B − A in the characteristic's unit, and the decision and status under each kernel.
- **REQ-CORE.RPL.XVER-003** The report shall flag every characteristic whose decision or status differs between the kernels and classify it `DIVERGENT` regardless of the size of the delta.
- **REQ-CORE.RPL.XVER-004** The report shall classify every other characteristic with `CORE.RPL.CLASS` using different-kernel context, and shall name the REP table version used.
- **REQ-CORE.RPL.XVER-005** If either kernel cannot be supplied by the `KernelSource`, then the command shall report `NOT_REPLAYABLE` naming the missing kernel digest and shall run neither side.
- **REQ-CORE.RPL.XVER-006** A characteristic present under only one kernel shall be listed with the missing side marked `absent` and classified `DIVERGENT`.
- **REQ-CORE.RPL.XVER-007** The signed delta shall be computed from the exact difference of the two f64 values rounded once to f64, and the report shall also hold both values' IEEE 754 bits in hexadecimal.
- **REQ-CORE.RPL.XVER-008** The command shall be class R1, write nothing to the project and append nothing to the Record Journal.

## Acceptance criteria

- **AC-1** Given fixture project `fixtures/core/rpl/xver/session_20` (20 characteristics) and two fixture kernels `k_a` and `k_b` where `k_b` changes one fit's last bits, when `fid journal replay-cross-version --branch main --kernel-a <k_a> --kernel-b <k_b> --json` runs, then the report validates against the schema, lists 20 characteristics, 19 `IDENTICAL` and 1 `EQUIVALENT` with the expected delta bits.
- **AC-2** Given a fixture kernel `k_c` that moves one value across its tolerance limit, then that characteristic is flagged with a decision change and classified `DIVERGENT`.
- **AC-3** (error path) Given `--kernel-b` naming a digest not in any local source, then the result is `NOT_REPLAYABLE` naming that digest and no worker is started.
- **AC-4** Given a kernel under which one characteristic is not produced, then it appears once with the missing side `absent` and outcome `DIVERGENT`.
- **AC-5** Given AC-1, when project file hashes and the Record Journal head are compared before and after, then they are unchanged.

## Design

- **Registry command(s):** `journal.replay_cross_version@1`: R1, projection `exposed`, D0 orchestration, `fs_effect: none`, `network: none`, `web: unavailable` (note "needs project storage and kernel workers"); params `{ branch: string (x-ref branch), from_seq?: integer, to_seq?: integer, kernel_a?: string (kernel manifest digest; default recorded), kernel_b: string }`; result `CrossVersionReport`. Runs as a cancellable job.
- **Data structures:** `CrossVersionReport { schema: "cross-version-report@1", span, kernels: [{ role, digest, origin }], rep_table, rows: [{ characteristic, unit, a: Side, b: Side, delta, delta_hex, decision_changed, status_changed, outcome, annotations }], summary: { identical, equivalent, divergent, not_replayable } }`; `Side { value, value_hex, decision, status } | absent`.
- **Algorithm:** check both kernels with `KernelSource` → run worker A then worker B over the span (sequentially, bounded memory) → collect characteristic results keyed by stable ID in ordered maps → pair → exact difference by TwoSum then one rounding → classify (decision change first) → sort rows by characteristic ID.
- **Errors:** `branch_not_found`, `span_invalid`, `cancelled`; missing kernel is an outcome, not an error.
- **Determinism:** D0 for the report given the two kernels' outputs.
- **Performance budget:** pairing and report for 100,000 characteristics in < 1 s, excluding worker runtime [unverified until measured].
- **Crate:** `fid-replay` (module `cross_version`).

## Test fixtures

`fixtures/core/rpl/xver/` synthetic project plus fixture kernels built from the reference kernel with injected last-bit changes (built in CI, never shipped); stand-in REP table (synthetic). CC-BY-4.0.

## Build prompt

> In crate `fid-replay`, add module `cross_version` and register `journal.replay_cross_version@1` (R1, exposed; params branch, optional from_seq/to_seq, optional kernel_a defaulting to the recorded kernel, required kernel_b; cancellable job). Check both kernels with `verify::KernelSource` (missing: outcome `NOT_REPLAYABLE` naming the digest, start no worker); run the span once per kernel in a `verify` worker; pair characteristic results by stable ID; compute the signed delta B − A as the exact difference rounded once (TwoSum, no FMA) plus both values' hex bits; flag decision and status changes as `DIVERGENT`; classify the rest with `classify` in different-kernel context and record the REP table version; one-sided rows are `absent` and `DIVERGENT`. Emit `CrossVersionReport` validating against `schemas/cross-version-report.schema.json` v1, rows sorted by characteristic ID. Write nothing to the project. Tests AC-1..AC-5. Constraints: CONSTITUTION §4 (any decision change is DIVERGENT; KNP-1 no FMA), C7 (no downloads), §8.

## Open questions

- **Resolved (parent's open question on old kernels offline):** D-020 option C applies; XVER uses the local `KernelSource` of `CORE.RPL.EXEC.VERIFY` and never downloads.
- `ECO.CI` release regression and `TRUST` re-attestation should list `CORE.RPL.XVER` in `uses_artifacts` for `cross-version-report.schema.json`.

## Children

_Leaf: none._
