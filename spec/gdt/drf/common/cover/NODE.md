---
id: GDT.DRF.COMMON.COVER
title: Datum target coverage check
level: 4
parent: GDT.DRF.COMMON
kind: task
status: leaf
phase: P1b
size: S
owners: []
claimed_by:
built_in: [fid-gps]
satisfies: [REQ-GDT.DRF.COMMON-003]
covers: []
depends_on: [GDT.DRF.DATUM, GEO.FRAME, KRN.NUM]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# GDT.DRF.COMMON.COVER: Datum target coverage check

## Purpose

Before a datum is simulated from its targets, every target must actually be touched by measured data. This leaf computes, for each point, line or area target, how far the target is from the nearest measured point and fails with `target_not_covered` when that distance exceeds the threshold the resolved row states. It is plain geometry; the threshold is the row's.

## Scope

**In:** distance from a point target to the nearest measured point; for a line target, the largest gap (maximum over target samples of the distance to the nearest measured point) along the line segment; for an area target, the same over a deterministic sample grid of the area; comparison with the row's threshold; per-target report.
**Out:** simulating the datum (owned by `GDT.DRF.COMMON.TARGET`); target geometry (owned by `GDT.DRF.DATUM`); the threshold value (owned by DMT rows).

## Requirements

- **REQ-GDT.DRF.COMMON.COVER-001** For each target the function shall report the coverage distance: for a point target the distance to the nearest measured point, and for line and area targets the maximum, over a deterministic sample set with spacing given by the caller, of the distance to the nearest measured point.
- **REQ-GDT.DRF.COMMON.COVER-002** If any target's coverage distance exceeds the threshold supplied by the resolved row, then the function shall return `target_not_covered{target, distance}` naming the first such target in definition order.
- **REQ-GDT.DRF.COMMON.COVER-003** If no threshold is supplied by the chain, then the function shall return `threshold_missing` rather than assume one.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/gdt/drf/targets_plate.json` (three point targets and a measured grid at 1 mm spacing), when checked with threshold 0.8 mm, then all targets pass and each reported distance matches the analytic nearest-point distance within 1e-15 m.
- **AC-2** Given one point target moved 2 mm outside the grid, then `target_not_covered{target: A2, distance: 2 mm}`.
- **AC-3** Given a line target half outside the data, then the reported coverage distance equals the analytic largest gap.
- **AC-4** (error path) Given a chain without a threshold, then `threshold_missing`.

## Design

- **Registry command(s):** none (kernel function `check_coverage(targets, points, threshold, spacing) -> Result<CoverageReport, CoverError>`).
- **Data structures:** `CoverageReport { per_target: [(label, distance_m)] }`.
- **Algorithm:** k-d tree nearest neighbour (deterministic build order from GEO); line and area sampled at the caller's spacing in fixed order; total-order max.
- **Errors:** `target_not_covered{target, distance}`, `threshold_missing`, `empty_points`, `nan_input`.
- **Determinism:** D0.
- **Performance budget:** 1 M points, 10 targets < 100 ms [unverified].
- **Crate:** `fid-gps` (module `drf::targets::coverage`).

## Test fixtures

Synthetic plates with targets on and off the data (CC0).

## Build prompt

> In crate `fid-gps` (kernel), module `drf::targets::coverage`, implement `check_coverage(targets, points, threshold, spacing)`. For point targets compute the nearest measured point distance; for line and area targets sample deterministically at `spacing` and take the total-order max of nearest distances. Return `target_not_covered{target, distance}` for the first target (definition order) above the threshold; `threshold_missing` when the chain supplies none; never assume a default. Tests per acceptance criteria. Constraints: CONSTITUTION §8, KNP-1 (fixed-order reductions, total-order max, no std `HashMap`/`HashSet`, NaN is an error).

## Open questions

- **Built (fid-gps `drf::targets::coverage`).** AC-1 to AC-4 and REQ-001 to REQ-003 are met; oracle `fixtures/gdt/drf/make_targets_plate.py` (exact rational nearest distances of the pinned samples, the exact largest gap along line targets, hand-labelled area suprema).
- **Target geometry is defined here for now.** `GDT.DRF.DATUM` stores a target only as a geometry reference, so the function takes the resolved geometry: a point, a line segment, a parallelogram area (corner and two edges) or a circular area (centre, normal, radius). When `GDT.DRF.COMMON.TARGET` resolves target references to geometry, it should produce these values.
- **Pinned sample sets.** Lines and parallelograms are sampled at `ceil(length / spacing)` equal steps including both ends and every corner; a disc by a square grid at the spacing in a pinned in-plane basis plus a rim of `max(4, ceil(2 pi r / spacing))` points. The sampled distance is never above the analytic largest gap and reaches it when the gap lies at a sample (an end, a corner or a grid point).
- **Errors beyond the Design list (fail closed):** `bad_threshold` (negative), `bad_spacing` (not positive), `no_targets`, `degenerate_target{target}` (a disc with a zero normal or a negative radius), `too_many_samples{target}` (more than 2^20 samples for one target), and `index{code}` for a k-d tree refusal. `nan_input` also covers infinities. The report carries each target's sample count.

## Children

_Leaf: none._
