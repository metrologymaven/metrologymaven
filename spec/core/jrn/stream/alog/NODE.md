---
id: CORE.JRN.STREAM.ALOG
title: Stream routing and the P0 Activity Log
level: 4
parent: CORE.JRN.STREAM
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
built_in: [fid-journal, fid-jobs]
built_pr: 307
satisfies: [REQ-CORE.JRN.STREAM-001, REQ-CORE.JRN.STREAM-002, REQ-CORE.JRN.STREAM-003, REQ-CORE.JRN.STREAM-007]
covers: []
depends_on: [CORE.JRN.ENTRY, CORE.JRN.CHAIN.APPEND, CORE.JOB.SCHED, CORE.STO.LAYOUT, CORE.STO.SET]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# CORE.JRN.STREAM.ALOG: Stream routing and the P0 Activity Log

## Purpose

Decides which stream a record belongs to and writes the local Activity Log. Every committed command whose class is not R0 or R1 goes to the Record Journal. R0 and R1 calls by local principals go to the Activity Log, and only while a recording session is active or when the call is a step of a Command Program (D-031); otherwise they are not stored at all. Job lifecycle changes always go to the Activity Log. In P0 the Activity Log uses the entry schema without hash chaining and rotates by size. The Record Journal and Access Ledger are never compacted, rewritten or reordered.

## Scope

**In:** `Stream::route(class, principal, context) -> Option<StreamId>`; the recording flag API used by the recording session owner; the Activity Log writer (unchained, rotating, local); the job `LifecycleSink` implementation; retention rules per stream.
**Out:** the Record Journal append (`CORE.JRN.CHAIN.APPEND`); Access Ledger format (`CORE.JRN.STREAM.LEDGER`); P1b chaining and compaction (`CORE.JRN.STREAM.COMPACT`); recording sessions and their policy (`VIZ.CAPTURE.REC`); Command Program execution (`AUTO.PROG`, `AUTO.RUN`).

## Requirements

- **REQ-CORE.JRN.STREAM.ALOG-001** When a command of class W1, W2, W3, X1, I1, D1, M1, S1 or A0 commits, the router shall assign it to the Record Journal.
- **REQ-CORE.JRN.STREAM.ALOG-002** When an R0 or R1 call by a local principal completes, the router shall assign it to the Activity Log if a recording session is active or the call carries a Command Program step reference, and otherwise to no stream; it shall never assign it to the Record Journal.
- **REQ-CORE.JRN.STREAM.ALOG-003** When an R0 or R1 call by an external principal (a self-hosted server channel) completes, the router shall assign it to the Access Ledger when one exists, and otherwise to no stream.
- **REQ-CORE.JRN.STREAM.ALOG-004** When a job changes lifecycle state, the journal shall write the change to the Activity Log, whether or not a recording session is active.
- **REQ-CORE.JRN.STREAM.ALOG-005** In P0 the Activity Log shall write records in the Record Journal entry schema with stream `activity`, without previous-hash links, as one JSON line per record.
- **REQ-CORE.JRN.STREAM.ALOG-006** The Activity Log shall rotate when the current file reaches a configurable size, defaulting to 64 MiB, and keep a configurable number of rotated files, defaulting to 8, deleting the oldest beyond that.
- **REQ-CORE.JRN.STREAM.ALOG-007** The journal shall offer no operation that compacts, rewrites, reorders or deletes Record Journal or Access Ledger entries.
- **REQ-CORE.JRN.STREAM.ALOG-008** If the Activity Log cannot be written, then the journal shall report a warning to the caller and shall not fail the R0 or R1 call.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/core/jrn/stream/alog/routing.csv` (every class x principal locality x recording flag x program step, with expected stream), when `route` runs for each row, then every result matches.
- **AC-2** Given no recording session, when 100 R0 test calls run, then the Activity Log and Record Journal are unchanged; after `set_recording(true)`, the next R0 call adds one Activity Log line that validates against `entry.schema.json` with stream `activity`.
- **AC-3** Given a Command Program step reference on an R1 call without recording, then one Activity Log line is written.
- **AC-4** Given a test job that runs and succeeds, then the Activity Log holds its lifecycle changes even with recording off.
- **AC-5** Given a 1 MiB rotation size set in settings and 5,000 records, then the log rotates, at most 8 rotated files remain, and no line is split across files.
- **AC-6** (error path) Given a read-only Activity Log directory, when an R0 call runs during recording, then the call succeeds with a warning.

## Design

- **Registry command(s):** none; library API.
- **Data structures:** `enum StreamId { Record, Activity, AccessLedger }`; `RouteContext { recording: bool, program_step: Option<ProgramStepRef>, external: bool }`; `ActivityLog { dir, current, size, max_size, keep }`; `RecordingFlag` (atomic bool plus recording ID) with `set_recording(active, id)`.
- **Algorithm:** total `match` on class; for R0/R1, check `external`, then `recording || program_step.is_some()`. Writer appends a canonical JSON line and flushes at rotation and close (not durable per line: the Activity Log is not a record of state); rotation renames `activity.log` to `activity.<n>.log`.
- **Errors:** `activity_log_unavailable` (warning only).
- **Determinism:** D0 for routing.
- **Performance budget:** one Activity Log line < 20 µs excluding disk flush [unverified until measured].
- **Crate:** `fid-journal` (module `stream`).

## Test fixtures

`fixtures/core/jrn/stream/alog/routing.csv`, written from CONSTITUTION §3 (D-031); CC-BY-4.0.

## Build prompt

> In crate `fid-journal`, add module `stream`. Implement `route(class, principal, ctx) -> Option<StreamId>`: every non-R class (W1, W2, W3, X1, I1, D1, M1, S1, A0) → Record; R0/R1 from an external (server) principal → AccessLedger if one exists, else none; R0/R1 from a local principal → Activity if `ctx.recording` or `ctx.program_step` is set, else none; never Record. Provide `set_recording(active, recording_id)` for the recording-session owner. Implement the P0 Activity Log: entry-schema JSON lines with stream `activity`, no hash chaining, rotation at a settings value (default 64 MiB) keeping a settings number of files (default 8). Implement the job `LifecycleSink` from `fid-jobs` to write every lifecycle change regardless of recording. Write failures produce a warning, never a failed read. Offer no compaction or rewrite API for Record Journal or Access Ledger. Tests: AC-1..AC-6 with `fixtures/core/jrn/stream/alog/routing.csv`. Constraints: CONSTITUTION §3 Reads (D-031), C4, C7 (local only), §8.

## Open questions

- REQ-CORE.JRN-001 and REQ-CORE.JRN.CHAIN-001 list W1, W2, W3, D1, M1 and S1; REQ-001 here also routes X1, I1 and A0 commands to the Record Journal, following C4 and D-031 (only reads are exempt). A maintainer should align the parent wording.

## Children

_Leaf: none._
