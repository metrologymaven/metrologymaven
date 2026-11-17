---
id: HAL.DRV.ARE
title: Structured-light, handheld and terrestrial scanner integration
level: 3
parent: HAL.DRV
kind: module
status: open
phase: P2
size: L
owners: []
claimed_by:
satisfies: [REQ-HAL.DRV-001, REQ-HAL.DRV-005, REQ-HAL.DRV-006]
covers: [SA.INS.03, DEV.ARE.01, DEV.ARE.02]
depends_on: [HAL.ACQ.FILEDEV, HAL.IDL.PROFILE, HAL.IDL.SENS, IO.MESH, IO.CLOUD]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# HAL.DRV.ARE: Structured-light, handheld and terrestrial scanner integration

## Purpose

Brings area scanners into the platform: first through the `FILE` path (meshes and point clouds exported by the vendor's acquisition software, including terrestrial laser scanners), mapped onto file pseudo-devices with an instrument profile; later through an `SDK` path for structured-light scanners with an open capture SDK. The file path is what makes scan-file inspection usable before any live scanner integration.

## Scope

**In:**
- `FILE` path for DEV.ARE.01 and terrestrial scanners (SA.INS.03): the mapping from IO-read meshes and clouds to file pseudo-devices, device identity from file metadata, instrument-profile choice
- Instrument profiles (spec-sheet based) for common scanner classes, delivered with `UNC.INST`
- `SDK` path for DEV.ARE.02: an `AreaScan` Tier-2 host for a scanner with an open capture SDK (P4)
- Catalog entries

**Out** (and which node owns it instead):
- Parsing mesh and cloud formats (owned by `IO.MESH` and `IO.CLOUD`)
- Stamping file observations (owned by `HAL.ACQ.FILEDEV`)
- Scan processing (owned by `SCAN`)

## Requirements

- **REQ-HAL.DRV.ARE-001** When a mesh or cloud exported by a scanner is imported through the `FILE` path, the resulting observations shall carry trust `declared` (or `unknown` without device identity) and the chosen instrument profile by digest.
- **REQ-HAL.DRV.ARE-002** Device identity read from scanner file metadata shall be stored as `UntrustedText` and shall be shown in the catalog entry only as an example, never as a verified device.
- **REQ-HAL.DRV.ARE-003** The `SDK` path for DEV.ARE.02 shall pass `AreaScan-core@1` against the area-scan simulator before its catalog status can be `supported`.
- **REQ-HAL.DRV.ARE-004** While no vendor-terms record exists for a capture SDK, DEV.ARE.02 shall be `blocked`.

## Interfaces

- **Offers:** `FILE`-path mapping for scanners; instrument profile references; catalog entries for DEV.ARE.*.
- **Consumes:** `HAL.ACQ.FILEDEV`, `HAL.IDL.PROFILE`, `IO.MESH`, `IO.CLOUD`; later `HAL.HOST.SDK` and `HAL.SIM.SCANSIM`.

## Decomposition hints

FILEMAP (file-path mapping and metadata; likely a leaf), PROFS (scanner-class profiles with `UNC.INST`), SDK (capture-SDK host, P4).

## Open questions

- The seed catalog puts DEV.ARE.01 in P1b, while `HAL.DRV` (and therefore this node) is P2. The P1b scan-file journey is delivered by `IO` readers plus `HAL.ACQ.FILEDEV` (P1a); this node adds the scanner-specific mapping and catalog entry in P2. If the catalog entry itself must exist in P1b, `HAL.DRV` needs to move earlier by a decision.

## Children

_None yet: status `open`._
