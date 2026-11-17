---
id: ALN.RPS.ITER
title: Iterative 3-2-1 alignment on nominal-located points
level: 3
parent: ALN.RPS
kind: feature
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-ALN.RPS-003, REQ-ALN.RPS-007]
covers: [PCD.ALN.01]
depends_on: [ALN.RPS.VECTOR.LOOP, ALN.MODEL.ELEM, ALN.QA.REPORT, ALN.QA.HIST]
side_effect: W2
determinism: D0
cli: [fid align run-321-iterative]
mcp: [align_run_321_iterative]
journal: [align.run_321_iterative@1]
updated: 2026-11-17
---

# ALN.RPS.ITER: Iterative 3-2-1 alignment on nominal-located points

## Purpose

The iterative 3-2-1 used when the inputs are points measured at nominal locations (typically 3 on the primary surface, 2 on the secondary, 1 on the tertiary) rather than fitted features. Each point's deviation along its nominal direction depends on the current alignment, so the system re-derives the deviations and re-solves until the locked-direction deviations stop changing (PC-DMIS iterative alignment [unverified]). It is a grouped preset over the vector-point loop: every point locks its nominal direction, and `ALN.RPS.VECTOR.LOOP` does the iteration. On a live CMM, re-measuring between iterations is a CMM program loop; this node solves offline on given points.

## Scope

**In:** `align.run_321_iterative@1`; primary, secondary and tertiary point groups with nominal positions and directions; group minimum counts; mapping groups to locked vector points with discrete-hit sources; per-group deviation reporting; recording of iteration count, stopping reason and per-iteration largest change.
**Out:** the iteration and solve (`ALN.RPS.VECTOR.LOOP`); scan-surface vector points (`ALN.RPS.VECTOR.SURF`); single-pass 3-2-1 from fitted elements (`ALN.RPS.CLASSIC`); frame-axis lock sets (`ALN.RPS.LOCK`); re-measurement on a machine between iterations (`CMM.EXEC`, `CMM.GEN`).

## Requirements

- **REQ-ALN.RPS.ITER-001** When `align.run_321_iterative` runs, the system shall repeat transform, deviation re-derivation and 3-2-1 re-solve until the largest change in any locked-direction deviation is below the threshold (default 1e-6 m [unverified]) or the iteration limit (default 50) is reached.
- **REQ-ALN.RPS.ITER-002** The alignment shall record the iteration count, the stopping reason and the per-iteration largest change, and a stop by iteration limit shall give status `not_converged`.
- **REQ-ALN.RPS.ITER-003** If a group has fewer than its minimum points (3, 2, 1), then the command shall fail with `insufficient_group_points` naming the group, and journal nothing.
- **REQ-ALN.RPS.ITER-004** Given noise-free synthetic points with a known transform, the converged pose shall reproduce every point's nominal along its nominal direction within 1e-9 m [unverified].
- **REQ-ALN.RPS.ITER-005** The iteration shall run in the CPU kernel under KNP-1 and declare class D0 (fixed iteration schedule) or D3.
- **REQ-ALN.RPS.ITER-006** The re-derivation rule shall be: a point's deviation is the signed distance of the transformed measured point from its nominal position along its nominal unit direction; the alignment shall record it as `rederive: along_nominal`.
- **REQ-ALN.RPS.ITER-007** The command shall map every group point to a locked vector point with a discrete-hit source, in the order primary, secondary, tertiary and then input order, and call `ALN.RPS.VECTOR.LOOP`; if the loop reports `rps_dof_mismatch` (for example all primary and secondary directions parallel), the command shall pass it through and journal nothing.
- **REQ-ALN.RPS.ITER-008** The residual report shall give each point's final deviation tagged with its group, and the RMS per group.
- **REQ-ALN.RPS.ITER-009** With `--dry-run`, the command shall run as R1 and journal nothing.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/aln/rps/iter/block_321.fidproj` (3 points on top face, 2 on side face, 1 on end face; noise-free; known transform 2 mm and 1 degree), when `fid align run-321-iterative --primary p1,p2,p3 --secondary s1,s2 --tertiary t1 --json` runs, then every point's deviation is at most 1e-9 m, status is `converged`, and one event is journaled.
- **AC-2** Given the same with 5 µm noise (seed 21) and 4 primary points, then the mode is `least_squares` and per-group RMS values are reported.
- **AC-3** (error path) Given only one secondary point, then the command fails with `insufficient_group_points{secondary}` and journals nothing.
- **AC-4** Given `--max-iterations 1` on a case that needs two iterations, then the alignment has status `not_converged` with reason `max_iterations`.
- **AC-5** (replay) Exact replay of AC-1 gives `IDENTICAL`.

## Design

- **Registry command:** `align.run_321_iterative@1` (W2; dry-run R1). Params: `primary`, `secondary`, `tertiary`: lists of `{ point: ElementId, nominal: Coord, direction: Vec3 }` (nominal and direction default from the point element's nominal when it has one); `threshold: Length = 1e-6 m`; `max_iterations: u32 = 50`; `parent?`. Result: `{ alignment, status, iterations, per_group_rms }`.
- **Data structures:** none new; groups map onto `VectorPoint { locked: true, source: Hit }`.
- **Algorithm:** validate group sizes; convert units once (journaled); map to vector points; `solve_vector_points` with the hit provider from the parent's composed pose; wrap through `ALN.MODEL.ELEM` with algorithm ID `align.rps.iterative_321@1.0.0`.
- **Errors:** `insufficient_group_points{group}`, `invalid_vector`, `element_not_found`, loop errors passed through.
- **Determinism:** D0 (fixed schedule, single-threaded loop).
- **Performance budget:** under 2 ms for 20 points [unverified until measured].
- **Crate:** `fid-align` (command layer; provisional name).

## Test fixtures

`block_321.fidproj` and noisy variants, generated from an analytic block with seeds (CC0).

## Build prompt

> In crate `fid-align`, register `align.run_321_iterative@1` (W2, dry-run R1, D0; CLI `fid align run-321-iterative`; MCP `align_run_321_iterative`; journal `align.run_321_iterative@1`). Params: primary, secondary, tertiary groups of `{point, nominal, direction}` (defaults from the point element's nominal), threshold (default 1e-6 m), max_iterations (default 50), parent. Fail `insufficient_group_points{group}` below 3, 2, 1 points. Convert lengths once (journaled). Map every point to a locked vector point with a discrete-hit source (order: primary, secondary, tertiary, input order) and call `solve_vector_points` (ALN.RPS.VECTOR.LOOP) from the parent's composed pose; pass `rps_dof_mismatch` through. Wrap with the alignment element API (ALN.MODEL.ELEM), algorithm ID `align.rps.iterative_321@1.0.0`, recording groups, threshold, limit, `rederive: along_nominal`, iteration count, stop reason, per-iteration largest change and per-group RMS; `not_converged` still creates the alignment. Errors create and journal nothing. Tests: AC-1..AC-5. Constraints: CONSTITUTION §3, §4, §8 (no `unwrap`/`expect`/`panic`/`unsafe`).

## Open questions

- Incumbent definitions of "iterative" 3-2-1 differ in what is re-derived between iterations [unverified]. This leaf fixes the rule as deviation along the nominal direction (REQ-006) and records it; a `plane_refit` rule (re-fitting the primary plane from points each iteration) can be added as a new value of `rederive` if a domain expert asks for it. Expert confirmation is still welcome but does not block the leaf.
- The parent's interface listed an axis-assignment parameter; with nominal-located points the frame is the nominals' frame, so no axis assignment is needed (that belongs to `ALN.RPS.CLASSIC`).

## Children

_Leaf: none._
