---
id: AGENT.LOCAL.CIRUN
title: Offline CI evaluation of the reference agent
level: 3
parent: AGENT.LOCAL
kind: task
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-AGENT.LOCAL-001, REQ-AGENT.LOCAL-002]
covers: []
depends_on: [AGENT.HARNESS.RUN, AGENT.HARNESS.RECORD, AGENT.REDTEAM.VERDICT]
uses_artifacts: [AGENT.LOCAL.REF, AGENT.BOARD.BUNDLE, ECO.CI.WF]
side_effect: W1
determinism: N
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# AGENT.LOCAL.CIRUN: Offline CI evaluation of the reference agent

## Purpose

On every release candidate, runs the reference local agent and the scripted adversary over the full eval and red-team suites in a CI job with networking removed, and places the run records where the scoreboard build picks them up. This is the project's baseline: one agent whose results never depend on a hosted service.

## Scope

**In:**
- The CI job definition (`ci/agent-eval-offline`) and its runner script
- Network isolation of the job
- Hand-off of run records and the red-team verdict to the board input folder

**Out** (and which node owns it instead):
- CI platform and workflow conventions (`ECO.CI.WF`)
- Harness, adversary and verdict (`AGENT.HARNESS.*`, `AGENT.REDTEAM.*`)
- Building the board (`AGENT.BOARD.BUNDLE`)

## Requirements

- **REQ-AGENT.LOCAL.CIRUN-001** When a release candidate is tagged, the job shall run `fid-eval run` with the reference adapter on every eval and red-team task at least 3 times, and the scripted adversary once on every red-team task.
- **REQ-AGENT.LOCAL.CIRUN-002** The job shall run the harness, `fid` and the agent in an environment with no network interface other than loopback, and shall fail if a connectivity probe to an external address succeeds.
- **REQ-AGENT.LOCAL.CIRUN-003** After the runs, the job shall produce the suite report, the red-team verdict and a `host.json` (CPU model, RAM, GPU model and memory, OS) and copy them with all run records and artifacts into `board-input/<release>/project/`.
- **REQ-AGENT.LOCAL.CIRUN-004** If the job cannot start the reference agent (missing weights, digest mismatch or insufficient hardware), then it shall fail visibly naming the cause and shall not publish partial results.

## Acceptance criteria

- **AC-1** Given a dry-run of the job on `suite-mini` with the `null` adapter substituted, when the job script runs on a CI runner, then `board-input/<rc>/project/` holds run records, the suite report and a verdict.
- **AC-2** Given the job environment, when the connectivity probe to an external address runs, then it fails to connect and the job continues; given a misconfigured runner with networking, then the job fails.
- **AC-3** (error path) Given missing weights, then the job fails with `reference_agent_unavailable` and writes nothing to `board-input/`.

## Design

- **Registry command(s):** none.
- **Data structures:** none new; uses run records, suite report and verdict formats.
- **Algorithm:** on RC tag: fetch the RC build; enter a network namespace with only loopback (Linux `unshare --net` or a container with `--network none`); run the reference agent (repeat 3) and the adversary; build report and verdict; leave the namespace; copy outputs.
- **Errors:** `reference_agent_unavailable`, `network_not_isolated`.
- **Determinism:** N (model runs); the adversary part is D0.
- **Performance budget:** whole job < 12 h on the CI reference host [unverified].
- **Crate:** none (CI job script)

## Test fixtures

`suite-mini` and the `null` adapter for the dry run; the full suites for real runs.

## Build prompt

> Add CI job `ci/agent-eval-offline` (following the project's CI workflow conventions) triggered on release-candidate tags. Inside a network namespace or container with loopback only (fail with `network_not_isolated` if a probe reaches an external address), run `fid-eval run` with `agents/local-ref/adapter.json` on all eval and red-team tasks with `--repeat 3`, run the scripted adversary once on every red-team task, then `fid-eval report` and `fid-eval redteam verdict`, and copy records, artifacts, report and verdict to `board-input/<release>/project/`. If the reference agent cannot start, fail with `reference_agent_unavailable` and publish nothing. Acceptance: AC-1 to AC-3 of AGENT.LOCAL.CIRUN. Constraints: CONSTITUTION C7; CI changes go in their own PR (AGENTS.md).

## Open questions

- Whether project CI hardware can host the reference model is unverified; a self-hosted runner may be needed (funding question raised at AGENT level).

## Children

_Leaf: none._
