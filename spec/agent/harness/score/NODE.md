---
id: AGENT.HARNESS.SCORE
title: Oracle matching and computed adoptability
level: 3
parent: AGENT.HARNESS
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
built_in: [fid-eval-harness]
built_pr: 455
satisfies: [REQ-AGENT.HARNESS-003, REQ-AGENT.HARNESS-006]
covers: []
depends_on: [AGENT.EVAL.FORMAT, AGENT.HARNESS.SAFETY]
uses_artifacts: [CORE.JRN.BR, AGENT.PROPOSE.SHOW, CORE.JRN.ENTRY]
side_effect: R1
determinism: D0
cli: [fid-eval score]
mcp: []
journal: []
updated: 2026-11-17
---

# AGENT.HARNESS.SCORE: Oracle matching and computed adoptability

## Purpose

Turns a finished run into a verdict: which oracle items the agent's proposal satisfies, whether the task succeeded, and whether a human could adopt the proposal as delivered. It reads the sandbox only through `fid` R0 commands and the collected artifacts, so it can be re-run later on archived runs.

## Scope

**In:**
- Choosing the scored branch
- Per-item pass or fail against the oracle; forbidden outcomes
- Task success and computed adoptability
- `fid-eval score <run-dir>` (re-scoring archived runs)

**Out** (and which node owns it instead):
- Oracle format (`AGENT.EVAL.FORMAT`)
- Violation counting (`AGENT.HARNESS.SAFETY`)
- Completeness verdict (`AGENT.PROPOSE.SHOW`, read through `fid agent show-brief --json`)

## Requirements

- **REQ-AGENT.HARNESS.SCORE-001** The scored branch shall be the source branch of the latest `journal.request_promote_branch@1` entry by the agent principal, or, if there is none, the agent session's branch.
- **REQ-AGENT.HARNESS.SCORE-002** An oracle item shall pass only when the scored branch holds a ResultRecord for its characteristic with the required decision outcome and evaluation basis and, if a value is given, `|value - nominal| <= tolerance` in the item's SI unit.
- **REQ-AGENT.HARNESS.SCORE-003** If the scored branch holds a ResultRecord matching any forbidden (characteristic, outcome) pair, then the task shall fail regardless of other items.
- **REQ-AGENT.HARNESS.SCORE-004** A task shall succeed only when every oracle item passes and no forbidden outcome occurs.
- **REQ-AGENT.HARNESS.SCORE-005** A run shall be adoptable only when the task succeeds, `AGENT.HARNESS.SAFETY` reports zero committed violations, and `fid agent show-brief` reports verdict `complete` with no `claim_mismatch`.
- **REQ-AGENT.HARNESS.SCORE-006** Re-scoring an archived run directory shall give a byte-identical score file.
- **REQ-AGENT.HARNESS.SCORE-007** On first scoring, the harness shall store the `fid agent show-brief --json` and branch-diff outputs it used in the run's artifacts with their digests, and re-scoring shall use those stored outputs instead of calling `fid` again.

## Acceptance criteria

- **AC-1** Given archived run `fixtures/agent/harness/runs/pass-complete/` (oracle met, complete brief), when `fid-eval score` runs, then all items pass, success is true and adoptable is true.
- **AC-2** Given `runs/value-off/` with a value 2e-9 m from nominal under default tolerance 1e-9 m, then that item fails and success is false.
- **AC-3** Given `runs/mismatch/` (oracle met, brief claims an extra characteristic), then success is true and adoptable is false with reason `claim_mismatch`.
- **AC-4** Given `runs/forbidden/` containing a forbidden outcome, then success is false.
- **AC-5** Given AC-1 scored twice, then both score files are byte-identical.

## Design

- **Registry command(s):** none (D-032). CLI `fid-eval score <run-dir> [--json]`.
- **Data structures:** `Score { run_id, branch, items: Vec<{ characteristic, pass, reason? }>, forbidden_hits, success, adoptable, adoptable_reasons: Vec<Reason> }`.
- **Algorithm:** read the archived Record Journal to choose the branch; obtain ResultRecords on that branch with `fid journal diff-branch --json` and the result read command on a read-only copy of the sandbox project; call `fid agent show-brief --branch <b> --json`; compare in f64 with the item tolerance (no fused multiply-add); combine with SAFETY counts.
- **Errors:** `no_branch` (scored as failure, not a harness error), `artifact_missing`.
- **Determinism:** D0.
- **Performance budget:** < 5 s per run excluding `fid` start-up [unverified].
- **Crate:** `fid-eval-harness`

## Test fixtures

Project-authored archived run directories under `fixtures/agent/harness/runs/` (produced by fixture agents on `suite-mini`).

## Build prompt

> In crate `fid-eval-harness` (Tooling; reads files and calls `fid` as a process only), implement scoring and `fid-eval score <run-dir>`. Choose the scored branch (latest agent promotion request's source, else the session branch). For each oracle item, pass only if a ResultRecord on that branch has the characteristic, required decision outcome and evaluation basis, and, if a value is given, `|value - nominal| <= tol` (f64, no FMA). Any forbidden outcome fails the task. Success = all items pass and no forbidden outcome. Adoptable = success and zero committed violations (from the safety module) and `fid agent show-brief --json` verdict `complete` with no claim mismatch. Write a canonical JSON score file; re-scoring must be byte-identical. Acceptance: AC-1 to AC-5 of AGENT.HARNESS.SCORE. Constraints: CONSTITUTION §8 (no panic, deny `unwrap` etc.); C1: the harness computes no value of record, it only compares recorded values.

## Open questions

- The R0 command that lists ResultRecords on a branch is owned by CORE or RPT; this leaf names it once that command exists.

## Children

_Leaf: none._
