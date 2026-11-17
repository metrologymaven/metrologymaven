---
id: HAL.DRV.ARM
title: Portable arm and arm-scanner drivers
level: 3
parent: HAL.DRV
kind: module
status: open
phase: P3
size: XL
owners: []
claimed_by:
satisfies: [REQ-HAL.DRV-001, REQ-HAL.DRV-003, REQ-HAL.DRV-006]
covers: [SA.INS.02, DEV.ARM.01, DEV.ARM.02, DEV.ARM.03]
depends_on: [HAL.IDL.SENS, HAL.IDL.CNC, HAL.HOST.SDK, HAL.SIM.PORT, HAL.SIM.SCANSIM, HAL.SIM.CONF]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# HAL.DRV.ARM: Portable arm and arm-scanner drivers

## Purpose

`PortablePose`, `TactileProbe` and `LineScan` drivers for portable measuring arms and arm-mounted laser line scanners, through vendor-built or project-built Tier-2 hosts. Arms are the most common shop-floor portable device; the first arm integration is a P3 milestone.

## Scope

**In:**
- Arm hosts per vendor family (DEV.ARM.01–02): pose stream, ball-probe triggers with tip identity and radius, button events
- Arm-mounted line scanners (DEV.ARM.03): `LineScan` with pose references and time-sync declaration
- Mapping vendor frames and timestamps into the HAL frame graph and time sources
- Catalog entries with vendor-terms records; vendor-built hosts preferred

**Out** (and which node owns it instead):
- Live probing, compensation and scanning workflows (owned by `HAL.LIVE`)
- Arm simulators (owned by `HAL.SIM.PORT`)

## Requirements

- **REQ-HAL.DRV.ARM-001** Each arm driver shall report probe-tip centres with tip identity and radius, flagged uncompensated, and never report vendor-compensated surface points as raw observations.
- **REQ-HAL.DRV.ARM-002** Arm-scanner drivers shall declare the time source shared by pose and profile streams, so fused batches can be checked for time-sync sufficiency.
- **REQ-HAL.DRV.ARM-003** Each arm driver shall pass the conformance profiles of the capabilities it claims against the portable and scanner simulators before its catalog status can be `supported`.
- **REQ-HAL.DRV.ARM-004** While no vendor-terms record exists for an arm family, its catalog status shall be `blocked`.

## Interfaces

- **Offers:** arm hosts; catalog entries for DEV.ARM.*.
- **Consumes:** `HAL.IDL.SENS`, `HAL.IDL.CNC` (`TactileProbe`), `HAL.HOST.SDK`, `HAL.SIM.PORT`, `HAL.SIM.SCANSIM`, `HAL.SIM.CONF`.

## Decomposition hints

One child per vendor family (ARMA, ARMB) plus ARMSCAN (arm-mounted scanners).

## Open questions

- If a vendor SDK only returns compensated points, REQ-HAL.DRV.ARM-001 needs an `ExternalClaim` path instead; to be decided per vendor once terms are known [unverified].

## Children

_None yet: status `open`._
