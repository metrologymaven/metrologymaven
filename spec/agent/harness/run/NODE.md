---
id: AGENT.HARNESS.RUN
title: Run orchestration, MCP tap and budgets
level: 3
parent: AGENT.HARNESS
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
satisfies: [REQ-AGENT.HARNESS-003, REQ-AGENT.HARNESS-010]
covers: []
depends_on: [AGENT.HARNESS.ADAPT, AGENT.HARNESS.SANDBOX, AGENT.EVAL.FORMAT]
uses_artifacts: [AUTO.MCP.MODES, AUTO.MCP.SESS, CORE.JRN.STREAM]
side_effect: W1
determinism: N
cli: [fid-eval run]
mcp: []
journal: []
updated: 2026-11-17
---

# AGENT.HARNESS.RUN: Run orchestration, MCP tap and budgets

## Purpose

Runs one agent on one task: build the sandbox, start the agent through its adapter with a pass-through tap in front of a headless `fid mcp` session, enforce the wall-time and tool-call budgets, stop everything, and collect the run artifacts that scoring and safety counting read. It never changes what the agent sends or receives.

## Scope

**In:**
- `fid-eval run --suite <dir> --adapter <file> [--task <id>...] [--repeat N]`
- The MCP tap: a byte-exact stdio relay that logs every JSON-RPC message with a monotonic timestamp
- The `fid` CLI shim for shell-access runs (logs argv, exit code and JSON output, then calls the real `fid` on the sandbox project)
- Budget enforcement and stop
- Collecting artifacts: tap transcript, shim log, Record Journal, Activity Log, file snapshots, token report

**Out** (and which node owns it instead):
- Adapter schema (`AGENT.HARNESS.ADAPT`); sandbox build (`AGENT.HARNESS.SANDBOX`)
- Scoring (`AGENT.HARNESS.SCORE`); safety counting (`AGENT.HARNESS.SAFETY`); run records (`AGENT.HARNESS.RECORD`)

## Requirements

- **REQ-AGENT.HARNESS.RUN-001** The tap shall relay every byte between agent and `fid mcp` unchanged and shall log each JSON-RPC message with direction and a monotonic time offset in milliseconds.
- **REQ-AGENT.HARNESS.RUN-002** When the agent's tool calls (counted as `tools/call` requests in the tap log, plus shim invocations) exceed the task's tool-call budget, the tap shall answer further calls with a JSON-RPC error `budget_exhausted` and the run shall stop within 5 s.
- **REQ-AGENT.HARNESS.RUN-003** When the task's wall-time budget elapses, the harness shall terminate the agent's whole process tree and the `fid mcp` session, and record the stop reason `wall_time`.
- **REQ-AGENT.HARNESS.RUN-004** The stop reason shall be exactly one of `agent_exit`, `wall_time`, `tool_calls` or `harness_error`, and every stop reason other than `harness_error` shall proceed to scoring.
- **REQ-AGENT.HARNESS.RUN-005** When the adapter grants shell access, the harness shall put the `fid` shim first on the agent's PATH and shall log every shim invocation in the run artifacts.
- **REQ-AGENT.HARNESS.RUN-006** After stop, the harness shall copy the Record Journal, Activity Log, tap transcript, shim log, token report and both file snapshots into `run/<run-id>/artifacts/`, with a manifest of their SHA-256 digests.
- **REQ-AGENT.HARNESS.RUN-007** The harness process shall open no network socket and shall invoke no promotion, adoption or release command on the sandbox.
- **REQ-AGENT.HARNESS.RUN-008** With `--repeat N`, the harness shall run each selected task N times in fresh sandboxes, defaulting N to 3.

## Acceptance criteria

- **AC-1** Given the fixture scripted agent `fixtures/agent/harness/agents/two-calls` (a deterministic test client making 2 tool calls then exiting) on `suite-mini/t1`, when `fid-eval run` runs, then the tap log holds both calls and replies byte-identical to what the agent and server exchanged, and the stop reason is `agent_exit`.
- **AC-2** Given fixture agent `loop-calls` and a tool-call budget of 10, then call 11 gets `budget_exhausted` and the run stops with `tool_calls` within 5 s.
- **AC-3** Given fixture agent `sleeper` and a wall-time budget of 3 s, then the run stops with `wall_time` and no agent process remains.
- **AC-4** Given a shell-access adapter and fixture agent `shell-fid` that runs `fid journal list-branches --json`, then the shim log records the argv and exit code.
- **AC-5** Given a run under an OS-level network monitor fixture (Linux network namespace with no interfaces), then the harness process makes no connection attempt.

## Design

- **Registry command(s):** none (D-032). CLI `fid-eval run`.
- **Data structures:** `RunArtifacts { run_id, task_id, stop: StopReason, tap_log, shim_log?, record_journal, activity_log, fs_before, fs_after, tokens?, digests }`.
- **Algorithm:** build sandbox; write prompt file and MCP config; start the agent with the adapter argv and private environment; the MCP config's command is `fid-eval tap -- fid mcp --headless --server <profile> --project <sandbox>`; the tap counts calls and shares the count with the supervisor; supervise with timers; on stop, kill the process tree (job object on Windows, process group on Unix), wait for `fid mcp` to exit cleanly or kill it after 5 s; collect artifacts.
- **Errors:** `launch_failed`, `harness_error { detail }`.
- **Determinism:** N (the agent is an external, usually stochastic process); artifacts are hashed so later steps are D0.
- **Performance budget:** harness overhead < 50 ms per tool call through the tap [unverified].
- **Crate:** `fid-eval-harness` (binary `fid-eval`)

## Test fixtures

Project-authored deterministic fixture agents (`two-calls`, `loop-calls`, `sleeper`, `shell-fid`) under `fixtures/agent/harness/agents/`, and `suite-mini`.

## Build prompt

> In crate `fid-eval-harness` (Tooling; links no product crates; drives `fid` only as a process), implement `fid-eval run --suite <dir> --adapter <file> [--task <id>...] [--repeat N=3]`. For each task and repetition: build a fresh sandbox; write the prompt and the MCP client config whose server command is `fid-eval tap -- fid mcp --headless --server <profile> --project <sandbox>`; start the agent per adapter. The tap relays stdio byte-for-byte and logs every JSON-RPC message with direction and ms offset; it counts `tools/call` (plus shim calls) and, past the budget, answers `budget_exhausted`. Enforce wall time by killing the agent's process tree. With shell access, put a logging `fid` shim first on PATH. Stop reasons: `agent_exit|wall_time|tool_calls|harness_error`. Collect journal, Activity Log, tap and shim logs, token report and before/after file snapshots into `artifacts/` with a digest manifest. Open no network socket; never promote, adopt or release. Acceptance: AC-1 to AC-5 of AGENT.HARNESS.RUN. Constraints: CONSTITUTION §8 (no panic, deny `unwrap` etc.); `unsafe` only in a named process-control shim if an OS API requires it.

## Open questions

- D-031 sends R0 and R1 calls to the Activity Log only while a recording session is active. The tap transcript is the authoritative tool-call record here; if Activity Log entries are wanted too, `fid mcp --headless` needs a way to start with recording active (an AUTO.MCP.MODES question).
- Bus refusals commit nothing (REQ-CORE.BUS.POL-001), so refused attempts are visible only as error replies in the tap and shim logs.

## Children

_Leaf: none._
