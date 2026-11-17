---
id: HAL.SIM.IPPSIM
title: Open I++ DME simulator (protocol engine, L0 and L1) and simulator launcher
level: 3
parent: HAL.SIM
kind: module
status: open
phase: P0
size: XL
owners: []
claimed_by:
satisfies: [REQ-HAL.SIM-001, REQ-HAL.SIM-002, REQ-HAL.SIM-005, REQ-HAL.SIM-007, REQ-HAL.SIM-008]
covers: [DEV.SIM.01]
depends_on: [HAL.IDL.CNC, HAL.SIM.WORK]
cli: [fid device start-sim, fid device stop-sim]
mcp: [device_start_sim, device_stop_sim]
journal: [device.start_sim@1, device.stop_sim@1]
updated: 2026-11-16
---

# HAL.SIM.IPPSIM: Open I++ DME simulator (protocol engine, L0 and L1) and simulator launcher

## Purpose

A standalone, permissively licensed I++ DME server that behaves like a CMM controller: it speaks the protocol on a TCP socket, runs scripted scenarios (L0) or ideal Cartesian kinematics against a virtual workpiece (L1), and can emulate known controller dialects. It is the hardware-free target for the I++ client, the Motion Executive and every CMM workflow test. This node also owns the generic launcher commands that start and stop any simulator as a device source.

## Scope

**In:**
- Protocol engine: line framing, tags, normal and fast queues, events, errors; session, tools, coordinate systems
- L0: scenario files of scripted responses for protocol-level tests
- L1: Cartesian three-axis kinematics with ideal motion, using `HAL.SIM.WORK` for contact
- Rotary tables and articulating heads in the kinematics (from P3)
- Dialect profiles of known server behaviour, from public behaviour or own observation only
- `device.start_sim@1` and `device.stop_sim@1` (I1) for every simulator kind (I++, pose, scan, gauge, environment, surface, camera), and declaring every simulator source as simulated
- Standalone binary for CI and for outside developers

**Out** (and which node owns it instead):
- The virtual workpiece and contact model (owned by `HAL.SIM.WORK`)
- L2 error injection (owned by `HAL.SIM.ERR`) and L3 replay (owned by `HAL.SIM.REPLAY`)
- The I++ client (owned by `HAL.IPP`)
- Conformance tests (owned by `HAL.SIM.CONF`)
- Offline program simulation with machine models (owned by `CMM`)

## Requirements

- **REQ-HAL.SIM.IPPSIM-001** The simulator shall accept I++ DME client connections over TCP and implement session, tool, motion, point-measurement, scan, event and error semantics as defined by the `CncMotion@1` package.
- **REQ-HAL.SIM.IPPSIM-002** At L0, the simulator shall reply to each client line from a scenario file and shall report `scenario_mismatch` as an I++ error when the client sends a line the scenario does not expect.
- **REQ-HAL.SIM.IPPSIM-003** At L1, the simulator shall move a Cartesian three-axis machine with ideal kinematics within configured axis limits and report measured points from contact with the virtual workpiece.
- **REQ-HAL.SIM.IPPSIM-004** Every simulator started through `device.start_sim@1` shall declare itself simulated in its capability declaration, so the runtime stamps its batches `simulated = true`.
- **REQ-HAL.SIM.IPPSIM-005** The simulator source shall contain no text from the I++ DME specification and shall be releasable under a permissive licence independent of the product licence [unverified pending D-001 follow-up].
- **REQ-HAL.SIM.IPPSIM-006** `device.start_sim@1` and `device.stop_sim@1` shall be class I1 and exposed on MCP, and the simulator shall run headless on every supported CI platform.
- **REQ-HAL.SIM.IPPSIM-007** A dialect profile shall change only documented observable behaviours (unsupported commands, event timing, error codes) and shall cite its source as public behaviour or own observation.

## Interfaces

- **Offers:** `fid-ipp-sim` binary; scenario format; dialect profile format; the two launcher commands; DEV.SIM.01 catalog row.
- **Consumes:** `HAL.IDL.CNC` semantics; `HAL.SIM.WORK` contact queries; founder-written I++ spec cards.

## Decomposition hints

PROTO (framing, queues, events, errors), L0 (scenario runner; a P0 leaf), KIN (L1 Cartesian kinematics), ROT (rotary tables and heads, P3), DIAL (dialect profiles), LAUNCH (launcher commands; likely a leaf).

## Open questions

- The seed places DEV.SIM.01 in P0 while concept doc 08 calls the L0 skeleton a P1b stretch; the L0 scenario runner can start in P0, L1 needs the P2 spec cards.
- Use of the I++ name ("I++ DME-compatible") awaits a trademark check [unverified].

## Children

_None yet: status `open`._
