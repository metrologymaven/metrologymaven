---
id: AUTO.CLI.JOB
title: Terminal progress and interrupt-to-cancel
level: 3
parent: AUTO.CLI
kind: task
status: leaf
phase: P0
size: S
owners: []
claimed_by:
satisfies: [REQ-AUTO.CLI-006]
covers: []
depends_on: [AUTO.SURF.JOBS, AUTO.CLI.OUT]
side_effect: R0
determinism: N
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# AUTO.CLI.JOB: Terminal progress and interrupt-to-cancel

## Purpose

Shows job progress on stderr while a long command runs and turns a user interrupt into a cancellation request, then reports how the job really ended. A second interrupt detaches without waiting, and the exit code says the outcome is unknown.

## Scope

**In:**
- Progress bar or line on stderr when stderr is a terminal; periodic plain lines otherwise
- First interrupt (Ctrl-C, SIGINT, console control event): request cancellation and wait
- Second interrupt: stop waiting and exit with the `no_reply` code
- `--wait-timeout <seconds>` for scripted use

**Out:**
- Invocation tracking and timeout classification (`AUTO.SURF.JOBS`)
- Job cancellation semantics (`CORE.JOB`)

## Requirements

- **REQ-AUTO.CLI.JOB-001** While a job runs and stderr is a terminal, the CLI shall show its fraction and stage label on stderr, updated at least once per second.
- **REQ-AUTO.CLI.JOB-002** When the user interrupts once, the CLI shall request cancellation through the tracker and wait for the final job state, then exit 5 if cancelled or with the code of the actual outcome otherwise.
- **REQ-AUTO.CLI.JOB-003** When the user interrupts a second time, the CLI shall stop waiting and exit 7 (`no_reply`) printing the job ID.
- **REQ-AUTO.CLI.JOB-004** The CLI shall never exit 0 after an interrupt unless the final job state is committed or computed.

## Acceptance criteria

- **AC-1** Given the fake bus job `J1` that publishes progress every 200 ms and honours cancel after 300 ms, when the test harness sends one interrupt, then `job.cancel_job@1` is sent once and the CLI exits 5.
- **AC-2** Given a job that commits before the cancel is processed, when interrupted once, then the CLI exits 0 and prints the journal entry hash.
- **AC-3** Given a job that ignores cancel, when interrupted twice, then the CLI exits 7 and stderr names `J1`.
- **AC-4** Given stderr not a terminal, then progress is written as plain lines no more than once per second.

## Design

- **Registry command(s):** uses `job.cancel_job@1` (W1) via `AUTO.SURF.JOBS`.
- **Data structures:** `InterruptState { None, CancelRequested, Detached }`.
- **Algorithm:** install a handler (Unix SIGINT, Windows console control handler) that writes to a channel; main loop selects on job events and interrupts.
- **Errors:** handler installation failure → continue without interrupt support and warn on stderr.
- **Determinism:** N (timing-dependent).
- **Performance budget:** progress rendering < 1% CPU.
- **Crate:** `fid-cli`

## Test fixtures

Fake bus job scripts; interrupt injected through a test hook instead of real signals, plus one integration test that sends a real SIGINT on Unix.

## Build prompt

> In crate `fid-cli`, add progress display and interrupt handling around `fid_surface` invocation tracking. Show fraction and stage on stderr (terminal: redraw at ≥ 1 Hz; non-terminal: plain lines at ≤ 1 Hz). First interrupt: request cancellation (`job.cancel_job@1` through the tracker), wait for the final state, exit 5 if cancelled or with the actual outcome's exit code. Second interrupt: exit 7 (`no_reply`) and print the job ID. Never exit 0 after an interrupt unless the job committed or computed. Acceptance: the four cases using the fake bus and an injected interrupt hook. Constraints: no panic, no `unsafe` beyond what the signal crate encapsulates, deny `unwrap`/`expect`/`panic`/`todo`/`unimplemented`/`unreachable`.

## Open questions

- None.

## Children

_Leaf: none._
