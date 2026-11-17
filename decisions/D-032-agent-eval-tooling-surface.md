---
id: D-032
title: Surface and side-effect class of the agent evaluation tooling
status: accepted
raised_by: AGENT level-2 split
blocks: []
decide_by: before AGENT.HARNESS or AGENT.EVAL is split
updated: 2026-11-17
---

# D-032: Surface and side-effect class of the agent evaluation tooling

## Question

Are the agent evaluation harness and suite tools (run an agent on eval tasks, check the suite, recompute a published score) **registry commands** under `agent.*`, or a **separate binary** outside the registry, like `fid-verify`?

## Context

REQ-AGENT-003 asks for a harness that launches any MCP-capable agent and scores it. Three rules collide:

- CONSTITUTION C3: every change to document, record or configuration state is a registry command, and MCP tools are generated from the registry.
- REQ-AUTO.MCP.SRV-002: every projectable command is served by at least one MCP server profile. A W1 or R0 `agent.run_eval@1` would therefore reach agents over MCP.
- Running an eval launches an external executable (the agent under test) and reads task oracles. An agent that can call it could launch arbitrary processes or read oracles, which defeats the evaluation. D-007 classes plugin install (running third-party code) as A0.

CONSTITUTION §2 lists the binaries (`fid`, one desktop app per focused app, `fid-verify`); adding one is an architecture change for the steering group.

Affected nodes: `AGENT.HARNESS`, `AGENT.EVAL`, `AGENT.BOARD` (recompute check), `AGENT.LOCAL`.

## Options

| Option | For | Against |
|---|---|---|
| A. Separate binary `fid-eval`, outside the registry. Its outputs are run records and reports (files), never project state; every change to the sandbox projects still goes through the bus because the agent under test makes it | No MCP exposure by construction; oracles never in the manifest; mirrors `fid-verify` | New binary in CONSTITUTION §2; parent cli pattern `fid agent *` then covers only proposal and recipe commands |
| B. Registry commands, `agent.run_eval@1` classed A0 (never on MCP) and suite queries R0 excluded from every server profile | One CLI; generated docs | Needs an amendment to REQ-AUTO.MCP.SRV-002 (a per-command "not served" flag); stretches A0 beyond admin and safety configuration |
| C. Registry W1 commands exposed to MCP like any other | Simplest | Agents can launch processes and read oracles; rejected on safety grounds |

## Recommendation

A. Evaluation is test infrastructure, not product state, and keeping it out of the manifest removes the oracle-leak and process-launch paths entirely [unverified that no product workflow needs to start an eval run].

## Decision

**Accepted (recommendation as written above), 2026-11-17, founder.**

A. Evaluation is test infrastructure, not product state, and keeping it out of the manifest removes the oracle-leak and process-launch paths entirely [unverified that no product workflow needs to start an eval run].
