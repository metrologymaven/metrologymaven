---
id: SURF.CONT
title: 2D stylus contour tracing and tip deconvolution
level: 2
parent: SURF
kind: subsystem
status: open
phase: P3
size: L
owners: []
claimed_by:
satisfies: [REQ-SURF-005]
covers: []
depends_on: [KRN.LSQ, KRN.NUM, GEO.TYPES]
cli: [fid surf eval-contour]
mcp: [surf_eval_contour]
journal: [surf.eval_contour]
updated: 2026-11-22
---

# SURF.CONT: 2D stylus contour tracing and tip deconvolution

## Purpose

2D macro-to-micro stylus contour metrology: arc-radius tip compensation/deconvolution, segmentation into lines and circular arcs, nominal 2D contour alignment, and chamfer, radius, angle and break-edge dimensioning.

## Scope

**In:** Stylus tip radius and flank-angle morphological deconvolution, dual-stylus upper/lower trace stitching, automatic line/arc/blend segmentation, nominal-vs-actual 2D contour comparison, Gothic-arch and groove geometry checks.
**Out:** Roughness/waviness filtration (`SURF.FILT`, `SURF.PROF`); 3D mesh slicing (`SCAN.SECT`).

## Requirements

- **REQ-SURF.CONT-001** The subsystem shall compensate stylus tip radius deterministically and flag uncompensated undercut or flank-contact regions rather than fabricating surface points.
- **REQ-SURF.CONT-002** The subsystem shall extract radii, angles, chamfers, intersections and nominal deviations from 2D contour traces into standard ResultRecords.

## Children

_None yet: status `open`._
