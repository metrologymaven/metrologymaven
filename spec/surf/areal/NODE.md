---
id: SURF.AREAL
title: 3D areal surface texture and feature segmentation (ISO 25178)
level: 2
parent: SURF
kind: subsystem
status: open
phase: P3
size: XL
owners: []
claimed_by:
satisfies: [REQ-SURF-003]
covers: []
depends_on: [SURF.FILT, KRN.NUM, UNC.DEC]
cli: [fid surf eval-areal]
mcp: [surf_eval_areal]
journal: [surf.eval_areal]
updated: 2026-11-22
---

# SURF.AREAL: 3D areal surface texture and feature segmentation (ISO 25178)

## Purpose

Deterministic evaluation of 3D areal surface topography parameters under ISO 25178-2/3 on `HeightMap@1` grids, including height, spatial, hybrid, functional volume and watershed-segmented feature parameters.

## Scope

**In:** Height (`Sa`, `Sq`, `Sz`, `Ssk`, `Sku`), spatial (`Sal`, `Str`, `Std`), hybrid (`Sdq`, `Sdr`), functional/volume (`Smr`, `Smc`, `Vmp`, `Vmc`, `Vvc`, `Vvv`, `Sk`, `Spk`, `Svk`), deterministic watershed segmentation with Wolf pruning (`Spd`, `Spc`, `S5p`, `S5v`, `S10z`), validity-mask non-measured-point handling, step-height and wear-volume analysis.
**Out:** Areal filtration (`SURF.FILT`); X3P file I/O (`SURF.IO`); 3D optical fringe demodulation (`ECO.SDK` plugins per D-054).

## Requirements

- **REQ-SURF.AREAL-001** The subsystem shall evaluate ISO 25178-2 height, spatial, hybrid, functional volume and watershed feature parameters deterministically under KNP-1 on grids with explicit non-measured-point masks.
- **REQ-SURF.AREAL-002** When non-measured points exceed a configured threshold or invalidate spatial autocorrelation, the evaluator shall report a typed warning or error instead of silently interpolating.

## Children

_None yet: status `open`._
