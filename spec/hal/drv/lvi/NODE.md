---
id: HAL.DRV.LVI
title: Photogrammetry, total stations and indoor positioning systems
level: 3
parent: HAL.DRV
kind: module
status: open
phase: P3
size: L
owners: []
claimed_by:
satisfies: [REQ-HAL.DRV-001, REQ-HAL.DRV-005, REQ-HAL.DRV-006]
covers: [SA.INS.04, SA.INS.05, SA.INS.06, DEV.PHO.01, DEV.TST.01, DEV.TST.02]
depends_on: [HAL.ACQ.FILEDEV, HAL.IDL.SENS, HAL.HOST.SDK, HAL.SIM.PORT, IO]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# HAL.DRV.LVI: Photogrammetry, total stations and indoor positioning systems

## Purpose

The other large-volume instruments: photogrammetry systems through their exported coordinate files, total stations and theodolites, and indoor GPS-style positioning systems, through exported files or SDK hosts. Their observations feed `LVM` networks alongside trackers.

## Scope

**In:**
- Photogrammetry (DEV.PHO.01, SA.INS.04): mapping exported coordinate files with per-point uncertainty onto file pseudo-devices
- Total stations and theodolites (DEV.TST.01, SA.INS.05): angle and distance observations as `PortablePose`-family data, through files or an SDK host
- Indoor positioning systems (DEV.TST.02, SA.INS.06): position streams through an SDK host
- Catalog entries with vendor-terms records

**Out** (and which node owns it instead):
- Parsing coordinate file formats (owned by `IO`)
- Network adjustment (owned by `LVM`)
- Trackers (owned by `HAL.DRV.TRK`)

## Requirements

- **REQ-HAL.DRV.LVI-001** Photogrammetry coordinates imported through the `FILE` path shall carry trust `declared`, the per-point uncertainty stated in the file when present, and the chosen instrument profile by digest.
- **REQ-HAL.DRV.LVI-002** Total-station observations shall be recorded as raw horizontal angle, vertical angle and slope distance in SI units with their stated uncertainties, not as derived coordinates.
- **REQ-HAL.DRV.LVI-003** Any SDK-path driver in this node shall pass the conformance profile of each capability it claims before its catalog status can be `supported`.
- **REQ-HAL.DRV.LVI-004** While no vendor-terms record exists for an SDK-path device, its catalog status shall be `blocked`.

## Interfaces

- **Offers:** file mappings and hosts; catalog entries for DEV.PHO.*, DEV.TST.*.
- **Consumes:** `HAL.ACQ.FILEDEV`, `HAL.IDL.SENS`, `HAL.HOST.SDK`, `HAL.SIM.PORT`, `IO`.

## Decomposition hints

PHOTO (photogrammetry files; likely a leaf), TST (total stations), IPS (indoor positioning).

## Open questions

- Raw angle-and-distance observations may need their own capability or a `PortablePose` feature flag; this is for `HAL.IDL.SENS` to decide when this node is expanded.
- SA.INS.06 is now covered by DEV.TST.02 in the seed catalog.

## Children

_None yet: status `open`._
