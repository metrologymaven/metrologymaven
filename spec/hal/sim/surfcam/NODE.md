---
id: HAL.SIM.SURFCAM
title: Surface-topography, profile and camera simulators
level: 3
parent: HAL.SIM
kind: module
status: open
phase: P3
size: L
owners: []
claimed_by:
satisfies: [REQ-HAL.SIM-001, REQ-HAL.SIM-005]
covers: []
depends_on: [HAL.IDL.SENS, HAL.SIM.WORK, HAL.SIM.IPPSIM]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# HAL.SIM.SURFCAM: Surface-topography, profile and camera simulators

## Purpose

Synthetic `HeightMap`, `Profile` and `Camera` sources: areal topography from analytic and seeded-random surfaces, stylus profiles and roundness traces, and rendered camera frames of the virtual workpiece with known intrinsics. They complete REQ-HAL-002 (a simulator for every device class) for the later-phase capabilities.

## Scope

**In:**
- `HeightMap` simulator: analytic surfaces (sinusoids, steps) and seeded random surfaces with known parameters
- `Profile` simulator: stylus traverse profiles and roundness traces with known form and waviness
- `Camera` simulator: frames of the virtual workpiece with known intrinsics and pose, in GenICam pixel formats
- Declaring every source simulated

**Out** (and which node owns it instead):
- Surface-texture evaluation (owned by `SCAN` or a later texture domain)
- Real profilometer and camera drivers (owned by `HAL.DRV`)

## Requirements

- **REQ-HAL.SIM.SURFCAM-001** The `HeightMap`, `Profile` and `Camera` simulators shall implement their capability packages and pass their conformance profiles.
- **REQ-HAL.SIM.SURFCAM-002** Each analytic surface or profile shall ship with its generating parameters, so evaluators can be checked against known values.
- **REQ-HAL.SIM.SURFCAM-003** The camera simulator shall publish the exact intrinsics and extrinsics used to render each frame.
- **REQ-HAL.SIM.SURFCAM-004** Every simulator in this node shall declare itself simulated.

## Interfaces

- **Offers:** three simulators registered with the launcher.
- **Consumes:** `HAL.IDL.SENS`, `HAL.SIM.WORK`.

## Decomposition hints

HMAP (height map), PROF (profile and roundness), CAM (camera rendering on the CPU).

## Open questions

- Camera rendering must be CPU-based to stay deterministic; GPU rendering is display-only (CONSTITUTION C2).

## Children

_None yet: status `open`._
