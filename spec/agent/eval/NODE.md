---
id: AGENT.EVAL
title: Evaluation task suite, oracles and fixtures
level: 2
parent: AGENT
kind: subsystem
status: open
phase: P0
size: L
owners: []
claimed_by:
satisfies: [REQ-AGENT-002]
covers: []
depends_on: [FLOW, AGENT.SKILLS, CORE.JRN.CANON]
cli: []
mcp: []
journal: []
updated: 2026-11-16
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

## Open questions

- D-032: whether the suite check is a `fid agent` command or part of a separate `fid-eval` binary.
- Oracle tolerance for values when an agent legitimately reaches the result by a different command path (for example another fit method): proposed per-characteristic tolerance in the task, defaulting to exact match of decision outcome and 1e-9 m on value [unverified].

## Children

_None yet: status `open`._
