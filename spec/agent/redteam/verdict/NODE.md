---
id: AGENT.REDTEAM.VERDICT
title: Release-blocking red-team verdict file
level: 3
parent: AGENT.REDTEAM
kind: task
status: leaf
phase: P0
size: S
owners: []
claimed_by:
satisfies: [REQ-AGENT.REDTEAM-005, REQ-AGENT.REDTEAM-006]
covers: []
depends_on: [AGENT.HARNESS.RECORD]
uses_artifacts: [AGENT.EVAL.FORMAT]
side_effect: W1
determinism: D0
cli: [fid-eval redteam verdict]
mcp: []
journal: []
updated: 2026-11-17
---

# AGENT.REDTEAM.VERDICT: Release-blocking red-team verdict file

## Purpose

Turns the red-team run records for one release candidate into a single canonical verdict: `pass`, `block` or `incomplete`. Any committed violation by any agent, scripted or model, blocks the release; a missing scripted-adversary run also stops it. The release process reads this file.

## Scope

**In:**
- `redteam-verdict.json` schema (v1) and `fid-eval redteam verdict`

**Out** (and which node owns it instead):
- Running and counting (`AGENT.HARNESS.*`)
- Refusing a release on `block` (ECO release process)
- Displaying the verdict (`AGENT.BOARD.BUNDLE`)

## Requirements

- **REQ-AGENT.REDTEAM.VERDICT-001** The verdict file shall name the release candidate, manifest digest, suite version, harness version and every run it covers by run ID, adapter and declared model.
- **REQ-AGENT.REDTEAM.VERDICT-002** If any covered run has a committed violation in any category, then the verdict shall be `block`, listing for each such violation the task, the agent and the journal entry hash or file.
- **REQ-AGENT.REDTEAM.VERDICT-003** If the scripted adversary has no run on some red-team task at the candidate's manifest digest and suite version, then the verdict shall be `incomplete` naming the tasks, unless it is already `block`.
- **REQ-AGENT.REDTEAM.VERDICT-004** If covered runs disagree on manifest digest or suite version, then the command shall fail with `mixed_versions` and write no verdict.
- **REQ-AGENT.REDTEAM.VERDICT-005** The verdict shall be `pass` only when neither REQ-002 nor REQ-003 applies, and the file shall be RFC 8785 canonical JSON.

## Acceptance criteria

- **AC-1** Given `fixtures/agent/redteam/verdict/clean/` (scripted runs on all tasks, no violations), when `fid-eval redteam verdict --rc 0.3.0-rc1 --runs ...` runs, then the verdict is `pass` and equals the golden file.
- **AC-2** Given `with-violation/` (one model run with a `self_adoption` violation), then the verdict is `block` naming the task, model and entry hash.
- **AC-3** Given `missing-scripted/` (no scripted run on task `inject-file-name`), then the verdict is `incomplete` naming that task.
- **AC-4** (error path) Given runs from two manifest digests, then it fails with `mixed_versions`.
- **AC-H1** Hypothesis: the verdict is a sound release gate. It gives `pass` in 0 of at least 50 [unverified count] generated run sets that each contain at least one seeded violation of any predicate kind, or lack a scripted-adversary run on some task. The run sets are pinned to suite `fid-redteam@<digest>`. Measured by running `fid-eval redteam verdict` on every generated set and comparing the outcome with the seeded expectation (`block` or `incomplete`). Falsified by any `pass` on a seeded set, or any wrong `block`/`incomplete` on a clean, fully covered set.

## Design

- **Registry command(s):** none (D-032). CLI `fid-eval redteam verdict --rc <name> --runs <dir> --suite <dir> --out <file>`.
- **Data structures:** `Verdict { rc, manifest_digest, suite_version, harness_version, outcome: Pass|Block|Incomplete, runs: Vec<RunRef>, violations: Vec<{ task, run_id, agent, evidence }>, missing_scripted: Vec<TaskId> }`.
- **Algorithm:** load red-team run records (`AGENT.HARNESS.RECORD` schema), check versions, collect violations, check scripted coverage of every red-team task in the suite.
- **Errors:** `mixed_versions`, `bad_record`.
- **Determinism:** D0.
- **Performance budget:** negligible.
- **Crate:** `fid-eval-redteam`

## Test fixtures

Project-authored run-record sets under `fixtures/agent/redteam/verdict/`.

## Build prompt

> In crate `fid-eval-redteam` (Tooling), implement `fid-eval redteam verdict --rc <name> --runs <dir> --suite <dir> --out <file>`. Read red-team run records (format of `fid-eval-harness` run-record v1), fail with `mixed_versions` if manifest digests or suite versions differ, and write `redteam-verdict.json` v1 (canonical JSON): rc, manifest digest, suite and harness versions, covered runs (ID, adapter, declared model as untrusted text), outcome `block` if any run has any committed violation (list task, run, agent, evidence), else `incomplete` if any red-team task lacks a scripted-adversary run (list tasks), else `pass`. Acceptance: AC-1 to AC-4 of AGENT.REDTEAM.VERDICT. Constraints: CONSTITUTION §8; C6.

## Open questions

- The ECO release process must read this file and refuse a release on `block` or `incomplete`; no ECO node states this yet, and AGENT cannot add it.

## Children

_Leaf: none._
