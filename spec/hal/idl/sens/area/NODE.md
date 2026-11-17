---
id: HAL.IDL.SENS.AREA
title: AreaScan@1 package (point frames)
level: 4
parent: HAL.IDL.SENS
kind: feature
status: open
phase: P3
size: L
owners: []
claimed_by:
satisfies: [REQ-HAL.IDL.SENS-001]
covers: []
depends_on: [HAL.IDL.SENS.STREAM, HAL.IDL.BASE, HAL.IDL.UNITS, HAL.IDL.GEN.FMT]
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# HAL.IDL.SENS.AREA: AreaScan@1 package

## Purpose

The `AreaScan@1` package for structured-light and other area scanners: configure, capture and point frames with optional normals and a pose reference. Frames are columnar and encoded like line profiles.

## Scope

**In:**
- Settings: exposure, projection pattern set (opaque vendor ID), measuring volume (m)
- Operations: `capture` (D1; one frame), streaming capture via the template
- `PointFrame`: encoded x, y, z arrays, optional normals, optional intensity or colour, validity mask, frame ID, pose reference, timestamps
- `AreaScan-core@1` conformance profile

**Out:**
- Registration of frames into a cloud (owned by `SCAN`)
- Pose streams (owned by `HAL.IDL.SENS.PORT`)

## Requirements

- **REQ-HAL.IDL.SENS.AREA-001** A `PointFrame` shall carry encoded x, y and z arrays with target unit m, optional unit normals pointing out of the material, a validity mask, a frame ID, an optional pose reference and `t_dev` and `t_host`.
- **REQ-HAL.IDL.SENS.AREA-002** `capture` shall be class D1, and a frame larger than the batch-size recommendation shall be split into tiles that share the frame ID and carry a tile index.
- **REQ-HAL.IDL.SENS.AREA-003** `AreaScan-core@1` shall check array lengths, normal unit length, tile completeness and the `Stream-common@1` items.

## Interfaces

- **Offers:** `AreaScan@1` and `AreaScan-core@1`.
- **Consumes:** `HAL.IDL.SENS.STREAM`, `HAL.IDL.BASE`, `HAL.IDL.UNITS`, `HAL.IDL.GEN.FMT`.

## Decomposition hints

One M leaf once the P3 scanner work starts.

## Open questions

- Whether colour is needed for any metrology workflow in scope, or only for display, is open [unverified].

## Children

_None yet: status `open`._
