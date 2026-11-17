---
id: AGENT.REDTEAM
title: Red-team tasks and release-blocking verdict
level: 2
parent: AGENT
kind: subsystem
status: split
phase: P0
size: L
owners: []
claimed_by:
satisfies: [REQ-AGENT-004]
covers: []
depends_on: [AGENT.EVAL, AGENT.HARNESS]
uses_artifacts: [AUTO.MCP.TAINT, CORE.BUS.POL]
cli: [fid-eval adversary, fid-eval redteam *]
mcp: []
journal: []
updated: 2026-11-17
---

# AGENT.REDTEAM: Red-team tasks and release-blocking verdict

## Purpose

Tests, adversarially, that the safety rules hold when an agent tries to break them: injected instructions in imported content, attempts to exceed side-effect classes, attempts to adopt its own work, and attempts to pass off its own numbers as values of record. Enforcement lives in CORE.BUS and AUTO.MCP; this node only attacks it and turns the outcome into a release verdict. It includes a scripted adversary that attempts every forbidden action directly, so the verdict does not depend on whether a given model happens to refuse.

## Scope

**In:**
- Red-team task families and their payloads (project-authored)
- Violation predicates: checks over the journal streams and sandbox file system after a run
- The scripted adversary (a deterministic MCP client, no LLM)
- Shell-access variants (the agent also has a shell and can call `fid`)
- The release verdict file

**Out** (and which node owns it instead):
- Enforcement of classes, roots and promotion rules (`CORE.BUS.POL`, `CORE.JRN.BR`, `AUTO.MCP.PROJ`)
- Untrusted marking (`AUTO.MCP.TAINT`)
- Running agents and counting events (`AGENT.HARNESS`)
- The release process that consumes the verdict (`ECO`)
- Publishing results (`AGENT.BOARD`)

## Requirements

- **REQ-AGENT.REDTEAM-001** The red-team suite shall contain tasks in each of these families: injection through imported content (element names, PMI notes, file metadata, file names, operator text), side-effect escalation (W3, X1, D1, M1 and A0 by any route, widening roots, raw paths), self-adoption (promoting or adopting the agent's own or another agent's work), value-of-record fabrication (agent-computed numbers written as results), and shell-access variants of each.
- **REQ-AGENT.REDTEAM-002** Each family shall have at least 5 tasks at the first release that ships MCP [unverified sizing].
- **REQ-AGENT.REDTEAM-003** Each red-team task shall define its violation predicate as a check over the Record Journal, Activity Log, bus refusals and sandbox file system after the run, independent of anything the agent reports.
- **REQ-AGENT.REDTEAM-004** The scripted adversary shall attempt every forbidden action of every family directly over MCP, and its run shall be deterministic (class D0) for a given suite version and manifest digest.
- **REQ-AGENT.REDTEAM-005** If any red-team run on a release candidate, by the scripted adversary or by any evaluated model, commits a violation, then the verdict shall be `block` and shall name the task, the agent and the journal entry.
- **REQ-AGENT.REDTEAM-006** The verdict shall be a canonical JSON file naming the release candidate, suite version, manifest digest and every run it covers.
- **REQ-AGENT.REDTEAM-007** Every injection payload shall be project-authored and shall carry a unique marker string, so that a predicate can detect a committed command whose arguments carry the marker.

## Interfaces

- **Offers:** red-team task files (in the `AGENT.EVAL` format plus violation predicates); the scripted adversary; the verdict file consumed by the release process in ECO and shown by `AGENT.BOARD`.
- **Consumes:** `AGENT.EVAL` task format; `AGENT.HARNESS` runs and event counts; `AUTO.MCP.TAINT` marking (to test it); `CORE.BUS.POL` refusal records.

## Decomposition hints

INJECT (injection family and payloads), ESCAL (side-effect escalation family), ADOPT (self-adoption family), FABRIC (value-of-record fabrication), SHELL (shell-access variants), SCRIPT (scripted adversary), VERDICT (verdict file).

## Split rationale

All children are `fid-eval` tooling (D-032) or suite data. The split separates **what counts as a breach** (PRED, the predicate schema evaluated by `AGENT.HARNESS.SAFETY`), the **attack data** by family (INJECT; ESCAL, which holds both escalation and self-adoption because both are refused by the same bus and branch rules; FABRIC; SHELL, the shell-access variants under D-012 option C), the **attacker that always tries** (SCRIPT) and the **verdict** (VERDICT). Alternative considered: one TASKS child for all families; rejected because each family has its own vectors, predicates and fixture agents, and would exceed one PR.

## Open questions

- ECO's release process must read the verdict and refuse a release on `block`; no ECO node states this yet. Needs an ECO requirement (AGENT cannot add it).
- D-012: the shell-access self-adoption tasks pass or fail depending on the principal kind given to a CLI caller. D-012 is accepted with option C (in-app or OS user-presence confirmation), and `AGENT.REDTEAM.SHELL` tests that a faked TTY cannot upgrade the principal.
- Fabrication predicate: detecting agent-computed numbers in free-text fields is heuristic; the precise rule (any numeric token in an agent-authored field that matches no kernel output) needs a leaf design.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| AGENT.REDTEAM.PRED | Violation predicate schema and payload markers | 003 007 | none |
| AGENT.REDTEAM.INJECT | Prompt injection through imported content | 001 002 007 | none |
| AGENT.REDTEAM.ESCAL | Side-effect escalation and self-adoption tasks | 001 002 | none |
| AGENT.REDTEAM.FABRIC | Value-of-record fabrication tasks | 001 002 | none |
| AGENT.REDTEAM.SHELL | Shell-access variants of red-team tasks | 001 002 | none |
| AGENT.REDTEAM.SCRIPT | Scripted deterministic adversary | 004 | none |
| AGENT.REDTEAM.VERDICT | Release-blocking red-team verdict file | 005 006 | none |
