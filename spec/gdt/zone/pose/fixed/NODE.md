---
id: GDT.ZONE.POSE.FIXED
title: Fixed-pose zone evaluation
level: 4
parent: GDT.ZONE.POSE
kind: task
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-GDT.ZONE.POSE-001, REQ-GDT.ZONE.POSE-002]
covers: []
depends_on: [GDT.ZONE.ZTYPE, GDT.DRF.BUILD]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# GDT.ZONE.POSE.FIXED: Fixed-pose zone evaluation

## Purpose

The no-optimisation path of the zone engine: when the DRF leaves no free DOF (or the caller supplies a reference pose, such as a least-squares pose while minimum zone is unavailable in P1a), the usage is simply evaluated at that pose with `GDT.ZONE.ZTYPE`. It returns a `ZoneSolution` of the same shape as the solver path, with certificate kind `fixed_pose`, so callers handle one type.

## Scope

**In:** `ZoneSolution` type (shared with SOLVE); evaluation at the DRF pose or a caller pose; `reference` field (`drf_pose` or `caller_pose`); driving index pass-through.
**Out:** any solver call (owned by `GDT.ZONE.POSE.SOLVE`); the conservative-upper-bound flag on form results (owned by `GDT.FORM.FCORE`).

## Requirements

- **REQ-GDT.ZONE.POSE.FIXED-001** When the free DOF set is empty, the module shall return usage and driving index from `GDT.ZONE.ZTYPE` at the DRF pose with certificate kind `fixed_pose` and `reference: drf_pose`, without calling a solver.
- **REQ-GDT.ZONE.POSE.FIXED-002** When the caller supplies a reference pose, the module shall evaluate at that pose and record `reference: caller_pose` with the pose digest.
- **REQ-GDT.ZONE.POSE.FIXED-003** For an offset shape, the module shall obtain deviations at the pose from the caller's `DeviationOracle` exactly once.
- **REQ-GDT.ZONE.POSE.FIXED-004** The returned pose shall be bit-identical to the input pose, and errors from ZTYPE shall pass through unchanged.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/gdt/zone/fixed_planes.json` (two-plane zone, full DRF, dyadic points with planted maximum |n·(p − o)| = 0.0125 mm), when evaluated, then usage = 0.025 mm exactly, the driving index is the planted one, and the certificate kind is `fixed_pose`.
- **AC-2** Given a caller pose, then `reference: caller_pose` and the pose digest matches the input's.
- **AC-3** Given an offset shape with a counting oracle stub, then the oracle is called exactly once.
- **AC-4** (error path) Given an empty point set, then `empty_points` from ZTYPE.

## Design

- **Registry command(s):** none (internal kernel API).
- **Data structures:** `ZoneSolution { usage_m, pose, driving_index, free: DofSet, cert: ZoneCert{FixedPose | Certified(Certificate) | LocalOnly(Certificate)}, reference: DrfPose | CallerPose{digest} | Solved, alg_id }`.
- **Algorithm:** transform points by the pose; call `ZoneShape::usage`; wrap.
- **Errors:** pass-through `ZoneError`.
- **Determinism:** D0.
- **Performance budget:** as ZTYPE (1 M points < 10 ms) [unverified].
- **Crate:** `fid-gps` (module `zone::pose`). Algorithm ID `gps.zone.fixed@1.0.0`.

## Test fixtures

Dyadic analytic fixtures (CC0, in-repo); an oracle stub that counts calls.

## Build prompt

> In crate `fid-gps` (kernel, pure, KNP-1), module `zone::pose`, define `ZoneSolution { usage_m, pose, driving_index, free, cert: ZoneCert, reference, alg_id }` and `ZoneCert { FixedPose, Certified(Certificate), LocalOnly(Certificate) }`, plus the `DeviationOracle` trait (`deviations_at(pose) -> Result<Vec<f64>, _>` plus normals for linearisation). Implement `evaluate_fixed(shape, points, pose, reference, oracle) -> Result<ZoneSolution, ZoneError>`: usage and driving index from `ZoneShape::usage` at the given pose, no solver, certificate `FixedPose`, `reference` = `DrfPose` or `CallerPose{digest}`, algorithm ID `gps.zone.fixed@1.0.0`; for offset shapes call the oracle exactly once. Tests per the acceptance criteria. Constraints: CONSTITUTION §8 (no `unwrap`/`expect`/`panic`/`unsafe`), KNP-1 (no FMA, fixed-order reductions, total-order max, no std `HashMap`/`HashSet`, NaN is an error).

## Open questions

- None.

## Children

_Leaf: none._
