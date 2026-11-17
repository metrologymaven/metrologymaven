---
id: AGENT
title: Agent experience: skills, recipes and evaluation
level: 1
parent: FID
kind: domain
status: split
phase: P0
size: XL
owners: []
claimed_by:
satisfies: [REQ-FID-030]
covers: []
depends_on: [AUTO, CORE, FLOW]
cli: [fid agent *]
mcp: [agent_*]
journal: [agent.*]
updated: 2026-11-16
---

# AGENT: Agent experience and evaluation

## Purpose

Turns "MCP-first" from plumbing into a proven capability. AUTO exposes the commands. AGENT makes agents **good at metrology work**, and **measures** how good they are:
- curated skills and recipes (program a part from PMI, set up an inspection, explain a difference, build a report template);
- the guardrails that keep agents within the human-adoption rule;
- a public **evaluation suite** of metrology tasks with scored results per model and release.

## Scope

**In:**
- Skills and recipes shipped with each MCP server (progressive disclosure, AUTO.MCP)
- Task-level prompts and worked examples drawn from the acceptance jobs
- Proposal UX contract: what an agent proposal contains so a human can adopt it quickly (with VIZ.INBOX)
- Evaluation suite: tasks, fixtures, scoring (correctness against golden journals, safety violations, human-adoption rate, cost), and a harness that runs any MCP-capable agent
- A published scoreboard per release and model
- Red-team tasks: prompt injection through file content, attempts to exceed side-effect classes, attempts to adopt own proposals
- Local and open-weight model guidance (offline shops)

**Out:**
- MCP server mechanics (AUTO.MCP)
- An LLM embedded in the engine (not in P0–P2, baseline §7)
- Safety enforcement itself (CORE.BUS); AGENT only tests it

## Requirements

- **REQ-AGENT-001** Each MCP server shall ship curated skills for its top workflows, versioned with the registry manifest. Every skill shall reference only commands that exist in that manifest.
- **REQ-AGENT-002** The project shall maintain an evaluation suite of metrology tasks derived from the acceptance jobs, each scored automatically against golden journals.
- **REQ-AGENT-003** The evaluation harness shall run any MCP-capable agent unchanged and report task success, safety violations, human-adoption rate, tokens and wall time.
- **REQ-AGENT-004** The suite shall include red-team tasks (prompt injection through imported content, attempts to exceed side-effect classes, self-adoption attempts). Any successful violation shall block release.
- **REQ-AGENT-005** Evaluation results shall be published per release and per model, with the exact suite version, and no capability shall be claimed in marketing without a published score.
- **REQ-AGENT-006** Every agent proposal shall carry a structured rationale, the affected characteristics and a predicted-impact summary, so a human can adopt or reject it from the Proposal Inbox without reading the raw diff.

## Interfaces

- **Offers:** skills, recipes, the eval harness and suite, the scoreboard, the proposal content contract.
- **Consumes:** AUTO (MCP), CORE (journal and adoption), FLOW (acceptance jobs as task sources).

## Decomposition hints

SKILLS (skills and recipes per server), PROPOSE (proposal content contract), EVAL (task suite and fixtures), HARNESS (agent-agnostic runner and scoring), REDTEAM (adversarial tasks), BOARD (published scoreboard), LOCAL (local and open-weight model guidance).

## Open questions

- Costs of running third-party models for the public scoreboard: who pays? Sponsorship may be needed.

## Split rationale

The split follows **what is being produced for or about an agent**: what it is taught (SKILLS), what it hands a human (PROPOSE), what it is tested on (EVAL), how the test runs (HARNESS), how it is attacked (REDTEAM), what is published (BOARD), and the offline baseline (LOCAL). The guiding constraint is that AGENT never enforces safety: enforcement stays in CORE.BUS, CORE.JRN.BR and AUTO.MCP, and AGENT only teaches, describes and attacks it. Children depend on specific AUTO and CORE nodes (AUTO.MCP.SKILLS, AUTO.MCP.TAINT, AUTO.MCP.MODES, AUTO.SURF.ROOTS, CORE.JRN.BR, CORE.JRN.STREAM, CORE.BUS.POL), never on VIZ.INBOX, so that VIZ.INBOX may later depend on AGENT.PROPOSE without a cycle.

Shared concerns sit in exactly one child: the task file format in EVAL (reused by REDTEAM), the proposal completeness verdict in PROPOSE (used by the Inbox and by HARNESS for adoptability), attempt and violation counting in HARNESS (whose predicates REDTEAM supplies), and publication in BOARD (LOCAL's guidance is generated from it).

Alternatives considered:
- **EVAL and HARNESS merged.** Rejected: the suite is versioned data that must stay hidden from agents under test, while the harness is a runner with an adapter contract; merging them makes an XXL node and blurs oracle isolation.
- **REDTEAM folded into EVAL.** Rejected: red-team tasks need violation predicates, a scripted adversary and a release-blocking verdict, none of which ordinary tasks have.
- **PROPOSE inside VIZ.INBOX.** Rejected: the brief is produced over MCP from P0, before the Inbox exists (P1a), and the eval harness needs it without a GUI.
- **LOCAL folded into BOARD.** Rejected: the reference offline agent is an adapter and CI configuration, not publication; C7 makes it worth an owner.

Open interface issue: D-032 (new) decides whether the eval tooling is a set of registry commands or a separate binary; it affects EVAL, HARNESS, BOARD and LOCAL interfaces but not this split.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| AGENT.SKILLS | Curated skill catalog and worked recipes | 001 | none |
| AGENT.PROPOSE | Proposal brief and computed impact | 006 | none |
| AGENT.EVAL | Evaluation task suite, oracles and fixtures | 002 | none |
| AGENT.HARNESS | Agent-agnostic evaluation runner and scoring | 002 003 | none |
| AGENT.REDTEAM | Red-team tasks and release-blocking verdict | 004 | none |
| AGENT.BOARD | Published scoreboard and claim register | 005 | none |
| AGENT.LOCAL | Reference local agent and open-weight model guidance | 003 005 | none |
