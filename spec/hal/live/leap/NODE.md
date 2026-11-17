---
id: HAL.LIVE.LEAP
title: Device positions and common points (leapfrog bookkeeping)
level: 3
parent: HAL.LIVE
kind: feature
status: leaf
phase: P3
size: M
owners: []
claimed_by:
satisfies: [REQ-HAL.LIVE-005]
covers: [PW.PRB.04]
depends_on: [HAL.LIVE.PROBE, HAL.ACQ.STAMP, HAL.SIM.PORT]
side_effect: W2
determinism: D0
cli: [fid device record-leapfrog]
mcp: [device_record_leapfrog]
journal: [device.record_leapfrog@1]
updated: 2026-11-16
---

# HAL.LIVE.LEAP: Device positions and common points (leapfrog bookkeeping)

## Purpose

When an operator moves a tracker or arm to a new position, the measurements from the new position live in a new frame. This leaf records each device position as its own frame and records which common points (targets) were measured from which positions, so `LVM` can compute and adjust the station transforms. It does bookkeeping only; it computes no transform.

## Scope

**In:**
- `device.record_leapfrog@1` (W2): close the current device position and open a new one for the same device instance
- `DevicePosition` records: ID, device instance, sequence index, new `FrameId`, opening and closing journal entries
- Common-point records: target ID, position ID, batch reference of the measurement
- A consistency check of common points between consecutive positions
- An event that `LVM` consumes

**Out:**
- Computing station transforms and network adjustment (owned by `LVM`)
- Measuring the points (owned by `HAL.LIVE.PROBE`)

## Requirements

- **REQ-HAL.LIVE.LEAP-001** When `device.record_leapfrog@1` runs, the system shall close the device's current position, open a new position with a new frame ID, and journal both with the device instance and sequence index.
- **REQ-HAL.LIVE.LEAP-002** Every point measured while a position is open shall be linked to that position, and points tagged with a target ID shall be recorded as common-point measurements.
- **REQ-HAL.LIVE.LEAP-003** When a new position is completed, the system shall report how many targets were measured from both it and the previous position, and shall flag the move `leapfrog_common_points_low` when the count is below 3.
- **REQ-HAL.LIVE.LEAP-004** The system shall never compute or store a transform between positions; it shall publish the positions and common-point records for `LVM` to adjust.
- **REQ-HAL.LIVE.LEAP-005** If the command names a device with no open session or no current position, then it shall fail with `leapfrog_no_session` and journal nothing.

## Acceptance criteria

- **AC-1** Given the simulated tracker scenario `fixtures/hal/live/leap/two_stations.json` (6 targets measured from station 1, a scripted move, 5 of the same targets measured from station 2), when `fid device record-leapfrog --json` runs at the move, then two positions exist with different frame IDs, 11 common-point records are linked to their positions, and the completion report shows 5 shared targets.
- **AC-2** Given `fixtures/hal/live/leap/two_stations_sparse.json` (2 shared targets), when the second position is completed, then the move is flagged `leapfrog_common_points_low`.
- **AC-3** (error path) Given no open session, when `fid device record-leapfrog` runs, then it fails with `leapfrog_no_session` and journals nothing.
- **AC-4** Given AC-1, when the journal is inspected, then no transform value appears in any entry of this command.
- **AC-5** Given AC-1, every common-point record references a batch stamped `simulated: true`.
- **AC-6** (replay) Given the journal of AC-1, when replayed in exact mode, then the outcome is `IDENTICAL`.

## Design

- **Registry command:** `device.record_leapfrog@1`, class W2 (journaled working-document change; agents write to an agent branch); params `device: Handle`, optional `label: String`; result `{ closed: PositionId, opened: PositionId, frame: FrameId }`. Completion report as an R0 field of `device.show_session`.
- **Data structures:** `DevicePosition { id: UUIDv7, device, seq: u32, frame: FrameId, opened_at, closed_at }`, `CommonPoint { target: TargetId, position: PositionId, batch: BatchRef }`.
- **Algorithm:** bookkeeping in the project document; shared-target count by sorted-set intersection of target IDs.
- **Errors:** `leapfrog_no_session`; flag `leapfrog_common_points_low`.
- **Determinism:** D0 (bookkeeping on journaled inputs; new IDs come from the journal's ID source).
- **Performance budget:** trivial.
- **Crate:** `fid-hal-live` (module `leap`).

## Test fixtures

Scripted tracker-simulator scenarios under `fixtures/hal/live/leap/` (CC-BY-4.0), using `HAL.SIM.PORT` with a fixed seed.

## Build prompt

> In crate `fid-hal-live`, module `leap`, implement registry command `device.record_leapfrog@1` (side effect W2; CLI `fid device record-leapfrog`, MCP `device_record_leapfrog`; params: device handle, optional label). It closes the device's current `DevicePosition`, opens a new one with a new `FrameId` and sequence index, and journals both. Link every point measured while a position is open to it; points tagged with a target ID become `CommonPoint` records. On completion, report shared-target count with the previous position and flag `leapfrog_common_points_low` below 3. Never compute or store transforms; publish positions and common points for LVM. Fail `leapfrog_no_session` (journal nothing) if the device has no open session or position. IDs come from the journal's ID source (UUIDv7). No panic, unwrap, expect or unsafe (CONSTITUTION §8). Tests: AC-1..AC-6 using the tracker simulator scenarios in `fixtures/hal/live/leap/`.

## Open questions

- `LVM` may require more than 3 shared targets for a 6DoF transform with redundancy; the flag threshold here is a minimum warning, not an acceptance rule.

## Children

_Leaf: none._
