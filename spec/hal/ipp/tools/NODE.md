---
id: HAL.IPP.TOOLS
title: Tools, probe heads, changers and rotary tables via the controller
level: 3
parent: HAL.IPP
kind: module
status: open
phase: P2
size: L
owners: []
claimed_by:
satisfies: [REQ-HAL.IPP-001, REQ-HAL.IPP-005]
covers: [PCD.MCH.04, DEV.CMM.04, DEV.CMM.05]
depends_on: [HAL.IPP.SESS]
cli: [fid device read-config-cmm]
mcp: [device_read_config_cmm]
journal: [device.read_config_cmm@1]
updated: 2026-11-16
---

# HAL.IPP.TOOLS: Tools, probe heads, changers and rotary tables via the controller

## Purpose

Reads and uses what the controller reports about its measuring hardware: tools and styli, articulating heads, tool changers and racks, scanning probes and rotary tables. It journals that configuration by digest at session start and exposes tool changes, head angles and rotary-table moves as `CncMotion` operations for the Motion Executive. It never writes probe constants or compensation data to the controller.

## Scope

**In:**
- `device.read_config_cmm@1` (D1): tools, stars, tip radii, heads and angles, changers and racks, scanning-probe parameters, rotary tables
- Journaling the configuration digest at session start
- Tool change, head-angle and rotary-table motion as M1 operations issued for the Motion Executive
- Rotary-table angle reports as `Rotary` batches

**Out** (and which node owns it instead):
- Probe qualification and probe models (owned by `CMM`)
- Writing probe data (A0; not offered)
- Measurement mapping (owned by `HAL.IPP.MEAS`)

## Requirements

- **REQ-HAL.IPP.TOOLS-001** At session start, the client shall read the controller's tool, head, changer and rotary-table configuration and journal it as a digest with the full content in the content-addressed store.
- **REQ-HAL.IPP.TOOLS-002** The client shall define no operation that writes probe constants, tip data or compensation data to the controller.
- **REQ-HAL.IPP.TOOLS-003** Tool change, head-angle change and rotary-table motion shall be issued only as M1 instructions from the Motion Executive.
- **REQ-HAL.IPP.TOOLS-004** When the controller reports a rotary-table angle, the client shall emit a `Rotary` batch with the angle in radians and its timestamp.
- **REQ-HAL.IPP.TOOLS-005** If the configuration reported during a session differs from the digest journaled at its start, then the client shall raise a `device.event` with both digests.

## Interfaces

- **Offers:** `device.read_config_cmm@1`; tool and rotary operations for the Motion Executive; `Rotary` batches.
- **Consumes:** `HAL.IPP.SESS`.

## Decomposition hints

CFG (configuration read and digest), TOOLCHG (tool change and racks), HEAD (head angles), ROT (rotary tables).

## Open questions

- Which head and rotary features shipping servers implement is unverified (parent open question); concept doc 08 places head `Rotary` via I++ in P3.

## Children

_None yet: status `open`._
