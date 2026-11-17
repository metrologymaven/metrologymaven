---
id: CMM.EXEC.DRIVE
title: Armed run loop, lease, look-ahead and hold
level: 3
parent: CMM.EXEC
kind: module
status: open
phase: P2
size: XL
owners: []
claimed_by:
satisfies: [REQ-CMM.EXEC-002, REQ-CMM.EXEC-015, REQ-CMM.EXEC-016, REQ-CMM.EXEC-017, REQ-CMM.EXEC-018]
covers: [PW.CMM.01]
depends_on: [HAL.IPP, CMM.PATH.PLAN, CMM.EXEC.TICKET, CMM.EXEC.VALID]
cli: [fid cmm get-run-state]
mcp: [cmm_get_run_state]
journal: [cmm.arm_run@1, cmm.hold_run@1, cmm.resume_run@1]
updated: 2026-11-16
---

# CMM.EXEC.DRIVE: Armed run loop, lease, look-ahead and hold

## Purpose

The part of the Motion Executive that actually drives the controller through the HAL I++ client once a ticket is armed: it holds the single-writer lease per machine and keeps the I++ session open while armed, streams plan moves with bounded look-ahead beyond the controller-confirmed position, executes holds (requested, heartbeat-missed or session-lost) with measured latency, observes but never touches the hardware emergency stop, executes on-machine alignment (observing manual pre-alignment hits, then DCC re-alignment), and collects hits. Resume is a local human act only.

## Scope

**In:**
- Run state machine: armed, running, holding, aborted, completed, faulted
- Single-writer lease per machine; continuous I++ session while armed; alarm and hold on unexpected session loss
- Look-ahead bound in moves and seconds; single-move look-ahead for controllers recorded as continuing after client disconnect
- Hold execution: controller stop or abort, measured latency, journaled
- Emergency-stop observation only
- On-machine alignment execution: manual pre-alignment points observed as controller-side joystick hits, then DCC re-alignment through `ALN`
- Hit collection with provenance stamps
- Local-only resume; MCP connectivity has no effect in either direction
- `cmm.get_run_state@1` (R0) for status

**Out** (and which node owns it instead):
- I++ protocol client (owned by `HAL.IPP`)
- Hold requests (owned by `CMM.EXEC.REQ`)
- Speed scope, presence heartbeat policy and placement check (owned by `CMM.EXEC.SCOPE`)
- Run records (owned by `CMM.EXEC.RECORD`)
- Alignment solvers (owned by `ALN`)

## Requirements

- **REQ-CMM.EXEC.DRIVE-001** The Motion Executive shall hold one single-writer lease per machine for the armed ticket, keep the I++ session open while armed, and raise an alarm and hold on any unexpected session loss.
- **REQ-CMM.EXEC.DRIVE-002** The Motion Executive shall never have more moves or seconds of motion enqueued beyond the controller-confirmed position than the ticket's look-ahead bound, and shall use a bound of one move for controllers whose compatibility record says they continue motion after client disconnect.
- **REQ-CMM.EXEC.DRIVE-003** When a hold is requested, a station heartbeat is missed or the session is lost, the Motion Executive shall issue the controller's stop or abort and journal the hold with its cause and measured latency.
- **REQ-CMM.EXEC.DRIVE-004** The Motion Executive shall only observe the emergency-stop state reported by the controller and shall have no code path that triggers, writes or resets it.
- **REQ-CMM.EXEC.DRIVE-005** Resume after a hold shall require a local human act at the station or pendant; no MCP message, MCP disconnect or MCP reconnect shall start, continue or resume motion.
- **REQ-CMM.EXEC.DRIVE-006** During on-machine alignment, the Motion Executive shall record manual pre-alignment points as observations of controller-side motion it does not command, then run the DCC re-alignment moves from the plan.

## Interfaces

- **Offers:** run state machine; `cmm.get_run_state@1` (R0); hold and resume events; collected observations for `CMM.EXEC.RECORD`.
- **Consumes:** `HAL.IPP` client and controller compatibility record; `CMM.PATH.PLAN` plan; `CMM.EXEC.TICKET` armed ticket; `CMM.EXEC.VALID` pre-flight re-validation; `ALN` for re-alignment.
- **Journal:** `cmm.arm_run@1`, `cmm.hold_run@1`, `cmm.resume_run@1`.

## Decomposition hints

LEASE (lease and session), STREAM (look-ahead streaming), HOLD (hold execution and latency), ESTOP (observation only), ALIGN (on-machine alignment), STATE (run state machine).

## Open questions

- I++ DME details (port, fast-queue stop semantics) are from concept docs and are [unverified] against VDMA 8722:2026-10.
- An OS or hardware watchdog independent of the engine supervises the Motion Executive (HAL concept doc §6.5); whether that watchdog belongs here or in `PLAT` is open.

## Children

_None yet: status `open`._
