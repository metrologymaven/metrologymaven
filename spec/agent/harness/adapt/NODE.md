---
id: AGENT.HARNESS.ADAPT
title: Agent adapter contract and reference adapters
level: 3
parent: AGENT.HARNESS
kind: task
status: leaf
phase: P0
size: S
owners: []
claimed_by:
built_in: [fid-eval-harness]
satisfies: [REQ-AGENT.HARNESS-001]
covers: []
depends_on: [AGENT.EVAL.FORMAT]
uses_artifacts: [AUTO.MCP.MODES]
side_effect: R0
determinism: D0
cli: [fid-eval adapter check]
mcp: []
journal: []
updated: 2026-11-17
---

# AGENT.HARNESS.ADAPT: Agent adapter contract and reference adapters

## Purpose

Defines the one file that tells the harness how to run a particular agent, so the harness can run any MCP-capable agent unchanged and scoring never contains agent-specific code. Ships two reference adapters: a generic executable adapter and a `null` agent used to calibrate scoring.

## Scope

**In:**
- The adapter file schema (`adapter.json`) and its validator
- Generating the MCP client configuration the agent receives
- Reference adapters `exec` (template) and `null`

**Out** (and which node owns it instead):
- Launching, budgets and termination (`AGENT.HARNESS.RUN`)
- The reference local agent's adapter (`AGENT.LOCAL.REF`)
- The scripted red-team adversary (`AGENT.REDTEAM.SCRIPT`)

## Requirements

- **REQ-AGENT.HARNESS.ADAPT-001** An adapter file shall declare adapter name and version, the launch argv with placeholders `{prompt_file}`, `{mcp_config}` and `{workdir}`, the prompt delivery mode (`stdin`, `file` or `arg`), an optional token-report file name, the declared model name and declared sampling settings, and whether the agent gets shell access.
- **REQ-AGENT.HARNESS.ADAPT-002** The harness shall give the agent exactly one MCP server, described in a generated client configuration whose command runs the harness tap in front of `fid mcp --headless --server <task profile>` on the sandbox project.
- **REQ-AGENT.HARNESS.ADAPT-003** If an adapter file has an unknown field, an unknown placeholder, a launch argv that is empty, or an absolute path into the suite folder, then `fid-eval adapter check` shall fail with `invalid_adapter` naming each problem.
- **REQ-AGENT.HARNESS.ADAPT-004** A token report, when declared, shall be a JSON file `{ "tokens_in": n, "tokens_out": n }` written by the agent; if it is absent or malformed after the run, the harness shall record tokens as `not_reported`.
- **REQ-AGENT.HARNESS.ADAPT-005** The `null` reference adapter shall exit without any tool call, so that a scoring run with it yields zero task successes and zero violations on every task.

## Acceptance criteria

- **AC-1** Given `fixtures/agent/harness/adapters/exec-example.json`, when `fid-eval adapter check` runs, then it passes and prints the resolved argv for a fixture task.
- **AC-2** Given the same adapter and fixture task `suite-mini/t1`, then the generated MCP config equals `fixtures/agent/harness/mcp-config.golden.json` except the sandbox path.
- **AC-3** (error path) Given an adapter with placeholder `{oracle}`, then the check fails with `invalid_adapter`.
- **AC-4** Given a malformed token report, then the parsed token fields are `not_reported`.

## Design

- **Registry command(s):** none (D-032). CLI `fid-eval adapter check <adapter.json> [--task <dir>]`.
- **Data structures:** `Adapter { name, version, argv: Vec<String>, prompt: Stdin|File|Arg, token_report?: FileName, model: String, settings: BTreeMap<String, String>, shell: bool }`; `model` and `settings` are agent-declared and stored as untrusted text.
- **Algorithm:** strict parse; placeholder substitution only for the three names; MCP config written in the common `mcpServers` JSON shape used by MCP clients [unverified that one shape fits all clients; adapters may name a config template instead].
- **Errors:** `invalid_adapter`, `missing_placeholder`.
- **Determinism:** D0.
- **Performance budget:** negligible.
- **Crate:** `fid-eval-harness` (Tooling; no product crate dependencies)

## Test fixtures

Project-authored adapters and golden MCP config under `fixtures/agent/harness/`.

## Build prompt

> In crate `fid-eval-harness` (Tooling layer; depends on `fid-eval-suite`, never on product `fid-*` crates), define the adapter file: name, version, launch argv with placeholders `{prompt_file}`, `{mcp_config}`, `{workdir}`, prompt delivery `stdin|file|arg`, optional token-report file name, declared model and sampling settings (untrusted text), and a shell-access flag. Generate the MCP client config naming one server whose command is the harness tap in front of `fid mcp --headless --server <profile>` on the sandbox project. Add `fid-eval adapter check` (fail `invalid_adapter` on unknown fields or placeholders, empty argv, or paths into the suite folder). Ship `adapters/exec-example.json` and `adapters/null.json` (exits immediately). Acceptance: AC-1 to AC-4 of AGENT.HARNESS.ADAPT. Constraints: CONSTITUTION §8 (no panic, deny `unwrap` etc.); never pass oracle paths to an agent.

## Open questions

- MCP clients differ in configuration format [unverified]; adapters may need a config-template field.

## Children

_Leaf: none._
