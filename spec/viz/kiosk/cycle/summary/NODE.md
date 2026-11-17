---
id: VIZ.KIOSK.CYCLE.SUMMARY
title: Cycle summary, loop progress and end-of-cycle outcomes
level: 4
parent: VIZ.KIOSK.CYCLE
kind: task
status: leaf
phase: P1b
size: S
owners: []
claimed_by:
satisfies: [REQ-VIZ.KIOSK.CYCLE-003, REQ-VIZ.KIOSK.CYCLE-004]
covers: []
depends_on: [VIZ.KIOSK.STATUS, AUTO.RUN.PLAY.STATE, AUTO.RUN.BIND]
uses_artifacts: []
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# VIZ.KIOSK.CYCLE.SUMMARY: Cycle summary, loop progress and end-of-cycle outcomes

## Purpose

Shows where the operator is in a multi-part or pallet program and what happened at the end of the cycle. Loop iterations are the program's own loops (`AUTO.PROG.FLOW`); the kiosk only displays the iteration path from the run state ("part 3 of 8") and never adds, skips or repeats iterations itself. End-of-cycle actions (label print, report, export) are steps the program declares, run and journaled by the interpreter; the summary lists each one's outcome. "Next part" returns to serial entry for the next cycle.

## Scope

**In:** loop progress display from the current step key's iteration path and the loop's declared count when known; cycle summary screen (overall outcome with `VIZ.KIOSK.STATUS`, counts per decision outcome, the outcome of every report, export and print step with its output name); "Next part" action.
**Out:** loop semantics (`AUTO.PROG.FLOW`); running report and export steps (`AUTO.PROG.INTERP`, `RPT`, `IO`); failures (`VIZ.KIOSK.CYCLE.ESCAL`); serial entry (`VIZ.KIOSK.PROGPICK`).

## Requirements

- **REQ-VIZ.KIOSK.CYCLE.SUMMARY-001** Loop progress shall be derived only from the run state's iteration path and the program's declared loop bounds, and the kiosk shall offer no control that changes iteration.
- **REQ-VIZ.KIOSK.CYCLE.SUMMARY-002** When a run ends `succeeded`, the summary shall list every executed step whose command is a report, export or print command with its outcome and the entry hash that journaled it.
- **REQ-VIZ.KIOSK.CYCLE.SUMMARY-003** The summary shall show the cycle's overall outcome and counts per decision outcome through the `VIZ.KIOSK.STATUS` display.
- **REQ-VIZ.KIOSK.CYCLE.SUMMARY-004** "Next part" shall be offered only after the run reached a final state.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/viz/kiosk-pallet` (released program looping over 4 parts with a report step at the end), when the harness runs to part 3, then the screen shows "part 3 of 4" and no skip or repeat control exists.
- **AC-2** Given the run completed, then the summary lists the report step with outcome `succeeded` and its entry hash, plus outcome counts matching the run's ResultRecords.
- **AC-3** Given a running run, then "Next part" is absent; after the final state it appears.

## Design

- **Registry command(s):** none; reads `program.get_run_state@1` and the run record (`AUTO.RUN.BIND`).
- **Data structures:** `CycleSummary { outcome, counts, end_actions: [{ step_title, command, outcome, entry }] }`; end-action classification by a fixed list of command-ID prefixes (`report.`, `export.`, `print.`) [unverified final RPT and IO command names].
- **Algorithm:** fold run events into the summary; read iteration path from step keys.
- **Errors:** none.
- **Determinism:** D0.
- **Performance budget:** summary ready < 200 ms after the final state [unverified].
- **Crate:** `fid-kiosk` (module `cycle::summary`).

## Test fixtures

`fixtures/viz/kiosk-pallet`: generated project with a released looping program; CC-BY-4.0.

## Build prompt

> In crate `fid-kiosk`, add module `cycle::summary`: show loop progress ("part i of n") from the run state's iteration path and declared loop bounds with no control that changes iteration; when the run is final, show the overall outcome and per-outcome counts through the kiosk status display, and list every executed report/export/print step with outcome and entry hash; offer "Next part" (back to serial entry) only in a final state. Tests AC-1..AC-3. Rules: the kiosk shows only what the program declares; no panics or `unsafe`; deny unwrap/expect/panic/todo/unimplemented/unreachable.

## Open questions

- Settles REQ-VIZ.KIOSK.CYCLE-003 by treating end-of-cycle actions as declared program steps rather than a separate kiosk feature; label printing needs an RPT or IO print command that does not yet exist [unverified].

## Children

_Leaf: none._
