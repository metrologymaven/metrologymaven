---
id: HAL.ACQ.SESS
title: Device registry, sessions, configuration and health supervision
level: 3
parent: HAL.ACQ
kind: module
status: open
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-HAL.ACQ-009]
covers: []
depends_on: [HAL.IDL.BASE, HAL.IDL.NEG, CORE.BUS, CORE.JRN]
cli: [fid device open-session, fid device close-session, fid device configure-*, fid device show-*]
mcp: [device_open_session, device_close_session, device_configure_*, device_show_*]
journal: [device.open_session@1, device.close_session@1, device.configure_*, device.show_*]
updated: 2026-11-16
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

## Interfaces

- **Offers:** the commands above; `DeviceRegistry` and `SessionHandle` for sibling nodes, `HAL.LIVE`, `HAL.IPP` and `HAL.XSCR`; `device.session_opened` and `device.event` journal events.
- **Consumes:** `HAL.IDL.NEG` negotiate function; `HAL.IDL.BASE` error and health types; `CORE.BUS` command execution; `CORE.JRN`.

## Decomposition hints

REG (registry and source kinds), LIFE (open, close, interrupt, resume, writer lock), CFG (configure commands and digests), HEALTH (supervision and error-to-behaviour mapping), SHOW (R0 reads; a likely leaf).

## Open questions

- Whether request-only D1 tools ship on MCP in P2 or later (concept doc 08 H-D18) is inherited from the parent; this node assumes request-only from the first release that exposes them.

## Children

_None yet: status `open`._
