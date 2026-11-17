---
id: AGENT.LOCAL
title: Reference local agent and open-weight model guidance
level: 2
parent: AGENT
kind: subsystem
status: split
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-AGENT-003, REQ-AGENT-005]
covers: []
depends_on: [AGENT.HARNESS]
uses_artifacts: [AGENT.BOARD]
cli: [fid-eval local *]
mcp: []
journal: []
updated: 2026-11-17
---

# AGENT.LOCAL: Reference local agent and open-weight model guidance

## Purpose

Serves offline shops (C7): a reference agent configuration built from an open-weight model and a local MCP client, which the harness runs with no network as its baseline adapter, and generated guidance telling a shop which open-weight models can do which workflows on what hardware. It proves that "any MCP-capable agent" includes one a shop can run entirely on its own machines.

## Scope

**In:**
- The reference local agent configuration (open-weight model, local MCP client, adapter file)
- Running it on the full suite with no network in project CI
- Generated guidance per open-weight model: scores, hardware used, offline install steps, workflows not to delegate

**Out** (and which node owns it instead):
- The adapter contract and scoring (`AGENT.HARNESS`)
- Publishing scores (`AGENT.BOARD`; this node's guidance is generated from board data)
- Shipping or bundling model weights (not done; guidance points to the model's own distribution)
- An LLM embedded in the engine (out of scope until after P2, baseline §7)

## Requirements

- **REQ-AGENT.LOCAL-001** The project shall maintain at least one reference local agent configuration that runs the full eval and red-team suites with no network access.
- **REQ-AGENT.LOCAL-002** Project CI shall run the reference local agent on every release candidate and publish its results to the scoreboard [unverified that CI hardware can host the model].
- **REQ-AGENT.LOCAL-003** The guidance shall be generated from scoreboard data and shall list, for each evaluated open-weight model, its scores per workflow, the hardware it ran on (CPU, RAM, GPU memory) and its licence as declared by the model publisher.
- **REQ-AGENT.LOCAL-004** For each open-weight model, the guidance shall list every curated workflow whose success rate is below 80 % [unverified threshold] as not recommended for delegation.
- **REQ-AGENT.LOCAL-005** The reference configuration shall need no account, no telemetry and no download at run time beyond a one-time model install that the user starts.

## Interfaces

- **Offers:** the reference adapter file and setup notes; the generated guidance page.
- **Consumes:** `AGENT.HARNESS` adapter contract and runs; `AGENT.BOARD` scores.

## Decomposition hints

REF (reference configuration and adapter), CI (offline CI runs), GUIDE (generated guidance page).

## Split rationale

Split by **artifact**: the reference agent's configuration (REF, data and a launch wrapper), the offline CI job that runs it on each release candidate (CIRUN), and the guidance page generated from the scoreboard (GUIDE, a `fid-eval` subcommand per D-032). The specific model and client stay a maintainer choice made in review against REF's written criteria, so no decision record is needed now.

## Open questions

- Which open-weight model and MCP client to use as the reference is a maintainer choice at P1a time, constrained by licence (D-001 allowlist does not cover model weights) [unverified].
- Resolved by D-032 (accepted): the harness is the separate `fid-eval` binary; CI invokes it as in `AGENT.LOCAL.CIRUN`.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| AGENT.LOCAL.REF | Reference local agent configuration | 001 005 | none |
| AGENT.LOCAL.CIRUN | Offline CI evaluation of the reference agent | 001 002 | none |
| AGENT.LOCAL.GUIDE | Generated open-weight model guidance page | 003 004 | none |
