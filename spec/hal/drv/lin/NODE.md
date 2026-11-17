---
id: HAL.DRV.LIN
title: Line-profile sensor drivers
level: 3
parent: HAL.DRV
kind: module
status: open
phase: P2
size: XL
owners: []
claimed_by:
satisfies: [REQ-HAL.DRV-001, REQ-HAL.DRV-003, REQ-HAL.DRV-006]
covers: [DEV.LIN.01, DEV.LIN.02, DEV.LIN.03]
depends_on: [HAL.IDL.SENS, HAL.HOST.SDK, HAL.SIM.SCANSIM, HAL.SIM.CONF]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# HAL.DRV.LIN: Line-profile sensor drivers

## Purpose

`LineScan` drivers for industrial line-profile sensors through vendor SDKs in Tier-2 hosts. The first sensor family with an open SDK and an emulator is the P2 target; others follow. These drivers prove the high-rate data path (10 000 profiles per second) on real hardware.

## Scope

**In:**
- One Tier-2 host per sensor family, built from the SDK templates (C++ is the expected fit for C SDKs [unverified])
- Mapping vendor profile output to `LineScan` batches with declared scaled-integer encoding, intensity and validity
- Configuration mapping (exposure, spacing, trigger mode) and namespaced vendor extensions
- Use of a vendor emulator in CI where its licence allows [unverified]
- Catalog entries for DEV.LIN.01–03 with recorded vendor terms

**Out** (and which node owns it instead):
- Line scanners on arms and trackers (owned by `HAL.DRV.ARM` and `HAL.DRV.TRK`)
- Laser line sensors exposed through a CMM controller (owned by `HAL.IPP.MEAS`)
- Profile processing (owned by `SCAN`)

## Requirements

- **REQ-HAL.DRV.LIN-001** Each line-sensor driver shall emit raw profiles in the sensor's native encoding with a declared encoding descriptor, never converting to f64 itself.
- **REQ-HAL.DRV.LIN-002** The first supported sensor family shall sustain its rated profile rate up to 10 000 profiles per second into the runtime without gaps on reference hardware [unverified until measured].
- **REQ-HAL.DRV.LIN-003** Each driver shall pass `LineScan-core@1` against the line-scan simulator before its catalog status can be `supported`.
- **REQ-HAL.DRV.LIN-004** While no vendor-terms record exists for a sensor family's SDK, its catalog status shall be `blocked`.

## Interfaces

- **Offers:** Tier-2 line-sensor hosts; catalog entries for DEV.LIN.*.
- **Consumes:** `HAL.IDL.SENS`, `HAL.HOST.SDK`, `HAL.SIM.SCANSIM`, `HAL.SIM.CONF`.

## Decomposition hints

One child per sensor family (LIN1, LIN2, LIN3) plus EMU (emulator-based CI).

## Open questions

- SDK registration and redistribution terms for every listed family are unverified (seed catalog).

## Children

_None yet: status `open`._
