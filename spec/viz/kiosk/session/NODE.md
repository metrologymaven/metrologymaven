---
id: VIZ.KIOSK.SESSION
title: Kiosk session enter, exit and resume
level: 3
parent: VIZ.KIOSK
kind: feature
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-VIZ.KIOSK-003, REQ-VIZ.KIOSK-007]
covers: []
depends_on: [CORE.BUS, CORE.STO, AUTO.RUN, VIZ.SHELL.DOCK]
side_effect: A0
determinism: D0
cli: [fid view enter-kiosk, fid view exit-kiosk, fid view get-kiosk-status]
mcp: [view_get_kiosk_status]
journal: [view.enter_kiosk@1, view.exit_kiosk@1]
updated: 2026-11-16
---

# VIZ.KIOSK.SESSION: Kiosk session enter, exit and resume

## Purpose

The kiosk session lifecycle: an admin enters kiosk mode on a workstation (locking the shell to the kiosk surface), an admin leaves it, and after any restart during a cycle the app comes back in kiosk mode at the last committed step. Entering and leaving are A0 commands, journaled and never available to agents; status is an R0 read.

## Scope

**In:** `view.enter_kiosk@1` (A0), `view.exit_kiosk@1` (A0), `view.get_kiosk_status@1` (R0), the persisted kiosk-session record, start-up resume, shell lock-down switch.
**Out:** program choice (`VIZ.KIOSK.PROGPICK`); operator steps (`VIZ.KIOSK.STEPS`); program run state (`AUTO.RUN`); principals and admin roles (`CORE.BUS`).

## Requirements

- **REQ-VIZ.KIOSK.SESSION-001** `view.enter_kiosk@1` and `view.exit_kiosk@1` shall succeed only for an admin principal and shall be journaled.
- **REQ-VIZ.KIOSK.SESSION-002** Neither enter nor exit shall be projected to MCP or accepted from the control channel or any agent principal.
- **REQ-VIZ.KIOSK.SESSION-003** While kiosk mode is active, the shell shall show only the kiosk surface, and no menu, palette, panel or shortcut of the full shell shall be reachable.
- **REQ-VIZ.KIOSK.SESSION-004** If the application restarts while a kiosk session is active, then it shall start in kiosk mode and resume the running cycle at its last committed step.
- **REQ-VIZ.KIOSK.SESSION-005** `view.get_kiosk_status@1` shall return whether kiosk mode is active, the station ID, the program and revision in use, the cycle number and the current step.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/viz/kiosk-station` (project with one released program and an admin principal), when `fid view enter-kiosk --station S1 --json` runs as admin, then it succeeds and the journal holds one `view.enter_kiosk@1` entry.
- **AC-2** (error path) Given an operator principal, when the same command runs, then it fails with `AdminRequired` and journals nothing.
- **AC-3** Given the generated MCP tool list, then it contains `view_get_kiosk_status` and contains no tool for entering or exiting kiosk mode.
- **AC-4** Given a kiosk cycle at step 3 of 5 with step 3 committed, when the process is killed and restarted, then the app starts in kiosk mode and `fid view get-kiosk-status --json` reports step 3 as last committed and step 4 as current.
- **AC-5** Given kiosk mode active, when the UI test harness sends the palette shortcut and every full-shell menu accelerator, then no full-shell surface opens.

## Design

- **Registry command(s):** `view.enter_kiosk@1` (A0; params `station: StationId`), `view.exit_kiosk@1` (A0; params `station`, `reason: String`), `view.get_kiosk_status@1` (R0; result `KioskStatus { active, station, program?, revision?, cycle?, step?, last_committed_step? }`).
- **Data structures:** persisted `KioskSession { station, entered_by, entered_at, program_ref?, cycle_ref? }` in project settings via crash-safe storage.
- **Algorithm:** on enter, write the session record atomically, switch the shell to the kiosk surface; at start-up, read the record and, if active, open the kiosk surface and ask AUTO.RUN for the last committed step of the open cycle.
- **Errors:** `AdminRequired`, `AlreadyActive`, `NotActive`, `UnknownStation`.
- **Determinism:** D0 (state transitions only).
- **Performance budget:** resume to the kiosk surface within 10 s of process start [unverified].
- **Crate:** `fid-kiosk`.

## Test fixtures

`fixtures/viz/kiosk-station`: generated project with one released scan program and admin and operator principals; in-repo.

## Build prompt

> Create crate `fid-kiosk` with registry commands `view.enter_kiosk@1` and `view.exit_kiosk@1` (side-effect class A0: admin only, journaled, never on MCP or the control channel) and `view.get_kiosk_status@1` (R0; CLI `fid view get-kiosk-status`, MCP `view_get_kiosk_status`). Persist a `KioskSession` record with crash-safe writes; on start-up, if active, open only the kiosk surface and resume the open cycle at its last committed step from the program runner. While active, no full-shell menu, palette, panel or shortcut is reachable. Tests: admin enter journals one entry; operator gets `AdminRequired` and nothing is journaled; MCP tool list contains only the status tool; kill-and-restart resumes at the right step; full-shell shortcuts are inert. Rules: no panics or `unsafe`; deny unwrap/expect/panic/todo/unimplemented/unreachable; agents never perform A0 (CONSTITUTION §3).

## Open questions

- Station identity and pairing to a workstation are assumed to be project settings; PLAT may own kiosk hardware profiles.

## Children

_Leaf: none._
