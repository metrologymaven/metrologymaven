---
id: SURF.PROF
title: 2D profile roughness, waviness and material ratio (ISO 21920)
level: 2
parent: SURF
kind: subsystem
status: open
phase: P3
size: XL
owners: []
claimed_by:
satisfies: [REQ-SURF-002]
covers: []
depends_on: [SURF.FILT, KRN.NUM, UNC.DEC]
cli: [fid surf eval-profile]
mcp: [surf_eval_profile]
journal: [surf.eval_profile]
updated: 2026-11-22
---

# SURF.PROF: 2D profile roughness, waviness and material ratio (ISO 21920)

## Purpose

Deterministic evaluation of 2D profile surface texture parameters under ISO 21920-2/3 and ASME B46.1 (plus documented legacy ISO 4287 mapping) over filtered primary (`P`), waviness (`W`) and roughness (`R`) profiles.

## Scope

**In:** Height (`Ra`, `Rq`, `Rz`, `Rp`, `Rv`, `Rt`, `Rsk`, `Rku`), spatial (`RSm`), hybrid (`Rdq`), Abbott-Firestone material-ratio (`Rmr`, `Rdc`, `Rk`, `Rpk`, `Rvk`, `Mr1`, `Mr2`), section-length segmentation, 16%-rule and max-rule tolerance verdicts via `UNC.DEC`.
**Out:** Filtration (`SURF.FILT`); areal height maps (`SURF.AREAL`); contour geometry (`SURF.CONT`).

## Requirements

- **REQ-SURF.PROF-001** The subsystem shall evaluate `P`, `W` and `R` height, spacing, hybrid and material-ratio parameters under ISO 21920-2/3 and ASME B46.1 with explicit dialect selection and clause citations.
- **REQ-SURF.PROF-002** Every profile result shall emit a standard ResultRecord carrying the filter chain digest, nesting indices, evaluation length and `interpretation: provisional` status where unreviewed (D-045).

## Children

_None yet: status `open`._
