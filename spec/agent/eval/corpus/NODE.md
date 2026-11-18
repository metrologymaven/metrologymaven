---
id: AGENT.EVAL.CORPUS
title: First evaluation task corpus
level: 3
parent: AGENT.EVAL
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
built_in: [suite/tasks, fixtures/agent/eval/corpus]
built_pr: 120
satisfies: [REQ-AGENT.EVAL-002, REQ-AGENT.EVAL-006, REQ-AGENT.EVAL-008]
covers: []
depends_on: []
uses_artifacts: [AGENT.EVAL.FORMAT, AGENT.SKILLS.CATALOG, FLOW.RUNNER]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# AGENT.EVAL.CORPUS: First evaluation task corpus

## Purpose

Authors the first real task set: at least one task per curated workflow, cut from FLOW acceptance jobs, with start states, prompts and oracles, all on public or synthetic data. This is data, not code; its quality bar is that `fid-eval suite check` passes and every oracle traces to a golden-journal ResultRecord.

## Scope

**In:**
- Tasks under `suite/tasks/`, oracles under `suite/oracles/`, start-state archives
- A tracing table from each oracle item to the golden-journal entry that produced its ResultRecord
- Formula-derived characteristics (D-027) only where the golden journal computes them with `expr.evaluate_formula@1`

**Out** (and which node owns it instead):
- The format (`AGENT.EVAL.FORMAT`) and the check (`AGENT.EVAL.CHECK`)
- Red-team tasks (`AGENT.REDTEAM.*`)
- Golden journals and datasets (FLOW app nodes, `ECO.DATA`)

## Requirements

- **REQ-AGENT.EVAL.CORPUS-001** The corpus shall contain at least one task for each curated workflow of every app profile whose FLOW golden journals exist, and at least one task in each tier for each such app.
- **REQ-AGENT.EVAL.CORPUS-002** Each oracle item shall trace to the golden-journal entry hash whose ResultRecord it copies, and its outcome and value shall equal that record's.
- **REQ-AGENT.EVAL.CORPUS-003** Every input file shall be public under a licence on the project's allowlist or project-authored synthetic data, and its task file shall name that licence.
- **REQ-AGENT.EVAL.CORPUS-004** An oracle item for a formula-derived characteristic shall be admitted only if the golden journal produced it with `expr.evaluate_formula@1` (D-027), and the item shall require the same evaluation basis.
- **REQ-AGENT.EVAL.CORPUS-005** When the corpus changes, `fid-eval suite check` shall pass on it before the change is merged.

## Acceptance criteria

- **AC-1** Given the corpus and the catalog index at its release, when `fid-eval suite check` runs, then it exits 0.
- **AC-2** Given the tracing table `suite/TRACE.json`, when a CI script compares each oracle item with the cited golden-journal ResultRecord, then every outcome and value matches exactly.
- **AC-3** (error path) Given a task whose input lists licence `proprietary`, then the corpus licence check (part of AC-1 via the licence allowlist) fails naming the file.
- **AC-4** Given the corpus, then each app with golden journals has tasks in all three tiers.

## Design

- **Registry command(s):** none.
- **Data structures:** files in the AGENT.EVAL.FORMAT layout plus `suite/TRACE.json` mapping (task, oracle item) to golden-journal entry hash.
- **Algorithm:** for each catalog workflow, pick the FLOW job step range that performs it; save the project state at the range start as the start state; write a prompt in plain words naming the goal, not the commands; copy the ResultRecords at the range end into the oracle.
- **Errors:** none at run time; CI check findings.
- **Determinism:** D0 (static data).
- **Performance budget:** none.
- **Crate:** none (data in the `fid-eval` suite repository folder)

## Test fixtures

The corpus is itself test data. Sources: FLOW golden journals and `ECO.DATA` datasets (public or synthetic, licences recorded per file).

## Build prompt

> Author the first agent evaluation corpus in `suite/` using the task and oracle format of `fid-eval-suite`. For every curated workflow in `catalog-index.json` whose app has FLOW golden journals, create at least one task (and cover tiers `single_command`, `multi_step`, `full_job` for each app): start-state archive taken at the start of the job step range, a plain-language prompt that states the goal without naming commands or answers, the server profile, budgets, granted roots containing only task inputs, licences per input file (allowlisted SPDX or `project-authored`), and an oracle copied from the golden-journal ResultRecords at the range end. Write `suite/TRACE.json` linking each oracle item to its golden-journal entry hash and add a CI script that checks the copies match. Formula-derived characteristics are allowed only where the golden journal used `expr.evaluate_formula@1`. Acceptance: AC-1 to AC-4 of AGENT.EVAL.CORPUS; `fid-eval suite check` passes. Constraints: no standards text in prompts (CONSTITUTION §5); public or synthetic data only.

## Open questions

- REQ-AGENT.EVAL-008 was written while D-027 was open. D-027 is now accepted, so formula-derived values of record exist through `expr.evaluate_formula@1`; REQ-AGENT.EVAL.CORPUS-004 admits them only on that path. The parent requirement can be retired at its next revision.

## Children

_Leaf: none._
