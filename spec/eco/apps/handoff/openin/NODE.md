---
id: ECO.APPS.HANDOFF.OPENIN
title: Open-in hand-off with write-lock release
level: 4
parent: ECO.APPS.HANDOFF
kind: task
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-ECO.APPS.HANDOFF-004]
covers: []
depends_on: [ECO.APPS.HANDOFF.EDITORS, CORE.STO.LAYOUT, VIZ.SHELL.DOCK]
uses_artifacts: [ECO.REL.PKG.REG]
side_effect: R0
determinism: N
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# ECO.APPS.HANDOFF.OPENIN: "Open in" hand-off with write-lock release

## Purpose

The "Open in <app>" action in every app's File menu and in the read-only refusal panel. It closes the project cleanly in the handing app (all commits durable, write lock released), then starts the chosen installed app on the same project location, so at most one app ever writes to a project. Under CONSTITUTION §3.1 (D-031 item 4) launching another app is a UI or OS action, not a registry command: nothing is journaled and agents cannot trigger it.

## Scope

**In:**
- The UI action and its two entry points (File menu, refusal panel from ECO.APPS.HANDOFF.RO)
- Ordered hand-off: wait for running jobs or ask to cancel, close project, release lock, launch target, report outcome
- Resolving an `install_ref` from ECO.APPS.HANDOFF.EDITORS to the target app's launcher
- Behaviour when launch fails (reopen the project in the handing app)

**Out:**
- The lock itself and stale-lock rules (owned by `CORE.STO.LAYOUT`)
- Choosing which apps to offer (owned by `ECO.APPS.HANDOFF.EDITORS`)
- Installer registration and OS launch metadata (owned by `ECO.REL.PKG`)
- Menu and dialog rendering (owned by `VIZ.SHELL.DOCK`)

## Requirements

- **REQ-ECO.APPS.HANDOFF.OPENIN-001** When the user chooses "Open in <app>", the handing app shall release the project's write lock before it starts the target app process.
- **REQ-ECO.APPS.HANDOFF.OPENIN-002** If a job that writes to the project is running when the user chooses "Open in", then the app shall ask the user to wait or cancel it and shall not release the lock while it runs.
- **REQ-ECO.APPS.HANDOFF.OPENIN-003** If the target app fails to start within 30 s or exits before acquiring the lock, then the handing app shall reacquire the lock, reopen the project and show the failure reason.
- **REQ-ECO.APPS.HANDOFF.OPENIN-004** The hand-off shall not be reachable from the registry, the CLI, MCP or the control channel, and shall write nothing to the Record Journal.
- **REQ-ECO.APPS.HANDOFF.OPENIN-005** The target app shall be started with the project location as its only document argument, and the hand-off shall pass no other state.

## Acceptance criteria

- **AC-1** Given two test app binaries built from fixture manifests (`fixtures/handoff/apps/a`, `fixtures/handoff/apps/b`) and project `fixtures/handoff/mixed.fidproj` open for writing in A, when the hand-off to B is triggered through the UI test driver, then a lock probe shows the lock free before B's process start timestamp and held by B afterwards, and A no longer holds the project.
- **AC-2** (running job) Given a long fixture job writing to the project in A, when the hand-off is triggered and the user chooses "wait", then the lock is not released until the job ends.
- **AC-3** (error path) Given a target registration whose launcher exits with status 1 immediately, when the hand-off runs, then A holds the lock again within 2 s and shows reason `APP-LAUNCH-FAILED`.
- **AC-4** Given the registry manifest of the test app, then it contains no command for the hand-off; `fid mcp` tool listing and the control channel command list contain no hand-off entry; the Record Journal head is unchanged after AC-1.
- **AC-5** The target process's argument vector in AC-1 is exactly the launcher path plus the project location.

## Design

- **Registry command(s):** none (CONSTITUTION §3.1, launching another app). `side_effect: R0` records that the leaf issues no state-changing command of its own; closing the project uses the existing `project.close_project@1` (R0).
- **Data structures:** `HandoffPlan { target: InstallRef, project: ProjectLocation }`; outcome `Launched | Cancelled | Failed(reason)`.
- **Algorithm:** check running jobs (CORE.JOB status) → prompt if any → close project (flush, release lock through `ProjectLocation`) → resolve launcher from the registration entry → spawn detached process with the project location → poll the lock file for up to 30 s for the target's holder ID → on timeout or early exit, reacquire and reopen.
- **Errors:** `APP-LAUNCH-FAILED`, `APP-LAUNCH-TIMEOUT`, `APP-INSTALL-MISSING`.
- **Determinism:** N (process start and timing are observed, not computed); the outcome has no value of record.
- **Performance budget:** lock released within 1 s of confirmation on a project with no running jobs [unverified until measured].
- **Crate:** `fid-app-family` (module `openin`), wired into each app's File menu through the shell.

## Test fixtures

Two minimal test app binaries built in the test suite from fixture manifests, fixture registration entries and a fake launcher that exits early (MIT OR Apache-2.0).

## Build prompt

> In crate `fid-app-family` (implementation repository), add module `openin`: the "Open in <app>" UI action. It is NOT a registry command (CONSTITUTION §3.1: launching another app is a UI or OS action) and must not be reachable from CLI, MCP or the control channel, and must journal nothing. Steps: if a project-writing job runs, ask wait or cancel; close the project (existing `project.close_project@1`), which releases the CORE.STO.LAYOUT write lock; resolve the target's launcher from its installed-app registration entry (opaque `install_ref` from `app.list_editors@1`); spawn it with the project location as the only document argument; poll the lock for up to 30 s; if the target exits or never takes the lock, reacquire it, reopen the project and show `APP-LAUNCH-FAILED` or `APP-LAUNCH-TIMEOUT`. Expose entry points for the File menu and the refusal panel of module `readonly`. Tests for AC-1..AC-5 of spec node ECO.APPS.HANDOFF.OPENIN with two test app binaries and a fake failing launcher. Constraints: CONSTITUTION §8 (no `unwrap`/`expect`/`panic`/`unsafe`), no network, never pass anything but the project location.

## Open questions

- The WASM build cannot start processes; there the action becomes "copy project link" or is hidden. Decide with PLAT.WEB when it is expanded.

## Children

_Leaf: none._
