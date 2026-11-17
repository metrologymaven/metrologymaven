---
id: AGENT.BOARD.RECOMP
title: Score recompute check and third-party submissions
level: 3
parent: AGENT.BOARD
kind: task
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-AGENT.BOARD-002, REQ-AGENT.BOARD-003]
covers: []
depends_on: [AGENT.HARNESS.RECORD, AGENT.HARNESS.SCORE, AGENT.HARNESS.SAFETY]
uses_artifacts: [AGENT.EVAL.FORMAT]
side_effect: R1
determinism: D0
cli: [fid-eval board recompute]
mcp: []
journal: []
updated: 2026-11-17
---

# AGENT.BOARD.RECOMP: Score recompute check and third-party submissions

## Purpose

Makes every published number reproducible: from the shipped run records and run artifacts, re-score each run and re-aggregate with the harness's own functions, and fail on any difference. The same check is the admission test for scores submitted by third parties, which are published only with their submitter's name.

## Scope

**In:**
- `fid-eval board recompute <bundle-input-dir>`
- Submission layout and labelling (`submission.json`: submitter, contact handle, date, statement of method)

**Out** (and which node owns it instead):
- Aggregation and scoring functions (`AGENT.HARNESS.RECORD`, `AGENT.HARNESS.SCORE`, `AGENT.HARNESS.SAFETY`; called, not reimplemented)
- Building the bundle (`AGENT.BOARD.BUNDLE`)

## Requirements

- **REQ-AGENT.BOARD.RECOMP-001** For every run record in the input, the check shall re-run scoring and safety counting on that run's archived artifacts after verifying their digests, and shall fail with `run_mismatch` if the recomputed success, adoptability or any count differs from the record.
- **REQ-AGENT.BOARD.RECOMP-002** The check shall rebuild every suite report from the run records and shall fail with `score_mismatch` if any published figure differs byte for byte in canonical JSON.
- **REQ-AGENT.BOARD.RECOMP-003** If a run's artifacts are missing or their digests do not match the record's manifest, then the check shall fail with `artifact_mismatch` naming the run.
- **REQ-AGENT.BOARD.RECOMP-004** A third-party submission shall carry `submission.json` naming the submitter, and the check shall fail with `unlabelled_submission` if any run in a submission folder lacks it.
- **REQ-AGENT.BOARD.RECOMP-005** If a submission's suite version is not a published suite version, then the check shall fail with `unknown_suite`.

## Acceptance criteria

- **AC-1** Given `fixtures/agent/board/input-ok/` (project runs and one labelled third-party submission), when `fid-eval board recompute` runs, then it passes.
- **AC-2** (error path) Given a run record whose success flag was flipped by hand, then it fails with `run_mismatch`.
- **AC-3** (error path) Given a suite report with one rate edited, then it fails with `score_mismatch`.
- **AC-4** (error path) Given a submission folder without `submission.json`, then it fails with `unlabelled_submission`.

## Design

- **Registry command(s):** none (D-032). CLI `fid-eval board recompute <dir> [--json]`.
- **Data structures:** `Submission { submitter, contact, date, method_statement: Untrusted }`.
- **Algorithm:** verify artifact digests; call `fid-eval-harness` scoring and safety on archived artifacts (re-scoring uses the archived `show-brief` and branch-diff outputs stored by `AGENT.HARNESS.SCORE`, so no `fid` process is needed); call the report builder; compare canonical bytes.
- **Errors:** `run_mismatch`, `score_mismatch`, `artifact_mismatch`, `unlabelled_submission`, `unknown_suite`.
- **Determinism:** D0.
- **Performance budget:** < 10 min for 5,000 runs [unverified].
- **Crate:** `fid-eval-board`

## Test fixtures

Project-authored bundle inputs under `fixtures/agent/board/`, built from fixture-agent runs on `suite-mini`.

## Build prompt

> Create crate `fid-eval-board` (Tooling; depends on `fid-eval-harness` and `fid-eval-suite`, never on product crates). Implement `fid-eval board recompute <dir>`: for every run record, verify artifact digests (`artifact_mismatch`), re-run scoring and safety counting on the archived artifacts using the harness's own functions and compare (`run_mismatch`); rebuild every suite report with the harness report builder and compare canonical bytes (`score_mismatch`). Third-party submission folders need `submission.json` (submitter, contact handle, date, untrusted method statement) or fail with `unlabelled_submission`; their suite version must be published (`unknown_suite`). Acceptance: AC-1 to AC-4 of AGENT.BOARD.RECOMP. Constraints: CONSTITUTION §8; no reimplemented scoring or aggregation.

## Open questions

- None.

## Children

_Leaf: none._
