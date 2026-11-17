---
id: CMM.EXEC.STATION
title: Approval Station view and flow
level: 3
parent: CMM.EXEC
kind: module
status: open
phase: P2
size: L
owners: []
claimed_by:
satisfies: [REQ-CMM.EXEC-022]
covers: []
depends_on: [VIZ, CMM.EXEC.TICKET, CMM.EXEC.AUTHN]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# CMM.EXEC.STATION: Approval Station view and flow

## Purpose

The at-machine kiosk where a present human reviews and approves a run. It pulls queued run requests, asks the Motion Executive for the data to show, and renders only that data: a diff against the last released version, the simulation report summary, the ticket tuple (machine, setup, probe configuration, speed cap, scope, expiry), the released-by and requested-by principals, and a prominent agent-authored flag. Program and part names are shown as quoted, badged untrusted text. It is never a chat chip and never rendered by the engineering-workstation engine.

## Scope

**In:**
- Station flow: pull request, fetch Motion Executive view data, show, approve or decline, hand the assertion to the Motion Executive over local IPC
- Rendering rules: only Motion Executive-supplied fields; untrusted text badged; agent-authored flag; diff against last released version
- Scope display (setup approval versus run-batch authorization)
- Operator-presence heartbeat source during setup runs (sending it; the Motion Executive enforces it)

**Out** (and which node owns it instead):
- Assertion verification (owned by `CMM.EXEC.AUTHN`)
- Ticket content (owned by `CMM.EXEC.TICKET`)
- Heartbeat enforcement and hold (owned by `CMM.EXEC.DRIVE` and `CMM.EXEC.SCOPE`)
- UI toolkit and kiosk shell (owned by `VIZ`)

## Requirements

- **REQ-CMM.EXEC.STATION-001** The Approval Station shall render only data supplied by the Motion Executive over local IPC, and shall show nothing received from the engineering-workstation engine.
- **REQ-CMM.EXEC.STATION-002** The approval view shall always show a diff against the last released version of the program, the simulation report summary, the full ticket tuple and the granted scope.
- **REQ-CMM.EXEC.STATION-003** When the program is agent-authored, the view shall show an agent-authored flag that cannot be collapsed or hidden.
- **REQ-CMM.EXEC.STATION-004** The view shall render program names, part names and any other content-derived strings as quoted, badged untrusted text, never as instructions or UI labels.
- **REQ-CMM.EXEC.STATION-005** A kiosk click shall never start a cycle; the station's only motion-related outputs shall be the approval assertion, the decline and the operator-presence heartbeat.

## Interfaces

- **Offers:** station view model; local IPC client to the Motion Executive.
- **Consumes:** `CMM.EXEC.TICKET` view data; `CMM.EXEC.AUTHN` assertion hand-off; `VIZ` kiosk shell.
- **Journal:** none directly (approvals are journaled by `cmm.approve_ticket@1`).

## Decomposition hints

FLOW (pull, view, approve, decline), VIEW (rendering rules and diff), PRESENCE (heartbeat source).

## Open questions

- Accessibility of the agent-authored flag (colour, text, sound) needs a UX review.

## Children

_None yet: status `open`._
