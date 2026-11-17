---
id: ALN.XFM
title: Point-pair, scaled and known-matrix transforms
level: 2
parent: ALN
kind: subsystem
status: split
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-ALN-001, REQ-ALN-002]
covers: [GOM.ALN.09, SA.XFM.01, SA.XFM.05, PCD.ALN.06]
depends_on: [ALN.MODEL, ALN.QA, GEO, KRN]
cli: [fid align run-point-pairs*, fid align set-matrix*, fid align run-leapfrog*]
mcp: [align_run_point_pairs*, align_set_matrix*, align_run_leapfrog*]
journal: [align.run_point_pairs*, align.set_matrix*, align.run_leapfrog*]
updated: 2026-11-16
---

# ALN.XFM: Point-pair, scaled and known-matrix transforms

## Purpose

Closed-form transforms between corresponding point sets (rigid, or similarity with uniform scale), transforms entered as a known matrix, and the single-device leapfrog (move the device, re-measure common points, transform back). These are non-iterative, correspondence-given methods, which `ALN.PRE` and `LVM` call as building blocks.

## Scope

**In:**
- Point-to-point best-fit transform from N ≥ 3 corresponding pairs, with optional per-pair weights and per-axis DoF locks
- Optional uniform scale (similarity transform), off by default
- Known-matrix transform: enter or import a 4×4 matrix or translation plus rotation in a stated convention, with validation
- Single-device leapfrog: compute the station-move transform from common points measured before and after a device move, and apply it to subsequent data

**Out** (and which node owns it instead):
- Choosing correspondences interactively or automatically (`ALN.PRE`, `VIZ`)
- Multi-instrument network adjustment and bundle adjustment (`LVM`)
- Device-level leapfrog bookkeeping (which points, which station) (`HAL`)
- Iterative surface best-fit (`ALN.BFIT`)
- Uncertainty of the transform (`UNC`)

## Requirements

- **REQ-ALN.XFM-001** When given N ≥ 3 non-collinear corresponding point pairs, the system shall compute the rigid transform minimising the (weighted) sum of squared pair distances.
- **REQ-ALN.XFM-002** When scale is enabled, the system shall compute a similarity transform with one uniform scale factor and report the scale as a dimensionless value.
- **REQ-ALN.XFM-003** Given noise-free synthetic pairs with a known transform, the recovered transform shall reproduce every target point within 1e-12 m for parts within 10 m of the origin [unverified].
- **REQ-ALN.XFM-004** If a supplied known matrix is not a proper rigid transform (orthonormality error above 1e-12 or determinant not +1 within 1e-12) and scale was not declared, then the system shall reject it with a typed error.
- **REQ-ALN.XFM-005** A known-matrix transform shall record its rotation convention, units and source, and authored decimals shall keep their value and unit (CONSTITUTION §4).
- **REQ-ALN.XFM-006** When a leapfrog is run on common points measured before and after a device move, the system shall produce a station-move alignment chained to the previous station alignment and report per-point residuals.
- **REQ-ALN.XFM-007** Point-pair transforms shall be computed by the CPU kernel under KNP-1 with determinism class D0.

## Interfaces

- **Offers:** `align.run_point_pairs@1`, `align.set_matrix@1`, `align.run_leapfrog@1` (all W2); a pure point-pair solver callable as R1 compute by `ALN.PRE` and `LVM`.
- **Consumes:** KRN linear algebra (fid-num); GEO transform types; `ALN.MODEL`; `ALN.QA`; HAL observation sets for leapfrog common points.

## Decomposition hints

PAIRS (rigid point-pair solver with weights and locks), SCALE (similarity), MATRIX (known-matrix entry and validation), LEAP (single-device leapfrog).

## Split rationale

The split follows the decomposition hints, one child per **source of the transform**: correspondences (PAIRS), correspondences with scale (SCALE), a user-given transform (MATRIX) and a device move (LEAP). All four are small and closed-form or fixed-schedule, so all are finished as leaves. SCALE extends PAIRS' command with a parameter instead of adding a command, so the CLI/MCP surface stays as the parent declared.

Alternatives considered:
- **SCALE inside PAIRS.** Rejected: the reflection-and-scale derivation and its ppm reporting are a separate, small test surface, and keeping PAIRS rigid-only makes it the stable building block for PRE and LVM.
- **LEAP in LVM.** Rejected: the single-device move needs no network; LVM.MOVE remains the network workflow and calls PAIRS.

Phase: LEAP is tagged P3 with the live-portable schedule; the node stays P1a.

## Open questions

- PCD.ALN.06 (leapfrog) is split between HAL (device bookkeeping), LVM (multi-station networks) and here (the single transform). Confirm with LVM's expansion that it does not also claim the single-move transform.

- Resolution for the leapfrog question: `ALN.XFM.LEAP` owns only the single-device move; `LVM.MOVE` should call `ALN.XFM.PAIRS`'s R1 solver for its quick pose solve. LVM.MOVE's text ("via ALN point-pair transforms") is consistent with this.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| ALN.XFM.PAIRS | Rigid point-pair transform solver (leaf) | 001 003 007 | SA.XFM.01 |
| ALN.XFM.SCALE | Similarity point-pair transform with uniform scale (leaf) | 002 003 007 | SA.XFM.05 |
| ALN.XFM.MATRIX | Known-matrix transform entry and validation (leaf) | 004 005 | GOM.ALN.09 |
| ALN.XFM.LEAP | Single-device leapfrog transform (leaf) | 006 | PCD.ALN.06 |
