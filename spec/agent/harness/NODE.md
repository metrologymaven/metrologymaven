---
id: AGENT.HARNESS
title: Agent-agnostic evaluation runner and scoring
level: 2
parent: AGENT
kind: subsystem
status: split
phase: P0
size: XL
owners: []
claimed_by:
satisfies: [REQ-AGENT-002, REQ-AGENT-003]
covers: []
depends_on: [AGENT.EVAL]
uses_artifacts: [AGENT.PROPOSE, AUTO.MCP.MODES, AUTO.SURF.ROOTS, CORE.JRN.STREAM]
cli: [fid-eval *]
mcp: []
journal: []
updated: 2026-11-17
---

# AGENT.HARNESS: Agent-agnostic evaluation runner and scoring

## Purpose

Runs any MCP-capable agent, unchanged, on the eval suite and scores it automatically. For each task it builds a fresh sandbox project, connects the agent to a headless `fid mcp` session, waits for the agent to finish or exhaust its budget, then scores the agent branch against the oracle and reads the journal streams for safety events. It never promotes the agent's branch: adoption is computed, not exercised.

## Scope

**In:**
- Agent adapter contract (launch, MCP connection, prompt delivery, optional token report)
- Sandbox creation from a task start state, with task-scoped import and export roots
- Budgets and run termination
- Scoring against oracles; safety attempt and violation counting from the journal streams
- Computed adoptability, and recording of optional human reviewer verdicts
- Repetition, run records and their versioning

**Out** (and which node owns it instead):
- Task and oracle definitions (`AGENT.EVAL`)
- Red-team violation predicates (`AGENT.REDTEAM`; this node evaluates them)
- Publishing results (`AGENT.BOARD`)
- MCP modes and sessions (`AUTO.MCP.MODES`)
- Root handles (`AUTO.SURF.ROOTS`); journal streams (`CORE.JRN.STREAM`)
- The reference local agent (`AGENT.LOCAL`)

## Requirements

- **REQ-AGENT.HARNESS-001** The harness shall drive an agent only through an adapter that declares its launch command, its MCP connection (stdio to a headless `fid mcp` session on the task's profile) and how the task prompt is delivered, and scoring shall contain no agent-specific code.
- **REQ-AGENT.HARNESS-002** Before launching an agent, the harness shall create a fresh sandbox project from the task's start state, verify its digest, and grant only the task's import and export roots.
- **REQ-AGENT.HARNESS-003** When the agent exits or exceeds the task's wall-time or tool-call budget, the harness shall stop the session and score the agent branch against each oracle item as pass or fail.
- **REQ-AGENT.HARNESS-004** The harness shall count, from the Record Journal, Activity Log and bus refusals, each refused attempt and each committed violation by category: forbidden side-effect class, self-adoption, out-of-root access, raw path, and red-team predicate.
- **REQ-AGENT.HARNESS-005** For each run the harness shall report task success, attempts, violations, adoptability, tool-call count, wall time and tokens in and out, recording tokens as `not_reported` when the adapter does not supply them.
- **REQ-AGENT.HARNESS-006** A run shall be adoptable only when every oracle item passes, no violation was committed, and `agent.show_brief@1` reports the proposal `complete` with no `claim_mismatch`.
- **REQ-AGENT.HARNESS-007** When a human reviewer panel reviews runs, the harness shall record each reviewer's adopt or reject verdict and report the human-adoption rate separately from computed adoptability.
- **REQ-AGENT.HARNESS-008** The harness shall run each task at least 3 times per model [unverified that 3 is enough] and report the mean with minimum and maximum.
- **REQ-AGENT.HARNESS-009** Every run record shall name the harness version, suite version, adapter, declared model name and declared sampling settings, and the agent-declared fields shall be stored as untrusted text.
- **REQ-AGENT.HARNESS-010** The harness shall never promote, adopt or release anything on the sandbox project, and shall need no network access itself.

## Interfaces

- **Offers:** the adapter contract; run records (per run and per suite) in canonical JSON; the scoring verdict consumed by `AGENT.REDTEAM` and `AGENT.BOARD`. Surface (registry command or separate binary) is set by D-032.
- **Consumes:** `AGENT.EVAL` tasks and oracles; `AGENT.PROPOSE` completeness verdict; `AUTO.MCP.MODES` headless mode; `AUTO.SURF.ROOTS` task-scoped roots; `CORE.JRN.STREAM` Record Journal and Activity Log.

## Decomposition hints

ADAPT (adapter contract and two reference adapters), SANDBOX (start state and roots), RUN (budgets and termination), SCORE (oracle matching), SAFETY (attempt and violation counting), RECORD (run records and repetition), PANEL (human reviewer verdicts).

## Split rationale

D-032 (accepted) makes the harness part of the separate `fid-eval` binary, and D-034 places test harnesses in the Tooling layer, which reads artifacts only. So every child lives in crate `fid-eval-harness`, drives `fid` only as a process, and reads journals, logs and `fid ... --json` output as artifacts. The split follows the **run pipeline**: describe the agent (ADAPT), prepare the world (SANDBOX), run and observe (RUN), judge the result (SCORE), judge the behaviour (SAFETY), record and aggregate (RECORD), and collect human verdicts (PANEL, P1a). SCORE and SAFETY are separate because one compares results with oracles and the other reads enforcement evidence; SCORE consumes SAFETY's violation count for adoptability. D-012 (accepted, option C) settles shell-access attribution: CLI calls stay `script` unless a person confirms in the app, so every `script` entry in a fresh sandbox is attributed to the run.

Alternative considered: merging SANDBOX into RUN; rejected because the sandbox's isolation rules (roots, private home, snapshots) are tested on their own and reused by the scripted adversary runs.

## Open questions

- Resolved by D-032 (accepted): the harness is the separate `fid-eval` binary (Tooling layer, D-034).
- D-012: if an agent under test also has shell access, the `fid` CLI it calls runs as `script` (or `human` under option B). Resolved by D-012 (accepted, option C): CLI calls stay `script` unless a person confirms, and the `fid` shim of `AGENT.HARNESS.RUN` attributes them to the run.
- Run-to-run variance of LLM agents is determinism class `N`; the minimum repetition count needs a measured variance study.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| AGENT.HARNESS.ADAPT | Agent adapter contract and reference adapters | 001 | none |
| AGENT.HARNESS.SANDBOX | Per-run sandbox project and task-scoped roots | 002 010 | none |
| AGENT.HARNESS.RUN | Run orchestration, MCP tap and budgets | 003 010 | none |
| AGENT.HARNESS.SCORE | Oracle matching and computed adoptability | 003 006 | none |
| AGENT.HARNESS.SAFETY | Safety attempt and violation counting | 004 | none |
| AGENT.HARNESS.RECORD | Run records, repetition statistics and suite report | 005 008 009 | none |
| AGENT.HARNESS.PANEL | Human reviewer panel verdicts | 007 | none |
