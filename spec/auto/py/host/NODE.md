---
id: AUTO.PY.HOST
title: Out-of-process script host
level: 3
parent: AUTO.PY
kind: module
status: open
phase: P1b
size: L
owners: []
claimed_by:
satisfies: [REQ-AUTO.PY-003, REQ-AUTO.PY-004, REQ-AUTO.PY-007]
covers: []
depends_on: [AUTO.PY.PROTO, AUTO.SURF.JOBS, CORE.BUS]
cli: [fid script host-*]
mcp: []
journal: []
updated: 2026-11-16
---

# AUTO.PY.HOST: Out-of-process script host

## Purpose

Starts each script in its own OS process with a chosen interpreter, connects it to the session only through the host protocol, enforces time and memory limits, stops it on cancel, and reports which of its commands committed. A crash, hang or memory blow-up in a script must never take the app down or lose committed work.

## Scope

**In:**
- Process launch with an isolated environment (interpreter path, working directory in a write root, scrubbed environment variables)
- Time, memory and output limits; watchdog
- Cancellation: polite stop, then forced termination
- Per-run report of committed command entries
- OS-level sandboxing where available (per-OS profile)
- `fid script host-*` diagnostics (interpreter discovery, limits)

**Out:**
- Message protocol (`AUTO.PY.PROTO`)
- Starting runs and principal choice (`AUTO.PY.RUN`)
- Interpreter installation (`PLAT`)

## Requirements

- **REQ-AUTO.PY.HOST-001** The host shall run every script in a separate OS process connected to the app only through the host protocol pipes.
- **REQ-AUTO.PY.HOST-002** If a script process crashes, exceeds its memory limit or stops responding, then the host shall terminate it, mark the run failed and keep the app and every committed entry intact.
- **REQ-AUTO.PY.HOST-003** When a run is cancelled, the host shall request a polite stop, force termination if the process is still alive after 5 s [unverified], and report the journal entry hashes of every command the run committed.
- **REQ-AUTO.PY.HOST-004** The script process shall have no handle to project storage, and its only means of changing state shall be commands sent over the protocol.

## Interfaces

- **Offers:** `ScriptHost::start(spec) -> RunHandle`, cancel, status; `fid script host-show-config`.
- **Consumes:** `AUTO.PY.PROTO`; `AUTO.SURF.JOBS`; CORE bus; `PLAT` interpreter discovery and sandbox capabilities.

## Decomposition hints

LAUNCH (process creation and environment), LIMITS (watchdog, memory, output), CANCEL (stop and report), SANDBOX (per-OS confinement).

## Open questions

- Sandboxing depth differs by OS (for example seccomp or Landlock on Linux, App Sandbox on macOS, job objects and AppContainer on Windows [unverified]); needs a `PLAT` input on what each supported OS can offer before SANDBOX can be specified.

## Children

_None yet: status `open`._
