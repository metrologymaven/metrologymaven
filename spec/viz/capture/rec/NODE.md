---
id: VIZ.CAPTURE.REC
title: Recording sessions, indicator and governance
level: 3
parent: VIZ.CAPTURE
kind: module
status: split
phase: P1b
size: L
owners: []
claimed_by:
satisfies: [REQ-VIZ.CAPTURE-006, REQ-VIZ.CAPTURE-007, REQ-VIZ.CAPTURE-008, REQ-VIZ.CAPTURE-009]
covers: []
depends_on: [CORE, VIZ.SHELL.DOCK]
cli: [fid capture start-recording, fid capture stop-recording, fid capture get-recording-status]
mcp: [capture_start_recording, capture_stop_recording, capture_get_recording_status]
journal: [capture.start_recording@*, capture.stop_recording@*, capture.get_recording_status@*]
updated: 2026-11-17
---

# VIZ.CAPTURE.REC: Recording sessions, indicator and governance

## Purpose

The recording session that view-track and video capture run inside: start and stop commands, the always-visible recording indicator, org policy (mandate, forbid, allow screenshots only), panel redaction, local-only storage with retention, and the rule that an agent can only propose a recording, never start one.

## Scope

**In:**
- `capture.start_recording@1` and `capture.stop_recording@1`
- Recording policy model in org settings (mandate, forbid, screenshots-only, retention)
- Recording indicator in every app window (hosted by the shell)
- Agent start requests turned into Inbox proposals
- Panel redaction rules for captured frames
- Local storage of recorded media in the project store

**Out** (and which node owns it instead):
- View-track sampling (`VIZ.CAPTURE.TRACK`); video readback and segments (`VIZ.CAPTURE.VIDEO`)
- Export of media (`VIZ.CAPTURE.EXPORT`)
- Inbox UI (`VIZ.INBOX`)
- Setting org policy itself (A0, `CORE`)

## Requirements

- **REQ-VIZ.CAPTURE.REC-001** While recording, an indicator shall be visible in every window of the app, and no theme, skin, kiosk layout or agent shall be able to hide it.
- **REQ-VIZ.CAPTURE.REC-002** When an agent principal calls `capture.start_recording@1`, the system shall create a proposal for a human to accept and shall not start recording.
- **REQ-VIZ.CAPTURE.REC-003** If org policy forbids recording, then start shall fail with `PolicyForbidden` and journal nothing.
- **REQ-VIZ.CAPTURE.REC-004** While org policy mandates recording, the system shall start recording at session start and shall refuse stop by non-admin principals.
- **REQ-VIZ.CAPTURE.REC-005** Recorded media shall be stored only in the local project store, and the module shall offer no path that sends media off the machine.
- **REQ-VIZ.CAPTURE.REC-006** Panels marked for redaction shall be blanked in every captured frame before encoding.

## Interfaces

- **Offers:** recording session state and policy API used by SHOT, TRACK, VIDEO and OSCAP; `capture.start_recording@1`, `capture.stop_recording@1`.
- **Consumes:** CORE settings, principals and proposals; `VIZ.SHELL.DOCK` indicator host.

## Decomposition hints

SESSION (start, stop, state), POLICY, INDIC (indicator), AGENTREQ (proposal path), REDACT, RETAIN.

## Open questions

- Retention rules may later follow an export-control classification in project metadata (candidate owner `CORE.PRJ.META`); `VIZ.CAPTURE.REC.POLICY` only computes `retain_until`, and payload disposal is TRUST's (per `CORE.STO.CAS`), not yet specified there.
- Split: policy (data), session (commands and the Activity Log recording flag, CONSTITUTION Â§3 D-031), indicator (UI rule) and redaction (frame rule) are independent and each testable alone. Agent start requests use `human_only` and the generic pending-request path (`CORE.BUS.POL.PROJ`), approved in `VIZ.INBOX.ACTIONS.REQS`. Media leaves the machine only through `egress.send@1` (CONSTITUTION Â§3.1).
- `VIZ.SHELL.DOCK` must expose a reserved per-window draw layer and per-frame layout snapshots (requested by INDIC and REDACT).

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| VIZ.CAPTURE.REC.POLICY | Recording policy keys and policy evaluation (leaf) | 003 004 | none |
| VIZ.CAPTURE.REC.SESSION | Recording session start, stop and status (leaf) | 002 003 004 005 | none |
| VIZ.CAPTURE.REC.INDIC | Unsuppressible recording indicator (leaf) | 001 | none |
| VIZ.CAPTURE.REC.REDACT | Panel redaction marks and frame masking (leaf) | 006 | none |
