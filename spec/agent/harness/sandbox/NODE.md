---
id: AGENT.HARNESS.SANDBOX
title: Per-run sandbox project and task-scoped roots
level: 3
parent: AGENT.HARNESS
kind: task
status: leaf
phase: P0
size: S
owners: []
claimed_by:
built_in: [fid-eval-harness]
satisfies: [REQ-AGENT.HARNESS-002, REQ-AGENT.HARNESS-010]
covers: []
depends_on: [AGENT.EVAL.FORMAT]
uses_artifacts: [CORE.STO, AUTO.SURF.ROOTS]
side_effect: W1
determinism: D1
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# AGENT.HARNESS.SANDBOX: Per-run sandbox project and task-scoped roots

## Purpose

Builds a fresh, isolated project for every run from the task's start state, checks it is exactly the state the task names, and makes the task's granted folders the only import and export roots. Nothing from the suite except the start state and the granted inputs is placed in the sandbox.

## Scope

**In:**
- Run directory layout: `run/<run-id>/{project, roots/read/*, roots/write/*, home, artifacts}`
- Start-state unpack and digest check
- Root declarations limited to the task's grants
- A file-system snapshot (path and SHA-256 of every file) before and after the run

**Out** (and which node owns it instead):
- Root enforcement at run time (`AUTO.SURF.ROOTS`, `CORE.BUS.POL`)
- Launching the agent (`AGENT.HARNESS.RUN`)

## Requirements

- **REQ-AGENT.HARNESS.SANDBOX-001** For each run, the sandbox shall create a new run directory under the harness work folder with a fresh UUIDv7 run ID, and shall refuse to reuse an existing directory.
- **REQ-AGENT.HARNESS.SANDBOX-002** If the unpacked start state's digest differs from the task's start-state digest, then the sandbox shall fail with `start_state_mismatch` and the run shall not start.
- **REQ-AGENT.HARNESS.SANDBOX-003** The sandbox project's root configuration shall contain exactly the task's read and write roots, each mapped to a folder inside the run directory, and no other root.
- **REQ-AGENT.HARNESS.SANDBOX-004** The sandbox shall set a private home and configuration folder for the run, so no user or machine settings, credentials or other projects are visible to `fid` or the agent.
- **REQ-AGENT.HARNESS.SANDBOX-005** The sandbox shall record a snapshot of path and SHA-256 for every file in the run directory before launch and after stop.
- **REQ-AGENT.HARNESS.SANDBOX-006** The sandbox shall make no network connection and shall not invoke any promotion, adoption or release command.

## Acceptance criteria

- **AC-1** Given task `fixtures/agent/eval/suite-mini/t1`, when the sandbox is built, then the project digest equals the task's start-state digest and the root configuration lists only `in` (read) and `out` (write) inside the run directory.
- **AC-2** (error path) Given a start state with one altered byte, then building fails with `start_state_mismatch`.
- **AC-3** Given a user config file in the real home folder, then `fid` started inside the sandbox environment does not read it (checked by a fixture config that would add a root).
- **AC-4** Given the before and after snapshots of a run in which the fixture agent writes `out/report.csv`, then the snapshot diff lists exactly that file.

## Design

- **Registry command(s):** none. Library `Sandbox::build(task) -> Sandbox`, `Sandbox::snapshot() -> FsSnapshot`.
- **Data structures:** `Sandbox { run_id, dir, project, env: Vec<(String, String)> }`; `FsSnapshot { files: Vec<(RelPath, Digest)> }` sorted.
- **Algorithm:** create directory exclusively; unpack the archive rejecting absolute and `..` entries; verify digest; write roots as declarations relative to the project (requires CORE storage support for project-relative roots, see Open questions); set `HOME`/`XDG_CONFIG_HOME`/`APPDATA`-equivalents for child processes.
- **Errors:** `start_state_mismatch`, `run_dir_exists`, `bad_archive`.
- **Determinism:** D1 (file-system behaviour is per platform).
- **Performance budget:** < 2 s for a 200 MiB start state on SSD [unverified].
- **Crate:** `fid-eval-harness`

## Test fixtures

`fixtures/agent/eval/suite-mini/` start states (project-authored), plus an altered copy.

## Build prompt

> In crate `fid-eval-harness` (Tooling; reads files only, links no product crates), implement `Sandbox::build(task)`: exclusive run directory `run/<uuidv7>/` with `project/`, `roots/read/*`, `roots/write/*`, a private `home/`; unpack the start-state archive fail-closed; verify its digest (`start_state_mismatch`); declare exactly the task's roots (relative to the project) and nothing else; build the child-process environment so `fid` and the agent see only the private home and configuration. Implement `snapshot()` (sorted path and SHA-256 of every file). Make no network connection and never call promotion, adoption or release commands. Acceptance: AC-1 to AC-4 of AGENT.HARNESS.SANDBOX. Constraints: CONSTITUTION §8 (no panic, deny `unwrap` etc., bounded archive handling).

## Open questions

- Roots are configuration, and only a human may change them (REQ-AUTO.SURF.ROOTS-006). The sandbox therefore needs the start state to carry its roots as project-relative declarations that CORE storage accepts at open time. No CORE node states this yet; it needs a CORE.STO requirement (AGENT cannot add it).

## Children

_Leaf: none._
