---
id: ECO.COMM.GROW
title: Cross-platform tree-growth driver
level: 3
parent: ECO.COMM
kind: task
status: leaf
phase: P0
size: S
owners: []
claimed_by:
satisfies: [REQ-ECO.COMM-008]
covers: []
depends_on: [ECO.COMM.SCOPE]
side_effect: W1
determinism: N
cli: []
mcp: []
journal: []
built_in: [tools/]
built_pr: 517
updated: 2026-11-22
---

# ECO.COMM.GROW: Cross-platform tree-growth driver

## Purpose

A Python replacement for `tools/grow.ps1` that runs on Windows, Linux and macOS: it lists open nodes with `scope.py open`, starts one coding-agent process per node with the expand-node prompt (a canary first, then in parallel), optionally runs the level check, and finally runs `scope.py check`. It never commits or pushes.

## Scope

**In:**
- `tools/grow.py` with the same options as `grow.ps1`
- Canary-first execution, a parallelism limit, per-node logs and an authentication-failure detector
- A test seam that replaces the agent executable with a fake

**Out:**
- The validator and open listing (owned by `ECO.COMM.SCOPE`)
- The prompts themselves (`prompts/`, maintained with this node's parent `ECO.COMM`)
- Committing, pushing or opening PRs (people do that; AGENTS.md rule 8)

## Requirements

- **REQ-ECO.COMM.GROW-001** The driver shall accept `--level`, `--limit`, `--parallel` (default 3), `--agent claude|codex`, `--model`, `--dry-run` and `--no-level-check`, with the same meaning as `grow.ps1`.
- **REQ-ECO.COMM.GROW-002** The driver shall run the first node alone, and if its log matches an authentication-failure pattern, then it shall stop with exit code 2 without starting other nodes.
- **REQ-ECO.COMM.GROW-003** While nodes remain, the driver shall keep at most `--parallel` agent processes running.
- **REQ-ECO.COMM.GROW-004** When all agents finish, the driver shall run `scope.py check` and exit with its exit code, and it shall never invoke `git commit` or `git push`.
- **REQ-ECO.COMM.GROW-005** When the target level is 1 or 2, the driver shall print that splits need human review in a PR.
- **REQ-ECO.COMM.GROW-006** The driver shall use only the Python standard library.

## Acceptance criteria

- **AC-1** Given fixture tree `fixtures/grow/tree/` with three open level-2 nodes, when `python tools/grow.py --dry-run --root fixtures/grow/tree` runs, then it prints the three node paths and the human-review note and starts no process.
- **AC-2** Given the same tree and `--agent-cmd fixtures/grow/fake_agent.py` (writes "ok" and sleeps 0.2 s), when `grow.py --parallel 2 --no-level-check` runs, then three logs exist under `generated/logs/`, the fake reports at most two concurrent runs, and the canary finished before the others started.
- **AC-3** (error path) Given `fixtures/grow/fake_agent_authfail.py` (prints "Invalid API key"), when `grow.py` runs, then it exits 2 after exactly one agent run.
- **AC-4** Given AC-2's run, when it finishes, then `scope.py check` ran last and `git` was never invoked (the test puts a failing `git` shim first on `PATH`).
- **AC-5** The AC-1 to AC-4 tests pass on Windows, Linux and macOS in CI.

## Design

- **Registry command(s):** none; repository tool `python tools/grow.py [options]`. `side_effect: W1` because the agents it starts edit the working tree (an agent branch), never `main`.
- **Data structures:** none beyond the node list and job table.
- **Algorithm:** call `scope.py open` as a subprocess; build each agent command (`claude -p <msg> --permission-mode acceptEdits --allowedTools ...` or `codex exec --full-auto <msg>`); run the canary, check its log, then a bounded pool via `subprocess.Popen` and polling; then the level-check prompt unless disabled; then `scope.py check`. A hidden `--agent-cmd` option substitutes a test executable; `--root` selects the tree.
- **Errors:** exit 2 on authentication failure; exit code of `scope.py check` otherwise.
- **Determinism:** N; agent output is not deterministic. The driver's own scheduling is deterministic in order.
- **Performance budget:** driver overhead under 1 s per node [unverified].
- **Crate:** not Rust; `tools/grow.py` in the spec repository, standard library only.

## Test fixtures

A small fixture tree and two fake agent scripts under `tools/tests/fixtures/grow/` written for this leaf.

## Build prompt

> In the spec repository, write `tools/grow.py` (Python 3.9+, standard library only) to replace `tools/grow.ps1` on all OSes. Options: `--level`, `--limit`, `--parallel` (3), `--agent claude|codex`, `--model`, `--dry-run`, `--no-level-check`, plus test-only `--root` and `--agent-cmd`. Behaviour: list nodes via `python tools/scope.py open [--level] [--limit]`; print them; if the target level ≤ 2 print that splits need human review in a PR; on dry-run stop. Otherwise write logs to `generated/logs/<node path with / replaced by _>.log`; run the first node alone and exit 2 if its log matches `Failed to authenticate|OAuth access token is invalid|not logged in|Invalid API key`; then run the rest with at most `--parallel` concurrent processes; then, unless `--no-level-check`, run the agent once with "Follow prompts/level-check.md exactly. LEVEL: <n+1>"; finally run `scope.py check` and exit with its code. Never call git. Message per node: "Follow prompts/expand-node.md exactly. TARGET: <path>". Add `unittest` tests for AC-1..AC-5 of spec node ECO.COMM.GROW with fake agents, and run them in CI on three OSes. Keep `grow.ps1` until `grow.py` has passed one real level.

## Open questions

- When to delete `grow.ps1`: after one level grown with `grow.py` on each OS.

## Children

_Leaf: none._
