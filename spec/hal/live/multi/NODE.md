---
id: HAL.LIVE.MULTI
title: Multi-instrument live sessions
level: 3
parent: HAL.LIVE
kind: module
status: open
phase: P3
size: L
owners: []
claimed_by:
satisfies: [REQ-HAL.LIVE-007, REQ-HAL.LIVE-008]
covers: [SA.INS.08]
depends_on: [HAL.ACQ.SESS, HAL.ACQ.TIME, HAL.LIVE.PROBE, HAL.SIM.PORT]
cli: [fid device open-group, fid device close-group]
mcp: [device_open_group, device_close_group]
journal: [device.open_group@1, device.close_group@1]
updated: 2026-11-16
---

# HAL.LIVE.MULTI: Multi-instrument live sessions

## Purpose

Lets one operator run several instruments at once (for example two trackers and an arm on a large assembly) as one session group with a shared timebase and per-device status, so a fault on one device does not stop the others. It is separate because group lifecycle and fault isolation cut across all the single-device workflows.

## Scope

**In:**
- `device.open_group@1`, `device.close_group@1` (D1): a named group of device sessions
- Shared session timebase and per-device clock models
- Per-device status and fault isolation within the group
- Group-level view of active workflows (probing, scanning) per device
- Running groups of simulators for training and tests

**Out** (and which node owns it instead):
- Single-device sessions (owned by `HAL.ACQ.SESS`)
- Combining observations into one network (owned by `LVM`)

## Requirements

- **REQ-HAL.LIVE.MULTI-001** While a group is open, the system shall keep and expose a separate status for each device in the group.
- **REQ-HAL.LIVE.MULTI-002** If one device in a group faults or is interrupted, then the system shall continue acquiring from the other devices and journal the fault against that device only.
- **REQ-HAL.LIVE.MULTI-003** All sessions in a group shall share one session timebase anchored once when the group opens.
- **REQ-HAL.LIVE.MULTI-004** A group of simulators shall run end to end with all results flagged simulated.

## Interfaces

- **Offers:** group commands and status for `VIZ`; grouped observation streams for `LVM`.
- **Consumes:** `HAL.ACQ.SESS`, `HAL.ACQ.TIME`, `HAL.LIVE.PROBE`, `HAL.SIM.PORT`.

## Decomposition hints

GROUP (group lifecycle; likely a leaf), ISOL (fault isolation), STATUS (per-device status).

## Open questions

- Whether one operator principal may hold the controlling-writer lock of several devices at once is assumed yes; `HAL.ACQ.SESS` must allow it.

## Children

_None yet: status `open`._
