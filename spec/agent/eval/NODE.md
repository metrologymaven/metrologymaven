---
id: AGENT.EVAL
title: Evaluation task suite, oracles and fixtures
level: 2
parent: AGENT
kind: subsystem
status: split
phase: P0
size: L
owners: []
claimed_by:
satisfies: [REQ-AGENT-002]
covers: []
depends_on: []
uses_artifacts: [FLOW.RUNNER, AGENT.SKILLS.CATALOG, CORE.JRN.CANON]
cli: [fid-eval suite *]
mcp: []
journal: []
updated: 2026-11-17
---

# AGENT.EVAL: Evaluation task suite, oracles and fixtures

## Purpose

Defines the metrology tasks agents are evaluated on and the machine-checkable oracle for each. Tasks are cut from the FLOW acceptance jobs, so an agent is scored on the same real work the product is accepted on. The suite is data (task files, start states, oracles) with a version digest; running it is `AGENT.HARNESS`'s job. Adversarial tasks use this format but are owned by `AGENT.REDTEAM`.

## Scope

**In:**
- Task file format: source job, golden journal digest, start state, prompt, server profile, budgets, oracle
- Oracles: required ResultRecords (characteristic, decision outcome, evaluation basis, value within a stated tolerance) and forbidden outcomes
- Difficulty tiers and app tags
- Suite version digest and the suite consistency check
- Coverage rule: every curated workflow has a task
- Oracle isolation: what an agent under test must never be able to read

**Out** (and which node owns it instead):
- Running agents and computing scores (`AGENT.HARNESS`)
- Red-team tasks and violation predicates (`AGENT.REDTEAM`)
- Acceptance jobs and golden journals (`FLOW`)
- The workflow catalog (`AGENT.SKILLS`)
- Canonical JSON and digests (`CORE.JRN.CANON`)

## Requirements

- **REQ-AGENT.EVAL-001** Each task shall name its source FLOW acceptance job and golden-journal digest, its start-state digest, its task prompt, the MCP server profile the agent gets, and its budgets of wall time and tool calls.
- **REQ-AGENT.EVAL-002** Each task shall have an oracle that lists the ResultRecords the agent branch must produce, each with characteristic, decision outcome, evaluation basis and a value tolerance in SI units, plus any forbidden outcomes.
- **REQ-AGENT.EVAL-003** If a curated workflow in `AGENT.SKILLS` is exercised by no task, then the suite consistency check shall fail naming the workflow.
- **REQ-AGENT.EVAL-004** The suite version shall be the SHA-256 digest of the canonical JSON of all task files, start states and oracles, and any change to any of them shall produce a new suite version.
- **REQ-AGENT.EVAL-005** No prompt-external task material (oracles, golden journals, other tasks) shall lie under the import or export roots a task grants the agent, and the suite consistency check shall fail if one does.
- **REQ-AGENT.EVAL-006** All task data shall be public or synthetic, and each task shall record the licence of every input file.
- **REQ-AGENT.EVAL-007** Each task shall carry one difficulty tier (`single_command`, `multi_step` or `full_job`) and one or more app tags (Inspect, Probe, Network, Report).
- **REQ-AGENT.EVAL-008** While D-027 is open, the suite shall contain no task whose oracle requires a derived value of record from user arithmetic.

## Interfaces

- **Offers:** the task and oracle file format; the versioned suite; the suite consistency check (surface set by D-032).
- **Consumes:** FLOW golden journals and their fixtures; `AGENT.SKILLS` catalog; `CORE.JRN.CANON` digests.

## Decomposition hints

FORMAT (task and oracle schema), ORACLE (tolerance and outcome matching rules), CHECK (consistency, coverage and isolation check), CORPUS (the first task set, one per curated workflow).

## Split rationale

D-032 (accepted) puts all evaluation tooling in the separate `fid-eval` binary, outside the registry and never on MCP, so the children have CLI entries `fid-eval suite *` and no MCP tools or journal events. The split separates the **format** (FORMAT, the shared parser and suite digest for every `fid-eval` crate), the **static checks** (CHECK) and the **data** (CORPUS, P1a, because FLOW golden journals start in P1a). Alternative considered: an ORACLE child for matching rules; rejected because matching is scoring and belongs to `AGENT.HARNESS.SCORE`, while the oracle schema belongs to FORMAT.

## Open questions

- Resolved by D-032 (accepted): the suite check is `fid-eval suite check`.
- D-027 is accepted, so REQ-AGENT.EVAL-008 no longer constrains the suite; `AGENT.EVAL.CORPUS` admits formula-derived values only via `expr.evaluate_formula@1`. Retire -008 at the next revision.
- Oracle tolerance for values when an agent legitimately reaches the result by a different command path (for example another fit method): proposed per-characteristic tolerance in the task, defaulting to exact match of decision outcome and 1e-9 m on value [unverified].

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| AGENT.EVAL.FORMAT | Task and oracle file format and suite version | 001 002 004 006 007 | none |
| AGENT.EVAL.CHECK | Suite consistency, coverage and oracle isolation check | 003 005 | none |
| AGENT.EVAL.CORPUS | First evaluation task corpus | 002 006 008 | none |
