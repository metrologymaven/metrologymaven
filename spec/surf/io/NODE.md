---
id: SURF.IO
title: X3P and SDF surface formats and softgauge reference suites
level: 2
parent: SURF
kind: subsystem
status: open
phase: P3
size: L
owners: []
claimed_by:
satisfies: [REQ-SURF-006]
covers: []
depends_on: [IO.REG, HAL.IDL.SENS, UNC.RDL]
cli: [fid surf import-x3p, fid surf export-x3p]
mcp: [surf_import_x3p, surf_export_x3p]
journal: [surf.import_x3p, surf.export_x3p]
updated: 2026-11-22
---

# SURF.IO: X3P and SDF surface formats and softgauge reference suites

## Purpose

Bounded, fail-closed readers and writers for open surface-topography formats (ISO 25178-72 X3P and SurfStand SDF) plus project-authored clean-room synthetic Type F1/F2 mathematical softgauges verifying `SURF.FILT`, `SURF.PROF` and `SURF.AREAL` without external data redistribution (D-038, D-052).

## Scope

**In:** Bounded ZIP/XML X3P reader and writer mapped to `HeightMap@1` and `Profile@1`; SDF reader/writer; clean-room synthetic sinusoid/step/sawtooth/Gaussian-surface softgauge generators and arbitrary-precision expected values; fuzzing and fidelity scoring via `IO.CORP`.
**Out:** Proprietary vendor formats (left to `ECO.SDK` format plugins); external NMI redistribution (`D-038`).

## Requirements

- **REQ-SURF.IO-001** X3P and SDF readers shall enforce strict memory, decompression-ratio and XML depth bounds and fail closed on malformed files without panicking (CONSTITUTION §8).
- **REQ-SURF.IO-002** Every surface parameter and filter in `SURF` shall ship with clean-room synthetic softgauge fixtures cross-checked against an independent arbitrary-precision oracle.

## Children

_None yet: status `open`._
