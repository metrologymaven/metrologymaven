---
id: HAL.IDL.SENS.CAM
title: Camera@1 package (frames and intrinsics)
level: 4
parent: HAL.IDL.SENS
kind: feature
status: open
phase: P4
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

# HAL.IDL.SENS.CAM: Camera@1 package

## Purpose

The `Camera@1` package for machine-vision and photogrammetry cameras: image frames in GenICam pixel formats, intrinsic calibration references and trigger configuration. It serves P4 photogrammetry and vision workflows.

## Scope

**In:**
- Settings: exposure (s), gain, trigger mode, region of interest (pixels as dimensionless)
- `ImageFrame`: width, height, pixel format (GenICam naming [unverified]), stride, payload, timestamps, frame ID
- Intrinsics reference (camera model ID and digest), never a writable calibration
- `Camera-core@1` conformance profile

**Out:**
- Photogrammetric bundle adjustment (owned by `LVM`)
- Image display (owned by `VIZ`)

## Requirements

- **REQ-HAL.IDL.SENS.CAM-001** An `ImageFrame` shall carry width, height, pixel format, stride, payload digest, `t_dev`, `t_host` and frame ID, and payload length shall equal stride x height.
- **REQ-HAL.IDL.SENS.CAM-002** Camera intrinsics shall be referenced by model ID and digest only; the package shall define no operation that writes intrinsics.
- **REQ-HAL.IDL.SENS.CAM-003** Pixel formats shall come from a closed list taken from the GenICam pixel-format naming by name only [unverified]; any other format shall fail validation.

## Interfaces

- **Offers:** `Camera@1` and `Camera-core@1`.
- **Consumes:** `HAL.IDL.SENS.STREAM`, `HAL.IDL.BASE`, `HAL.IDL.UNITS`, `HAL.IDL.GEN.FMT`.

## Decomposition hints

One M leaf when P4 vision work starts.

## Open questions

- The GenICam pixel-format naming and its licence terms for reuse of names are [unverified].

## Children

_None yet: status `open`._
