---
id: HAL.DRV.TRK
title: Laser tracker, 6DoF probe and tracker-scanner drivers
level: 3
parent: HAL.DRV
kind: module
status: open
phase: P3
size: XL
owners: []
claimed_by:
satisfies: [REQ-HAL.DRV-001, REQ-HAL.DRV-003, REQ-HAL.DRV-006]
covers: [SA.INS.01, SA.INS.09, DEV.TRK.01, DEV.TRK.02, DEV.TRK.03, DEV.TRK.04]
depends_on: [HAL.IDL.SENS, HAL.IDL.CNC, HAL.HOST.SDK, HAL.SIM.PORT, HAL.SIM.CONF]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# HAL.DRV.TRK: Laser tracker, 6DoF probe and tracker-scanner drivers

## Purpose

`PortablePose`, `TactileProbe` and `LineScan` drivers for laser trackers from several vendors, their 6DoF probes and tracker-mounted scanners, through vendor-built or project-built Tier-2 hosts. Multi-vendor tracker support is a core large-volume capability; it is gated on vendor SDK terms.

## Scope

**In:**
- Tracker hosts per vendor family (DEV.TRK.01–03): pose and point streams, reflector lock state, weather sensors, station state
- 6DoF probes and tracker-mounted scanners (DEV.TRK.04): `TactileProbe` and `LineScan` with pose references
- Mapping vendor coordinates and timestamps to the HAL frame graph and time sources
- Catalog entries with vendor-terms records; vendor-built hosts preferred

**Out** (and which node owns it instead):
- Live probing and scanning workflows, leapfrog and field checks (owned by `HAL.LIVE`)
- Network adjustment of multi-station data (owned by `LVM`)
- Tracker simulators (owned by `HAL.SIM.PORT`)

## Requirements

- **REQ-HAL.DRV.TRK-001** Each tracker driver shall report poses or points with covariance in SI units, reflector state and the weather readings the tracker uses, all with device and host timestamps.
- **REQ-HAL.DRV.TRK-002** Each tracker driver shall pass `PortablePose-core@1` (and `TactileProbe-core@1` or `LineScan-core@1` where it claims them) against the portable simulators before its catalog status can be `supported`.
- **REQ-HAL.DRV.TRK-003** A host built under a vendor NDA shall never be linked into project binaries and shall expose only the public capability packages.
- **REQ-HAL.DRV.TRK-004** While no vendor-terms record exists for a tracker family, its catalog status shall be `blocked`.

## Interfaces

- **Offers:** tracker hosts; catalog entries for DEV.TRK.*.
- **Consumes:** `HAL.IDL.SENS`, `HAL.IDL.CNC` (`TactileProbe`), `HAL.HOST.SDK`, `HAL.SIM.PORT`, `HAL.SIM.CONF`.

## Decomposition hints

One child per vendor family as terms allow (TRKA, TRKB, TRKC), plus SIXDOF (6DoF probes and tracker scanners).

## Open questions

- Vendor SDK terms for every tracker family are unverified; concept doc 08 suggests starting with the vendor most open to partnership [unverified].

## Children

_None yet: status `open`._
