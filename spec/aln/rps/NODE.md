---
id: ALN.RPS
title: 3-2-1, iterative 3-2-1, RPS and sheet-metal alignment
level: 2
parent: ALN
kind: subsystem
status: split
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-ALN-004]
covers: [GOM.ALN.04, GOM.ALN.05, PCD.ALN.01, PCD.ALN.03, PW.ALN.04, PW.ALN.05, SA.XFM.03]
depends_on: [ALN.MODEL, ALN.QA, GEO, KRN]
cli: [fid align run-321*, fid align run-rps*]
mcp: [align_run_321*, align_run_rps*]
journal: [align.run_321*, align.run_rps*]
updated: 2026-11-16
---

# ALN.RPS: 3-2-1, iterative 3-2-1, RPS and sheet-metal alignment

## Purpose

The sequential, constraint-by-constraint alignments: classic 3-2-1 (primary plane, secondary line, tertiary point), its iterative form for measured points with nominal positions, reference-point-system (RPS) alignment where each reference point locks named directions, and the sheet-metal iterative method on vector points. These differ from best-fit because each input locks specific degrees of freedom rather than contributing to one global objective.

## Scope

**In:**
- 3-2-1 alignment from plane, line and point elements (or equivalent constructions), including frame construction from geometry (SA-style 3-2-1 frame)
- Iterative 3-2-1 on measured points against nominal positions, repeated until a convergence threshold
- RPS alignment: each reference point locks one to three named directions
- Sheet-metal iterative alignment on vector points (surface points measured along nominal vectors), including re-projection along nominal vectors per iteration
- Over- and under-determination checks (exactly six DoF locked, or a reported deficit or surplus)

**Out** (and which node owns it instead):
- Alignment from general geometric elements and datum reference frames (`ALN.DATUM`)
- Global and local surface best-fit (`ALN.BFIT`)
- Fitting the plane, line and point features themselves (`KRN`)
- Residuals and conditioning diagnostics (`ALN.QA`)
- The industry sheet-metal pack's hole, trim and hem checks (`IND`)

## Requirements

- **REQ-ALN.RPS-001** When the user runs a 3-2-1 alignment with a primary plane, a secondary line and a tertiary point, the system shall lock 3, 2 and 1 degrees of freedom in that order and produce a right-handed frame.
- **REQ-ALN.RPS-002** If the secondary element is parallel to the primary normal within a documented angular tolerance, or the tertiary point lies on the secondary line, then the system shall reject the alignment with a typed degeneracy error.
- **REQ-ALN.RPS-003** The iterative 3-2-1 shall repeat until the largest change in the locked-direction deviations falls below a user threshold (default 1e-6 m [unverified]) or an iteration limit, and shall report the iteration count and stopping reason.
- **REQ-ALN.RPS-004** The RPS alignment shall accept per-point direction locks (any non-empty subset of the frame axes) and shall reject any set whose locks do not total exactly six independent degrees of freedom unless the user explicitly selects a least-squares RPS mode.
- **REQ-ALN.RPS-005** Given noise-free synthetic RPS data with a known transform, the residuals in every locked direction shall be at most 1e-12 m.
- **REQ-ALN.RPS-006** The sheet-metal iterative alignment shall, on each iteration, re-evaluate each vector point's deviation along its nominal vector and converge to a pose in which the locked-direction deviations meet the threshold in REQ-ALN.RPS-003.
- **REQ-ALN.RPS-007** These methods shall be computed by the CPU kernel under KNP-1 and declare determinism class D0 (closed-form) or D3 (iterative) in the registry.

## Interfaces

- **Offers:** `align.run_321@1`, `align.run_321_iterative@1`, `align.run_rps@1`, `align.run_rps_vector@1` (all W2), each producing an `Alignment` element.
- **Consumes:** KRN fitted features and constructions; GEO frames and projection along vectors; `ALN.MODEL`; `ALN.QA`.

## Decomposition hints

CLASSIC (3-2-1 and frame from geometry), ITER (iterative 3-2-1), RPSLOCK (RPS direction locks and DoF accounting), VECTOR (sheet-metal vector-point iteration).

## Split rationale

The split follows the decomposition hints: one child per **lock scheme**. CLASSIC is closed-form over fitted elements; LOCK is the general per-point direction-lock solve; ITER and VECTOR are **outer loops** that re-derive deviations and call a lock solve each iteration. ITER and VECTOR stay open because their re-derivation rules need domain confirmation (ITER) or GEO's ray queries (VECTOR).

Alternatives considered:
- **Express 3-2-1 as RPS with a fixed lock pattern** and drop CLASSIC. Rejected: classic 3-2-1 works on fitted planes and lines, not points, and the SA three-point frame is a different construction; it is also the simplest D0 leaf.
- **Merge ITER into VECTOR.** Rejected for now: ITER groups points by 3-2-1 role on CMM data, VECTOR works per point and on scan surfaces; the ITER open question may later merge them.

## Open questions

- Incumbent definitions of "iterative" 3-2-1 differ in what they re-measure between iterations [unverified]; on a live CMM re-measurement is a CMM program loop, not part of this solver. Confirm with a domain expert.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| ALN.RPS.CLASSIC | Classic 3-2-1 alignment and frame from geometry (leaf) | 001 002 007 | GOM.ALN.04, PW.ALN.04, SA.XFM.03 |
| ALN.RPS.ITER | Iterative 3-2-1 alignment on nominal-located points | 003 007 | PCD.ALN.01 |
| ALN.RPS.LOCK | RPS alignment with per-point direction locks (leaf) | 004 005 007 | GOM.ALN.05, PW.ALN.05 |
| ALN.RPS.VECTOR | Sheet-metal iterative alignment on vector points | 003 006 007 | PCD.ALN.03 |
