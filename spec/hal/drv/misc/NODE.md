---
id: HAL.DRV.MISC
title: Vision cameras, encoder retrofits and shop-floor protocols
level: 3
parent: HAL.DRV
kind: module
status: open
phase: P3
size: XL
owners: []
claimed_by:
satisfies: [REQ-HAL.DRV-001, REQ-HAL.DRV-003, REQ-HAL.DRV-006, REQ-HAL.DRV-007]
covers: [PCD.LAS.03, DEV.CMM.03, DEV.CAM.01, DEV.CTL.01]
depends_on: [HAL.IDL.SENS, HAL.IDL.CNC, HAL.HOST.TIER1, HAL.HOST.SDK, HAL.SIM.SURFCAM, HAL.SIM.CONF]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# HAL.DRV.MISC: Vision cameras, encoder retrofits and shop-floor protocols

## Purpose

Three later-phase integration families grouped to stay within the child limit: industrial cameras for vision measurement through an open camera standard, encoder interfaces for manual CMMs and retrofits, and shop-floor protocols (OPC UA, MTConnect) as publish-or-monitor paths that can never create device I/O or motion.

## Scope

**In:**
- `Camera` driver through GenICam-compatible cameras for vision measurement (DEV.CAM.01, PCD.LAS.03) [unverified standard details]
- Encoder-interface drivers for manual CMMs and retrofits (DEV.CMM.03): position stream and probe triggers as `TactileProbe` and `PortablePose`-style data, never `CncMotion`
- OPC UA server and client and an MTConnect agent (DEV.CTL.01): publish HAL health, run state and results; monitor machine state as context
- Catalog entries

**Out** (and which node owns it instead):
- Vision measurement through I++ camera objects (may move to `HAL.IPP` later)
- Image-processing algorithms for vision measurement (owned by `SCAN` or a later vision node)
- Results export formats (owned by `IO`)

## Requirements

- **REQ-HAL.DRV.MISC-001** Shop-floor protocol integrations shall expose no write or method call that creates a D1 or M1 command, and a conformance test shall try each exposed node or endpoint and confirm it is read-only.
- **REQ-HAL.DRV.MISC-002** The camera driver shall deliver frames with exact pixel format, exposure and intrinsics references, and shall pass `Camera-core@1` against the camera simulator before its catalog status can be `supported`.
- **REQ-HAL.DRV.MISC-003** Encoder-retrofit drivers shall report positions and probe triggers only and shall implement no motion capability.
- **REQ-HAL.DRV.MISC-004** While no vendor-terms record exists for an SDK-path device in this node, its catalog status shall be `blocked`.

## Interfaces

- **Offers:** camera, encoder and shop-floor integrations; catalog entries for DEV.CAM.*, DEV.CTL.*, DEV.CMM.03.
- **Consumes:** `HAL.IDL.SENS`, `HAL.IDL.CNC` (`TactileProbe`), `HAL.HOST.TIER1`, `HAL.HOST.SDK`, `HAL.SIM.SURFCAM`, `HAL.SIM.CONF`.

## Decomposition hints

CAM (cameras, P3–P4), ENC (encoder retrofits, P3), OPCUA (P4), MTC (MTConnect, P4).

## Open questions

- The seed catalog places DEV.CAM.01 in P3 while concept doc 08 places `Camera` in P4; children will carry their own phases.
- If I++ camera objects land, DEV.CAM.01 and PCD.LAS.03 may move to `HAL.IPP` (parent open question).

## Children

_None yet: status `open`._
