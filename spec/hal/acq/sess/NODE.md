---
id: HAL.ACQ.SESS
title: Device registry, sessions, configuration and health supervision
level: 3
parent: HAL.ACQ
kind: module
status: split
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-HAL.ACQ-009]
covers: []
depends_on: [HAL.IDL.BASE, HAL.IDL.NEG, HAL.ACQ.TIME, HAL.PAIR.GRADE, CORE.BUS, CORE.JRN]
cli: [fid device open-session, fid device close-session, fid device resume-session, fid device configure-*, fid device show-*]
mcp: [device_open_session, device_close_session, device_resume_session, device_configure_*, device_show_*]
journal: [device.open_session@1, device.close_session@1, device.resume_session@1, device.configure_*, device.show_*]
updated: 2026-11-17
---

# HAL.ACQ.SESS: Device registry, sessions, configuration and health supervision

## Purpose

The control side of the runtime: the registry of connected device instances and their negotiated capabilities, the session lifecycle (one controlling writer, any number of telemetry subscribers), configuration changes as journaled D1 commands, and supervision of each device's health state machine. Every state-changing call to a driver passes through here, so this is where the "no side doors" rule for devices is enforced.

## Scope

**In:**
- Device registry: instances, serving source (driver, simulator, playback, file pseudo-device), negotiated capabilities
- `device.open_session@1`, `device.close_session@1` (D1); interrupt and resume semantics; the `device.session_opened` event with the negotiation result
- Controlling-writer lock and read-only telemetry subscriptions
- `device.configure_*` (D1) commands with before and after configuration digests
- `device.show_*` (R0) status and registry reads
- Health supervision: tracking the health state machine, mapping error codes to runtime behaviour (retry, interrupt, fail)
- The rule that driver control calls are issued only for journaled registry commands, and that D1 commands are request-only on MCP

**Out** (and which node owns it instead):
- Start and stop of acquisition streams and the data plane (owned by `HAL.ACQ.INGEST`)
- Stamping and journaling of batches (owned by `HAL.ACQ.STAMP`)
- Time mapping (owned by `HAL.ACQ.TIME`)
- Host process supervision (owned by `HAL.HOST.SUPER`)
- M1 motion commands (owned by `CMM` execution through `HAL.IPP`)

## Requirements

- **REQ-HAL.ACQ.SESS-001** When `device.open_session@1` runs, the runtime shall execute capability negotiation and journal a `device.session_opened` event holding the negotiation result, the configuration digest and the time-source declaration.
- **REQ-HAL.ACQ.SESS-002** While a session is open, the runtime shall allow exactly one controlling principal; a second request for control shall fail with `session_busy`, while telemetry subscriptions shall remain unlimited and read-only.
- **REQ-HAL.ACQ.SESS-003** The runtime shall issue a state-changing driver call only while executing a journaled registry command of class D1, or M1 through the Motion Executive, and shall reject any other origin with `no_command_context`.
- **REQ-HAL.ACQ.SESS-004** When a `device.configure_*` command succeeds, the journal entry shall record the configuration digests before and after; when it fails with `CONFIG_INVALID` or `OUT_OF_RANGE`, only the failure shall be journaled.
- **REQ-HAL.ACQ.SESS-005** When the driver reports `COMM_LOST`, the runtime shall mark the session `INTERRUPTED`, keep batches already stamped valid but flagged, and require an explicit resume or close command.
- **REQ-HAL.ACQ.SESS-006** D1 commands of this node shall be projected to MCP only as requests that a human must confirm, and agents shall never hold the controlling-writer lock directly (CONSTITUTION §3).
- **REQ-HAL.ACQ.SESS-007** The runtime shall keep a registry of every serving source in which the source kind (`driver`, `simulator`, `emulator`, `playback`, `file`) is set by the registering component and cannot be changed by the source or by any later command.

## Interfaces

- **Offers:** the commands above; `DeviceRegistry` and `SessionHandle` for sibling nodes, `HAL.LIVE`, `HAL.IPP` and `HAL.XSCR`; `device.session_opened` and `device.event` journal events.
- **Consumes:** `HAL.IDL.NEG` negotiate function; `HAL.IDL.BASE` error and health types; `CORE.BUS` command execution; `CORE.JRN`.

## Decomposition hints

REG (registry and source kinds), LIFE (open, close, interrupt, resume, writer lock), CFG (configure commands and digests), HEALTH (supervision and error-to-behaviour mapping), SHOW (R0 reads; a likely leaf).

## Split rationale

The split separates the **P1a registry** (REG), which file pseudo-devices and the stamping path need before any live driver exists, from the **P2 live-session machinery**: the session lifecycle with its lock and frozen context (LIFE), the driver-call gate that enforces "no side doors" (GATE), configuration commands (CFG) and health supervision (HEALTH). REQ-HAL.ACQ.SESS-007 was added at this split to state the registry responsibility that the Scope already held but no requirement named.

Trust grading is wired here, in LIFE (P2), because grade inputs (driver signature, conformance report, pairing) only exist for live sources; this keeps `HAL.ACQ.STAMP` (P1a) free of any dependency on `HAL.PAIR`.

Alternatives considered:
- **SHOW as its own child.** Rejected: each read belongs with the state it shows (devices in REG, sessions in LIFE, configuration in CFG, health in HEALTH).
- **GATE inside LIFE.** Rejected: the gate wraps calls from CFG, INGEST and IPP too, and must be testable alone.

## Open questions

- Whether request-only D1 tools ship on MCP in P2 or later (concept doc 08 H-D18) is inherited from the parent; this node assumes request-only from the first release that exposes them.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| HAL.ACQ.SESS.REG | Device registry of serving sources and their kinds (leaf, P1a) | 007 | none |
| HAL.ACQ.SESS.LIFE | Session lifecycle, controlling-writer lock and session context (leaf) | 001 002 005 006 | none |
| HAL.ACQ.SESS.GATE | Driver-call gate (leaf) | 003 | none |
| HAL.ACQ.SESS.CFG | Device configuration commands and configuration digests (leaf) | 004 006 | none |
| HAL.ACQ.SESS.HEALTH | Health supervision and error-to-behaviour mapping (leaf) | 005 | none |
