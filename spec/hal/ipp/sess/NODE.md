---
id: HAL.IPP.SESS
title: I++ session, framing, queues, events and connection loss
level: 3
parent: HAL.IPP
kind: module
status: open
phase: P2
size: L
owners: []
claimed_by:
satisfies: [REQ-HAL.IPP-001, REQ-HAL.IPP-003]
covers: [PCD.MCH.01, PCD.MCH.02, DEV.CMM.01, DEV.CMM.02]
depends_on: [HAL.IDL.CNC, HAL.HOST.TIER1, HAL.SIM.IPPSIM]
cli: [fid device connect-cmm, fid device show-cmm]
mcp: [device_connect_cmm, device_show_cmm]
journal: [device.connect_cmm@1, device.show_cmm@1]
updated: 2026-11-16
---

# HAL.IPP.SESS: I++ session, framing, queues, events and connection loss

## Purpose

The protocol core of the I++ client: TCP connection, line framing and tags, normal and fast queues, event and error handling, session start and end, and the behaviour when the connection drops. It also enforces the motion boundary: motion-class instructions are accepted only from the Motion Executive over local IPC. Everything else in `HAL.IPP` runs on top of this.

## Scope

**In:**
- Connection to an I++ DME server; `device.connect_cmm@1` (D1, request-only on MCP) and `device.show_cmm@1` (R0)
- Line framing, tags, normal and fast queues, acknowledgement and completion tracking
- Session start and end, server errors mapped to the HAL error model
- Caller check: M1 instructions only from the Motion Executive over local IPC; refusal journaled otherwise
- Connection-loss detection, `COMM_LOST` reporting and the no-further-motion rule until a new session
- Hold and abort on the fast path, never delayed by data traffic

**Out** (and which node owns it instead):
- Tools, heads, changers and rotary configuration (owned by `HAL.IPP.TOOLS`)
- Measurement mapping (owned by `HAL.IPP.MEAS`)
- Manual and DCC mode events (owned by `HAL.IPP.MODES`)
- Run tickets, lease and the decision to move (owned by `CMM` execution)

## Requirements

- **REQ-HAL.IPP.SESS-001** The client shall accept M1 instructions only over a local IPC channel bound to the Motion Executive's principal, and shall reject any other caller with `motion_caller_rejected` and a journaled refusal.
- **REQ-HAL.IPP.SESS-002** When the TCP connection drops or the server stops responding past the configured deadline during motion, the client shall report `COMM_LOST` to the Motion Executive within 200 ms of detection [unverified until measured on hardware].
- **REQ-HAL.IPP.SESS-003** After `COMM_LOST`, the client shall send no motion command until a new session has been opened through a new journaled command.
- **REQ-HAL.IPP.SESS-004** Hold and abort shall be sent on the fast queue and shall never wait behind normal-queue commands or data-plane traffic.
- **REQ-HAL.IPP.SESS-005** Every server error shall be mapped to a code of the closed HAL error model, with the server text kept as `UntrustedText`.
- **REQ-HAL.IPP.SESS-006** The client shall be bounded and fail closed on malformed server lines (over-long lines, unknown tags, invalid numbers), returning a typed error and never crashing.
- **REQ-HAL.IPP.SESS-007** `device.connect_cmm@1` shall be class D1 and exposed on MCP only as a request; no command of this node shall be M1 on MCP.

## Interfaces

- **Offers:** I++ session API to sibling children and to the Motion Executive (local IPC); the two commands.
- **Consumes:** `HAL.IDL.CNC`; `HAL.HOST.TIER1` driver API; `HAL.SIM.IPPSIM` for tests. Instructions from the Motion Executive arrive over IPC (no build dependency on `CMM`).

## Decomposition hints

FRAME (line framing and parser; likely a leaf with a fuzz target), QUEUE (normal and fast queues, tags), GUARD (caller check), LOSS (connection-loss detection and recovery), CMDS (connect and show).

## Open questions

- The 200 ms bound and hold latency on real controllers are unknown until measured (inherited from the parent).

## Children

_None yet: status `open`._
