---
id: AGENT.LOCAL.REF
title: Reference local agent configuration
level: 3
parent: AGENT.LOCAL
kind: task
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-AGENT.LOCAL-001, REQ-AGENT.LOCAL-005]
covers: []
depends_on: []
uses_artifacts: [AGENT.HARNESS.ADAPT]
side_effect: R0
determinism: N
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# AGENT.LOCAL.REF: Reference local agent configuration

## Purpose

A versioned configuration that turns an open-weight model plus a local MCP client into an agent the harness can run with no network: the adapter file, the client configuration, the model's identity and digest, and offline setup notes. It proves "any MCP-capable agent" includes one a shop can run entirely on its own machines.

## Scope

**In:**
- `agents/local-ref/adapter.json`, client configuration, `model.json` (model name, publisher, declared licence, weight file SHA-256, quantisation, context length, sampling settings), `SETUP.md`
- Selection criteria for the reference model and client
- A config check that validates these files

**Out** (and which node owns it instead):
- Adapter contract (`AGENT.HARNESS.ADAPT`)
- CI runs (`AGENT.LOCAL.CIRUN`)
- Shipping weights (not done; `SETUP.md` points to the publisher's distribution)

## Requirements

- **REQ-AGENT.LOCAL.REF-001** The configuration shall name one open-weight model by publisher, name, version and weight-file SHA-256, and one local MCP client by name and version, and shall pin sampling settings including a fixed seed where the runtime supports one.
- **REQ-AGENT.LOCAL.REF-002** The adapter shall pass `fid-eval adapter check` and shall launch the agent with no network access needed at run time, no account and no telemetry.
- **REQ-AGENT.LOCAL.REF-003** `SETUP.md` shall describe a one-time, user-started model install from the publisher's distribution and a way to verify the weight digest before first use.
- **REQ-AGENT.LOCAL.REF-004** The chosen model's licence, as declared by its publisher, shall be recorded in `model.json`, and the choice shall be made by maintainers in a reviewed PR against written selection criteria (licence permits local commercial use, runs on the CI reference host, supports tool calling).
- **REQ-AGENT.LOCAL.REF-005** If the weight file at run time does not match the recorded SHA-256, then the adapter's launch wrapper shall exit before the agent starts and the run shall end with `harness_error`.

## Acceptance criteria

- **AC-1** Given `agents/local-ref/`, when `fid-eval adapter check agents/local-ref/adapter.json` runs, then it passes.
- **AC-2** Given the reference agent run on `suite-mini/t1` inside a Linux network namespace with no interfaces, then the run completes with stop reason `agent_exit` or a budget reason, never a network error.
- **AC-3** (error path) Given a weight file with one byte changed, then the launch wrapper exits and the run reports `harness_error`.

## Design

- **Registry command(s):** none.
- **Data structures:** `ModelCard { publisher, name, version, licence_declared, weights_sha256, quantisation, context_tokens, sampling: { temperature, top_p, seed? } }` (all strings stored untrusted when copied into run records).
- **Algorithm:** a small launch wrapper verifies the weight digest, starts the local inference runtime bound to a loopback or Unix socket (no external interface), then starts the MCP client with the harness-provided MCP config.
- **Errors:** `weights_mismatch`.
- **Determinism:** N (model output); configuration validation is D0.
- **Performance budget:** must fit the CI reference host [unverified until the host is chosen].
- **Crate:** none (configuration files plus a launch script in `agents/local-ref/`)

## Test fixtures

`suite-mini` from `AGENT.EVAL.FORMAT` fixtures; the model weights are obtained by the CI host operator, never committed.

## Build prompt

> Create `agents/local-ref/` in the evaluation repository: `adapter.json` (harness adapter format), a local MCP client configuration, `model.json` (publisher, name, version, declared licence, weight SHA-256, quantisation, context length, pinned sampling with a seed if supported), a launch wrapper that verifies the weight digest (`weights_mismatch` -> exit non-zero) and starts the local runtime on loopback only, and `SETUP.md` (one-time user-started install from the publisher, digest verification, no account, no telemetry). The specific model and client are chosen by maintainers in review against the criteria: licence permits local commercial use, runs on the CI reference host, supports tool calling. Acceptance: AC-1 to AC-3 of AGENT.LOCAL.REF. Constraints: CONSTITUTION C7 (offline, no account, no telemetry); never commit weights.

## Open questions

- The reference model and client are a maintainer choice at P1a; the D-001 licence allowlist covers code dependencies, not model weights [unverified], so the model's licence is checked against the criteria above in review.

## Children

_Leaf: none._
