---
id: PLAT.DOCTOR.FEEDBK.CRASH
title: Local crash capture, queue and list command
level: 4
parent: PLAT.DOCTOR.FEEDBK
kind: feature
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-PLAT.DOCTOR.FEEDBK-001]
covers: []
depends_on: [CORE.BUS, PLAT.DOCTOR.PROBE]
side_effect: R0
determinism: N
cli: [fid doctor list-reports]
mcp: [doctor_list_reports]
journal: [doctor.list_reports@1]
updated: 2026-11-17
---

# PLAT.DOCTOR.FEEDBK.CRASH: Crash capture and queue

## Purpose

When an app or `fid` process dies abnormally, records what maintainers need to diagnose it in a small local crash record, without any network and without user data: versions, platform, the failing thread's symbolised stack of product frames, the last command IDs, and anonymous session counters. Records wait in a local queue until the user chooses to include one in a report or deletes it.

## Scope

**In:**
- A crash handler installed at process start (fatal signals and unhandled exceptions on Windows, Linux and macOS; out-of-process minidump capture is not included)
- The crash record schema `crash-record.json` v1
- The queue directory under the per-user cache (`fiducial/crash-queue/`), capped at 20 records and 1 MiB each, oldest dropped first
- Session counters: sessions started and sessions ended by a crash since the last sent report
- `doctor.list_reports@1` (R0) listing queued crash records and prepared bundles

**Out** (and which node owns it instead):
- Building and redacting the outgoing bundle (`PLAT.DOCTOR.FEEDBK.COMPOSE`)
- Sending (`PLAT.DOCTOR.FEEDBK.SEND`)
- Journal crash recovery (`CORE.RPL.RECON`)

## Requirements

- **REQ-PLAT.DOCTOR.FEEDBK.CRASH-001** When a process of the product terminates on a fatal signal or unhandled exception, the handler shall write one crash record containing product version, build commit, OS family and version, architecture, GPU backend, the crashing thread's stack as module-relative offsets with product symbol names only, the IDs (not parameters) of the last 20 commands, and a UTC timestamp truncated to the hour.
- **REQ-PLAT.DOCTOR.FEEDBK.CRASH-002** The crash record shall contain no user name, host name, absolute path, file name from a project, command parameter value or memory dump.
- **REQ-PLAT.DOCTOR.FEEDBK.CRASH-003** The handler shall make no network request, shall write only inside the queue directory, and shall write the record with a temp file and rename so a partial record is never listed.
- **REQ-PLAT.DOCTOR.FEEDBK.CRASH-004** The queue shall hold at most 20 records, each at most 1 MiB, dropping the oldest when full.
- **REQ-PLAT.DOCTOR.FEEDBK.CRASH-005** The product shall keep counters of sessions started and sessions ended by a crash since the last successfully sent report, and include them in each new crash record.
- **REQ-PLAT.DOCTOR.FEEDBK.CRASH-006** `doctor.list_reports@1` shall be class R0 and return, sorted by record ID, each queued crash record's ID, time, version and first product frame, and each prepared bundle's digest, with no file paths.

## Acceptance criteria

- **AC-1** Given test binary `crash-fixture` that aborts inside a named product function on each Tier 1 runner, when it crashes, then exactly one record appears in the queue, validates against `platform/crash-record.schema.json`, and names that function as the first product frame.
- **AC-2** Given the record from AC-1 and the runner's user and host names read by the test, when the record bytes are searched, then neither name nor any absolute path appears.
- **AC-3** Given 25 crashes, when the queue is listed, then 20 records remain and the 5 oldest are gone.
- **AC-4** Given a network-deny sandbox, when `crash-fixture` crashes, then the record is written and no connection attempt is logged.
- **AC-5** Given three records, when `fid doctor list-reports --json` runs, then it lists three crash entries sorted by ID and the output contains no path.

## Design

- **Registry command(s):** `doctor.list_reports@1`, R0, MCP exposed; no params; result `{ crashes: [{ id, time_hour, version, first_frame }], bundles: [{ digest, kind, created_hour }] }`.
- **Data structures:** `CrashRecord { schema_version: 1, id: UUIDv7, version, commit, os, arch, gpu_backend, frames: Vec<{ module, offset, symbol? }>, last_commands: Vec<CommandId>, counters: { sessions, crashed_sessions }, time_hour }`.
- **Algorithm:** pre-allocate the record buffer at start; on a fatal signal, walk the stack with an async-signal-safe unwinder, keep frames only from product modules, write the pre-formatted JSON through `write` and `rename`; symbolise lazily when listing.
- **Errors:** handler failures are silent (no recursion); list errors map to `Internal`.
- **Determinism:** N.
- **Crate:** `fid-crash` (handler and queue) with the command registered in `fid-doctor`.

## Test fixtures

Test binary `crash-fixture` built from test sources with a fixed crash site. Licensed with the code.

## Build prompt

> Create crate `fid-crash`: install a fatal-signal and unhandled-exception handler on Windows, Linux and macOS that writes a pre-allocated `crash-record.json` v1 (UUIDv7 id, version, commit, OS, arch, GPU backend, product-only stack frames as module + offset, last 20 command IDs from a lock-free ring, session counters since last sent report, UTC time truncated to the hour) into `<user cache>/fiducial/crash-queue/` via temp file and rename; no paths, names, parameters or memory; no network; queue capped at 20 records × 1 MiB, oldest dropped. Export the JSON Schema to `platform/crash-record.schema.json`. In `fid-doctor`, register `doctor.list_reports@1` (R0, MCP exposed; CLI `fid doctor list-reports` generated) returning crash entries and prepared bundle digests sorted by ID, no paths. Tests: AC-1..AC-5. Constraints: CONSTITUTION §8 (the handler is the named `unsafe` exception for signal handling if needed; nothing else), C7. Commit: `feat(fid-crash): local crash capture`.

## Open questions

- Signal-safe unwinding on each OS may need a named `unsafe` exception under CONSTITUTION §8 [unverified].

## Children

_Leaf: none._
