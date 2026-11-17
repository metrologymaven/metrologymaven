---
id: HAL.IPP.MODES
title: Manual and DCC modes and jog-box events
level: 3
parent: HAL.IPP
kind: task
status: leaf
phase: P2
size: S
owners: []
claimed_by:
satisfies: [REQ-HAL.IPP-006]
covers: [PCD.MCH.05, PCD.MCH.06]
depends_on: [HAL.IPP.SESS, HAL.SIM.IPPSIM]
side_effect: D1
determinism: N
cli: []
mcp: []
journal: [device.event]
updated: 2026-11-16
---

# HAL.IPP.MODES: Manual and DCC modes and jog-box events

## Purpose

Records what the operator does at the machine: switches between manual and direct-computer-control (DCC) mode, jog-box key events, and manually taken points. These are observed and journaled as device events, never turned into motion commands, so the operator's physical actions are in the record without opening a side door to motion.

## Scope

**In:**
- Mapping controller-reported mode changes (manual, DCC) to `device.event` records
- Mapping jog-box events (keys, override changes, manual point triggers) to `device.event` records
- Marking points taken in manual mode as `manual` so `HAL.IPP.MEAS` maps them as observations
- Tracking current mode in `device.show_cmm@1` output

**Out:**
- Mapping of measured points (owned by `HAL.IPP.MEAS`)
- Any motion (owned by `CMM` execution)
- Jog-box hardware itself (controller-side)

## Requirements

- **REQ-HAL.IPP.MODES-001** When the controller reports a change between manual and DCC mode, the client shall journal a `device.event` of kind `MODE_CHANGED` with the new mode and the device timestamp.
- **REQ-HAL.IPP.MODES-002** When the controller reports a jog-box event, the client shall journal a `device.event` of kind `JOG_EVENT` with the event type, and shall not issue, queue or modify any motion command as a result.
- **REQ-HAL.IPP.MODES-003** When a point is reported while the machine is in manual mode, the client shall mark it `manual: true` for measurement mapping.
- **REQ-HAL.IPP.MODES-004** While the machine is in manual mode, the client shall reject M1 instructions from the Motion Executive with `machine_in_manual_mode`.
- **REQ-HAL.IPP.MODES-005** Event text from the controller shall be stored as `UntrustedText`.

## Acceptance criteria

- **AC-1** Given the L0 scenario `fixtures/hal/ipp/modes/manual_then_dcc.scn` on the I++ simulator, when a session runs, then the journal holds `MODE_CHANGED(manual)` then `MODE_CHANGED(dcc)` with device timestamps in order.
- **AC-2** Given `fixtures/hal/ipp/modes/jog_keys.scn` (12 jog events), when a session runs, then 12 `JOG_EVENT` entries are journaled and the client sends no line to the server other than those in the scenario's expected client lines.
- **AC-3** Given `fixtures/hal/ipp/modes/manual_points.scn` (5 points in manual mode), when the session runs, then 5 points reach measurement mapping with `manual: true`.
- **AC-4** (error path) Given the machine in manual mode, when the Motion Executive sends a go-to instruction, then the client rejects it with `machine_in_manual_mode` and sends nothing to the server.
- **AC-5** Given a jog event whose text contains an instruction-like string, when journaled, then it is stored as `UntrustedText` and not interpreted.

## Design

- **Registry commands:** none of its own; runs inside sessions opened by `device.connect_cmm@1` (D1). Events: `device.event` kinds `MODE_CHANGED`, `JOG_EVENT`.
- **Data structures:** `MachineMode { Manual, Dcc, Unknown }`, `JogEvent { kind, value: Option<UntrustedText>, t_dev }`.
- **Algorithm:** event dispatch from the session's event stream; mode state machine; guard on M1 dispatch.
- **Errors:** `machine_in_manual_mode`.
- **Determinism:** N (device I/O); journaled events replay exactly.
- **Performance budget:** event handling < 1 ms [unverified until measured].
- **Crate:** `fid-hal-ipp` (module `modes`).

## Test fixtures

L0 scenario files for the open I++ simulator under `fixtures/hal/ipp/modes/` (project-authored, Apache-2.0), written from founder spec cards without specification text.

## Build prompt

> In crate `fid-hal-ipp`, module `modes`, handle controller mode and jog-box events from the I++ session event stream: journal `device.event` kind `MODE_CHANGED` (manual|dcc, device timestamp) and kind `JOG_EVENT` (type, optional untrusted text) and never issue, queue or change motion as a result; mark points reported in manual mode `manual: true` for the measurement mapper; reject M1 instructions while in manual mode with `machine_in_manual_mode` without sending anything to the server; expose current mode in `device.show_cmm@1`. Controller text is `UntrustedText`. No panic, unwrap, expect or unsafe (CONSTITUTION §8). Tests: AC-1..AC-5 against the open I++ simulator at L0 with scenarios in `fixtures/hal/ipp/modes/`. Do not paste I++ specification text into code or comments; cite clause IDs from the spec cards only.

## Open questions

- The exact I++ event names for mode and jog-box events wait for the spec cards [unverified].

## Children

_Leaf: none._
