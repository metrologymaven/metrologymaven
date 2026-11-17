---
id: ALN.RPS.ITER
title: Iterative 3-2-1 alignment on nominal-located points
level: 3
parent: ALN.RPS
kind: module
status: open
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-ALN.RPS-003, REQ-ALN.RPS-007]
covers: [PCD.ALN.01]
depends_on: [ALN.RPS.CLASSIC, ALN.RPS.LOCK, ALN.MODEL.ELEM, ALN.QA.REPORT, GEO.DIST, KRN.LSQ]
cli: [fid align run-321-iterative]
mcp: [align_run_321_iterative]
journal: [align.run_321_iterative@1]
updated: 2026-11-16
---

# ALN.RPS.ITER: Iterative 3-2-1 alignment on nominal-located points

## Purpose

The iterative 3-2-1 used when the 3-2-1 inputs are points measured at nominal locations (typically 3 points on the primary surface, 2 on the secondary, 1 on the tertiary) rather than fully fitted features: because each point's deviation along its nominal direction depends on the current alignment, the system re-derives the deviations and re-solves until the locked-direction deviations stop changing. On a live CMM, re-measuring between iterations is a CMM program loop; this node solves offline on given points.

## Scope

**In:**
- Inputs: measured points with nominal positions and nominal directions, grouped as primary (3 or more), secondary (2 or more), tertiary (1 or more)
- The iteration: transform points, re-derive deviations along nominal directions, re-solve the 3-2-1 lock set, repeat
- Convergence threshold (default 1e-6 m [unverified]), iteration limit, iteration count and stopping reason
- `align.run_321_iterative@1`

**Out** (and which node owns it instead):
- Re-measurement on a machine between iterations (`CMM.EXEC`, `CMM.GEN`)
- Single-pass 3-2-1 from fitted elements (`ALN.RPS.CLASSIC`)
- Per-point arbitrary lock sets (`ALN.RPS.LOCK`); surface re-projection of vector points on scan data (`ALN.RPS.VECTOR`)

## Requirements

- **REQ-ALN.RPS.ITER-001** When `align.run_321_iterative` runs, the system shall repeat transform, deviation re-derivation and 3-2-1 re-solve until the largest change in any locked-direction deviation is below the threshold (default 1e-6 m [unverified]) or the iteration limit (default 50) is reached.
- **REQ-ALN.RPS.ITER-002** The alignment shall record the iteration count, the stopping reason and the per-iteration largest change, and a stop by iteration limit shall give status `not_converged`.
- **REQ-ALN.RPS.ITER-003** If a group has fewer than its minimum points (3, 2, 1), then the command shall fail with `insufficient_group_points` naming the group, and journal nothing.
- **REQ-ALN.RPS.ITER-004** Given noise-free synthetic points with a known transform, the converged pose shall reproduce every point's nominal along its nominal direction within 1e-9 m [unverified].
- **REQ-ALN.RPS.ITER-005** The iteration shall run in the CPU kernel under KNP-1 and declare class D0 (fixed iteration schedule) or D3.

## Interfaces

- **Offers:** `align.run_321_iterative@1` (W2; params `primary`, `secondary`, `tertiary` point groups with nominals, `threshold`, `max_iterations`, axis assignment, `parent`).
- **Consumes:** `ALN.RPS.CLASSIC` (or `ALN.RPS.LOCK` with a 3-2-1 lock pattern) per iteration; GEO projection; `ALN.MODEL.ELEM`; `ALN.QA.REPORT`.

## Decomposition hints

GROUPS (point groups and their 3-2-1 lock pattern), LOOP (iteration and convergence), CMD (command). It may reduce to `ALN.RPS.LOCK` with a fixed lock pattern plus an outer loop, in which case it becomes one S leaf.

## Open questions

- Incumbent definitions of "iterative" 3-2-1 differ in what is re-derived between iterations [unverified]. A domain expert should confirm the re-derivation rule (deviation along nominal direction, or re-fitting the primary plane from points) before this becomes a leaf.

## Children

_None yet: status `open`._
