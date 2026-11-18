---
id: AGENT.EVAL.CHECK
title: Suite consistency, coverage and oracle isolation check
level: 3
parent: AGENT.EVAL
kind: task
status: leaf
phase: P0
size: S
owners: []
claimed_by:
built_in: [fid-eval-suite]
satisfies: [REQ-AGENT.EVAL-003, REQ-AGENT.EVAL-005]
covers: []
depends_on: [AGENT.EVAL.FORMAT]
uses_artifacts: [AGENT.SKILLS.CATALOG, FLOW.RUNNER]
side_effect: R0
determinism: D0
cli: [fid-eval suite check]
mcp: []
journal: []
updated: 2026-11-17
---

# AGENT.EVAL.CHECK: Suite consistency, coverage and oracle isolation check

## Purpose

One CI check that a suite is fit to run: every file parses and matches its digest, every curated workflow is exercised, and nothing an agent must not see (oracles, golden journals, other tasks) is reachable through the roots a task grants.

## Scope

**In:**
- `fid-eval suite check`: parse, digest, cross-reference, coverage and isolation rules

**Out** (and which node owns it instead):
- The format and parser (`AGENT.EVAL.FORMAT`)
- Runtime root enforcement (`AUTO.SURF.ROOTS`, `CORE.BUS.POL`); this check is static

## Requirements

- **REQ-AGENT.EVAL.CHECK-001** If any task's start-state archive does not match its recorded digest, or its source job or golden-journal digest does not match a FLOW job package, then the check shall fail naming the task.
- **REQ-AGENT.EVAL.CHECK-002** If a curated workflow in the catalog index is named by no task's `workflow` tag, then the check shall fail with `uncovered_workflow` naming it.
- **REQ-AGENT.EVAL.CHECK-003** If any granted root of a task, after unpacking its start state, contains an oracle file, a golden journal, another task's files or any file whose digest equals one of those, then the check shall fail with `oracle_leak` naming the task and file.
- **REQ-AGENT.EVAL.CHECK-004** If a task's prompt contains a characteristic value or decision outcome from its own oracle, then the check shall fail with `oracle_in_prompt`.
- **REQ-AGENT.EVAL.CHECK-005** The check shall exit 0 only when no rule fails, and shall report all failures, not just the first.

## Acceptance criteria

- **AC-1** Given `fixtures/agent/eval/suite-mini/` with `fixtures/agent/eval/catalog-index-mini.json`, when `fid-eval suite check --suite ... --catalog ... --jobs fixtures/agent/eval/jobs/` runs, then it exits 0.
- **AC-2** (error path) Given `suite-leak/` whose task grants a read root containing a copy of its oracle, then it fails with `oracle_leak`.
- **AC-3** (error path) Given a catalog index with a workflow no task names, then it fails with `uncovered_workflow`.
- **AC-4** (error path) Given a prompt containing "expected: CONFORMS" for its own characteristic, then it fails with `oracle_in_prompt`.
- **AC-5** Given a suite with three different faults, then all three are reported.

## Design

- **Registry command(s):** none (D-032). CLI `fid-eval suite check --suite <dir> --catalog <catalog-index.json> --jobs <dir> [--json]`.
- **Data structures:** `CheckFinding { rule, task?, file?, detail }`; the task `workflows` field of `AGENT.EVAL.FORMAT`.
- **Algorithm:** parse with `fid-eval-suite`; unpack each start state into a temp directory; hash every file under granted roots and compare with the set of oracle, golden-journal and other-task digests; scan prompts for oracle outcome words next to their characteristic IDs and for oracle values formatted in the task's units.
- **Errors:** rule codes above; `parse_error` passthrough.
- **Determinism:** D0.
- **Performance budget:** < 60 s for 1,000 tasks [unverified].
- **Crate:** `fid-eval-suite`

## Test fixtures

Project-authored suites `suite-mini`, `suite-leak` and fixture job packages under `fixtures/agent/eval/`.

## Build prompt

> In crate `fid-eval-suite` (Tooling; no product crate dependencies), add `fid-eval suite check --suite <dir> --catalog <catalog-index.json> --jobs <dir>`. Using the suite parser, fail when start-state digests or source-job/golden-journal digests do not match the FLOW job packages, when a catalog workflow is named by no task (`uncovered_workflow`), when any file reachable under a task's granted roots after unpacking its start state is (or has the digest of) an oracle, golden journal or another task's file (`oracle_leak`), or when a prompt contains its own oracle's outcome or value (`oracle_in_prompt`). Report every finding and exit non-zero on any. Acceptance: AC-1 to AC-5 of AGENT.EVAL.CHECK. Constraints: CONSTITUTION §8 (no panic, deny `unwrap` etc., bounded unpacking: reject archive entries with absolute paths or `..`).

## Open questions

- None.

## Children

_Leaf: none._
