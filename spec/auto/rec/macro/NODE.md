---
id: AUTO.REC.MACRO
title: Live macro recording
level: 3
parent: AUTO.REC
kind: feature
status: leaf
phase: P1b
size: S
owners: []
claimed_by:
satisfies: [REQ-AUTO.REC-001]
covers: [GOM.AUT.02, PW.AUT.01]
depends_on: [AUTO.REC.CONV]
side_effect: W1
determinism: D0
cli: [fid program record-start, fid program record-pause, fid program record-resume, fid program record-stop]
mcp: [program_record_start, program_record_pause, program_record_resume, program_record_stop]
journal: [program.record_start@1, program.record_pause@1, program.record_resume@1, program.record_stop@1]
updated: 2026-11-16
---

# AUTO.REC.MACRO: Live macro recording

## Purpose

The "record" button. Starting a recording journals a start mark on the current branch; pausing and resuming journal marks that exclude the paused span; stopping converts the recorded span into a draft program through `AUTO.REC.CONV`. The recording itself stores nothing but marks, so nothing is lost if the app crashes mid-recording: the marks are in the journal and the draft can still be made from them.

## Scope

**In:**
- Commands `program.record_start@1`, `program.record_pause@1`, `program.record_resume@1`, `program.record_stop@1` (all W1)
- Recording state per branch (at most one active recording per branch)
- Excluding paused spans; converting the span at stop

**Out:**
- Conversion rules (`AUTO.REC.CONV`); lifting (`AUTO.REC.LIFT`)
- Recording UI (`VIZ`)

## Requirements

- **REQ-AUTO.REC.MACRO-001** When `record_start` runs, the system shall journal a start mark on the current branch, and if a recording is already active on that branch, then it shall fail with `already_recording`.
- **REQ-AUTO.REC.MACRO-002** When `record_stop` runs, the system shall create a draft program containing one step per W1 or W2 command journaled on that branch between the start mark and the stop, in journal order, excluding paused spans.
- **REQ-AUTO.REC.MACRO-003** If the app restarts during a recording, then after reconstruction the recording shall still be active and `record_stop` shall produce the same draft as without the restart.
- **REQ-AUTO.REC.MACRO-004** If `record_pause`, `record_resume` or `record_stop` runs with no active recording on the branch, then it shall fail with `not_recording`.

## Acceptance criteria

- **AC-1** Given fixture project `fixtures/projects/align-and-check/`, when `fid program record-start`, three W2 commands, `record-pause`, one W2 command, `record-resume`, one W2 command and `record-stop --json` run, then the draft has 4 steps (the paused command excluded) in journal order.
- **AC-2** (error path) Given an active recording, then a second `record-start` fails with `already_recording`.
- **AC-3** Given AC-1 interrupted after the second W2 command by killing the process, when the project is reopened and the remaining sequence run, then the draft digest equals AC-1's.
- **AC-4** (replay) Given the journal from AC-1, when replayed in exact mode, then the outcome is `IDENTICAL`, including the draft created at stop.

## Design

- **Registry command(s):** `program.record_start@1` (W1; params `title: Option<String>`), `program.record_pause@1` (W1), `program.record_resume@1` (W1), `program.record_stop@1` (W1; result `{ program_id, draft_digest, step_count }`).
- **Data structures:** recording state is derived from marks on the branch (no separate store).
- **Algorithm:** at stop, compute the span from the marks, subtract paused spans, call the CONV converter with the remaining entry set.
- **Errors:** `already_recording`, `not_recording`.
- **Determinism:** D0.
- **Performance budget:** marks add < 1 ms per command [unverified until measured].
- **Crate:** `fid-program-rec`

## Test fixtures

`fixtures/projects/align-and-check/` (shared with `AUTO.REC.CONV`); crash test uses the CORE test harness's kill-and-reopen helper.

## Build prompt

> In crate `fid-program-rec`, add registry commands `program.record_start@1`, `program.record_pause@1`, `program.record_resume@1`, `program.record_stop@1` (all W1; CLI `fid program record-*`, MCP `program_record_*`). Recording state is derived only from journaled marks on the branch, so it survives restarts. At stop, compute the recorded span minus paused spans and call the `program.from_journal` converter on those entries (same filtering rules: W1 and W2 only, undone entries excluded) to create the draft. Errors: `already_recording`, `not_recording`. Acceptance: the four cases in the spec on fixture project `align-and-check`, including kill-and-reopen. Constraints: no panic, no `unsafe`, deny `unwrap`/`expect`/`panic`/`todo`/`unimplemented`/`unreachable`; all state changes through the bus.

## Open questions

- None.

## Children

_Leaf: none._
