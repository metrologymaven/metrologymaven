---
id: AUTO.CLI.TARGET.HEADLESS
title: Headless project session with locking and human upgrade
level: 4
parent: AUTO.CLI.TARGET
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
satisfies: [REQ-AUTO.CLI.TARGET-001, REQ-AUTO.CLI.TARGET-003, REQ-AUTO.CLI.TARGET-004]
covers: []
depends_on: [AUTO.CLI.TARGET.SELECT, AUTO.SURF.JOBS, AUTO.SURF.ROOTS, CORE.STO.LAYOUT, CORE.BUS.PRIN]
side_effect: R0
determinism: N
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# AUTO.CLI.TARGET.HEADLESS: Headless project session with locking and human upgrade

## Purpose

Runs a `fid` command in an in-process session on a project file without any window: resolve the project reference, open it with the storage lock, run the command through the shared invocation tracker, and release the project on every exit path including interrupt. It also carries the CLI side of the D-012 human upgrade for headless sessions (an OS user-presence check), because the session is where the principal lives.

## Scope

**In:**
- Resolving `--project` / `FID_PROJECT` values: a root handle for any principal, a raw path only for a session already upgraded to `human`
- Opening the project for reading (R0/R1 commands) or writing (W commands) through `CORE.STO.LAYOUT`
- Turning a `locked` error held by a running app into `conflict` naming the app
- Releasing the lock on normal exit, error and interrupt
- `--confirm-human`: asking `CORE.BUS.PRIN` for the OS user-presence upgrade of this session's principal

**Out:**
- Target choice (`AUTO.CLI.TARGET.SELECT`)
- The running-app target (`AUTO.CLI.TARGET.APP`)
- Lock file format and stale-lock rules (`CORE.STO.LAYOUT`)
- The upgrade mechanism and the user-presence check itself (`CORE.BUS.PRIN`, `TRUST.SIGN.UV`)
- Interrupt handling and progress rendering (`AUTO.CLI.JOB`)

## Requirements

- **REQ-AUTO.CLI.TARGET.HEADLESS-001** When the headless target is selected, the CLI shall open the project in an in-process session, run the command through the `AUTO.SURF.JOBS` tracker and close the session before exiting.
- **REQ-AUTO.CLI.TARGET.HEADLESS-002** When the process exits after a normal finish, a failure or an interrupt, the CLI shall release the project's write lock before the process ends.
- **REQ-AUTO.CLI.TARGET.HEADLESS-003** If the command is of class W1, W2 or W3 and the storage layer reports the project `locked` by a running app, then the CLI shall exit 3 with `conflict` naming the holder's app and process ID and shall not open a second writer.
- **REQ-AUTO.CLI.TARGET.HEADLESS-004** When the command is of class R0 or R1, the CLI shall open the project read-only and shall not take the write lock.
- **REQ-AUTO.CLI.TARGET.HEADLESS-005** If the project value is a raw path and the session principal is not `human`, then the CLI shall exit 3 with `raw_path_refused` and open nothing.
- **REQ-AUTO.CLI.TARGET.HEADLESS-006** When `--confirm-human` is given, the CLI shall request the OS user-presence upgrade from `CORE.BUS.PRIN` before opening the project, and if the check fails or is unavailable, then it shall exit 3 with `human_confirmation_failed` and run nothing.
- **REQ-AUTO.CLI.TARGET.HEADLESS-007** The headless target shall produce envelopes through the same `AUTO.SURF.ENV` encoder as the app target, with no target-specific fields.

## Acceptance criteria

- **AC-1** Given fixture project `fixtures/projects/min.fid` under read root `projects`, when `fid demo set-offset --project root:projects/min.fid --offset 1mm --json` runs, then the envelope outcome is `committed`, the project journal grows by one entry and no lock file remains after exit.
- **AC-2** Given the same project locked by a fake app process (lock record names app `inspect`, pid 4242), when a W2 command runs headless, then exit 3 with `conflict` naming `inspect` and 4242, and the journal is unchanged.
- **AC-3** Given the locked project of AC-2, when R0 `demo.show_thing@1` runs headless, then it succeeds read-only and the lock record is unchanged.
- **AC-4** (interrupt) Given a long fixture command `demo.slow_job@1`, when SIGINT (Ctrl+C on Windows) arrives mid-run, then the job is cancelled and the lock file is gone when the process exits.
- **AC-5** (error path) Given principal `script` and `--project C:\data\min.fid`, then exit 3 with `raw_path_refused` and nothing is opened.
- **AC-6** (error path) Given `--confirm-human` and a fake user-presence provider that denies, then exit 3 with `human_confirmation_failed`; given one that approves and a raw path, the command runs and the journal entry's principal kind is `human`.

## Design

- **Registry command(s):** none.
- **Data structures:** `HeadlessSession { location: Locked | ReadOnly, principal: Principal, transport: InProcTransport }` with `Drop` releasing the lock; `InProcTransport` implements `TargetTransport`.
- **Algorithm:** (1) if `--confirm-human`, call the `CORE.BUS.PRIN` upgrade with the OS user-presence provider; (2) parse the project value with `RootHandle::parse`; raw path only if principal is `human`; (3) choose read-only or write mode from the command's class; (4) `ProjectLocation::open`; map `locked` with an app holder to `conflict`; (5) run via the tracker; (6) close and release in a scope guard that also runs on the cancel path.
- **Errors:** `conflict` (exit 3), `raw_path_refused` (exit 3), `human_confirmation_failed` (exit 3), `bad_handle` (exit 2), storage errors (exit 4).
- **Determinism:** N (file locks, OS prompts); command results keep the determinism of the command.
- **Performance budget:** open plus close of `min.fid` < 150 ms on reference hardware [unverified until measured].
- **Crate:** `fid-cli`

## Test fixtures

`fixtures/projects/min.fid` (project-authored minimal project, CC-BY-4.0); fake lock holder helper that writes a lock record naming an app; fake user-presence provider (approve or deny); `demo.slow_job@1` from the fake registry.

## Build prompt

> In crate `fid-cli`, implement the headless target. Given a `Target::Headless { project }` from `resolve_target`: if `--confirm-human`, call the CORE.BUS.PRIN session-upgrade API with the OS user-presence provider and exit 3 `human_confirmation_failed` on failure. Parse `project` as a root handle (`fid_surface::RootHandle`); a raw path is allowed only when the session principal is `human`, otherwise exit 3 `raw_path_refused`. Open read-only for R0/R1 commands and with the write lock (`ProjectLocation::open`) for W1/W2/W3; a `locked` error held by an app becomes exit 3 `conflict` naming the app and pid, and never open a second writer. Run the command through `fid_surface` invocation tracking with an in-process transport, encode the envelope with the shared encoder, and release the lock on every path, including Ctrl+C cancellation (scope guard). Acceptance: AC-1..AC-6 with `fixtures/projects/min.fid`, a fake lock holder and a fake user-presence provider. Constraints: no panic, no `unsafe`, deny `unwrap`/`expect`/`panic`/`todo`/`unimplemented`/`unreachable`; raw paths stay refused for `script` (CONSTITUTION §3, D-012).

## Open questions

- `CORE.BUS.PRIN` has not yet been updated for D-012 (it is marked "Unblocked"); this leaf assumes it will offer a per-session upgrade call taking a user-presence provider. If PRIN names it differently, only the call site changes.
- The lock record must name the holding app for REQ-003; `CORE.STO.LAYOUT` currently promises process ID and host only. If it cannot add the app name, `conflict` names pid and host.

## Children

_Leaf: none._
